//! The AI layer.
//!
//! Every call goes through [`AiService`], and every call follows the same path:
//!
//! 1. check the privacy policy - a cloud call needs an explicitly configured provider,
//!    stored key and consent;
//! 2. assemble the minimum text needed, never a whole engagement or database;
//! 3. optionally redact structured secrets, keeping a map so the author still sees real
//!    values in the result;
//! 4. call the provider with a strict response schema;
//! 5. validate the reply against that schema and strip anything unsafe;
//! 6. run the fabrication guard, refusing suggestions that invent identifiers;
//! 7. record the request, and later the accept/edit/reject decision, in `ai_requests` and
//!    the audit log.
//!
//! What this module never does is write to a report. It returns a suggestion; applying it
//! is a separate, human-initiated call into `reports::save_draft`.

pub mod diff;
pub mod guard;
pub mod local;
pub mod openai;
pub mod prompts;
pub mod provider;
pub mod schema;

use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::audit;
use crate::database::{new_id, Database};
use crate::error::{Error, Result};
use crate::models::{
    Actor, AiConfidence, AiDecision, AiFunction, AiProviderKind, AiReviewResult, AiSettings,
    AiSuggestion, AiTextRequest, AuditAction, AuditOutcome, PrivacyMode, Provenance, ReportContent,
    Timestamp,
};
use crate::security::redact::Redactor;
use provider::{AiProvider, ProviderRequest};
use schema::ResponseSchema;

/// Longest assembled report text sent for a quality review.
pub const MAX_REVIEW_CHARS: usize = 24_000;
/// Shortest passage worth sending. Below this the model has nothing to work with.
pub const MIN_INPUT_CHARS: usize = 12;

/// What the UI shows in the AI panel before the user presses anything.
#[derive(Debug, Clone, Serialize)]
pub struct AiStatus {
    pub provider: AiProviderKind,
    pub privacy_mode: PrivacyMode,
    /// e.g. "AI Processing: Local / Private"
    pub banner: String,
    /// Exactly where text goes, in words.
    pub destination: String,
    pub model: String,
    pub enabled: bool,
    /// True when text would leave this machine on the next call.
    pub leaves_machine: bool,
    pub redaction_enabled: bool,
}

/// What a completed call recorded about itself.
struct RequestOutcome<'a> {
    request_id: &'a str,
    function: AiFunction,
    output_chars: usize,
    confidence: AiConfidence,
    detail: serde_json::Value,
}

pub struct AiService {
    provider: Box<dyn AiProvider>,
    settings: AiSettings,
}

impl AiService {
    pub fn new(provider: Box<dyn AiProvider>, settings: AiSettings) -> Self {
        AiService { provider, settings }
    }

    /// Build from settings, resolving the provider. Never fails: a misconfigured cloud
    /// provider degrades to Disabled so the application still starts, and the reason
    /// surfaces when the user tries to use it.
    pub fn from_settings(settings: &AiSettings, api_key: Option<String>) -> Self {
        let provider = provider::build(settings, api_key).unwrap_or_else(|error| {
            tracing::info!(%error, "AI provider unavailable; running disabled");
            Box::new(provider::DisabledProvider)
        });
        AiService {
            provider,
            settings: settings.clone(),
        }
    }

    pub fn status(&self) -> AiStatus {
        let privacy_mode = self.provider.privacy_mode();
        AiStatus {
            provider: self.provider.kind(),
            privacy_mode,
            banner: privacy_mode.banner().to_string(),
            destination: self.provider.destination(),
            model: self.provider.model(),
            enabled: self.provider.kind() != AiProviderKind::Disabled,
            leaves_machine: privacy_mode.leaves_machine(),
            redaction_enabled: self.settings.redact_before_send,
        }
    }

    /// Run a text function and return a suggestion for the human to accept, edit or
    /// reject.
    pub fn suggest(
        &self,
        db: &Database,
        actor: &Actor,
        request: &AiTextRequest,
    ) -> Result<AiSuggestion> {
        let original = request.text.clone();
        if original.trim().chars().count() < MIN_INPUT_CHARS {
            return Err(Error::validation(
                "there is not enough text here for the assistant to work with",
            ));
        }

        let request_id = new_id();
        self.begin_request(db, actor, &request_id, request.function, request)?;

        let mut redactor = Redactor::new();
        let (sent_text, sent_context) = if self.should_redact() {
            (
                redactor.redact(&original),
                redactor.redact(&request.context),
            )
        } else {
            (original.clone(), request.context.clone())
        };

        let instruction = prompts::instruction(request.function, request.summary_kind);
        let provider_request = ProviderRequest {
            system: prompts::SAFETY_RULE.to_string(),
            user: prompts::user_message(&instruction, &sent_context, &sent_text),
            schema: ResponseSchema::TextSuggestion,
            temperature: self.settings.temperature,
            max_output_tokens: self.settings.max_output_tokens,
            timeout_secs: self.settings.request_timeout_secs,
        };

        let response = match self.provider.complete(&provider_request) {
            Ok(response) => response,
            Err(error) => {
                self.fail_request(db, actor, &request_id, &error)?;
                return Err(error);
            }
        };

        let validated =
            match schema::parse_text_suggestion(&response.raw, sent_text.chars().count()) {
                Ok(validated) => validated,
                Err(error) => {
                    self.fail_request(db, actor, &request_id, &error)?;
                    return Err(error);
                }
            };

        // Put the author's real values back before anything is shown or checked.
        let suggestion = redactor.restore(&validated.suggestion);
        let missing_information: Vec<String> = validated
            .missing_information
            .iter()
            .map(|item| redactor.restore(item))
            .collect();
        let notes: Vec<String> = validated
            .notes
            .iter()
            .map(|item| redactor.restore(item))
            .collect();

        // The guard compares against everything the model was given, so a value that came
        // from the context line is not treated as invented.
        let source = format!("{original}\n{}", request.context);
        let fabrications = guard::fabricated_identifiers(&source, &suggestion);
        if !fabrications.is_empty() {
            let listed: Vec<String> = fabrications.iter().map(|item| item.value.clone()).collect();
            let error = Error::AiProvider(format!(
                "the suggestion was discarded because it introduced facts that are not in your \
                 text: {}. Add the detail yourself if it belongs in the report.",
                listed.join(", ")
            ));
            self.block_request(db, actor, &request_id, &error, &listed)?;
            return Err(error);
        }

        let diff = diff::word_diff(&original, &suggestion);
        let created_at = Timestamp::now();

        self.complete_request(
            db,
            actor,
            RequestOutcome {
                request_id: &request_id,
                function: request.function,
                output_chars: suggestion.chars().count(),
                confidence: validated.confidence,
                detail: json!({
                    "function": request.function.as_str(),
                    "target_type": request.target_type,
                    "target_id": request.target_id,
                    "redacted_values": redactor.redacted_count(),
                }),
            },
        )?;

        Ok(AiSuggestion {
            request_id,
            function: request.function,
            provider: self.provider.kind(),
            privacy_mode: self.provider.privacy_mode(),
            model: response.model,
            original,
            suggestion,
            confidence: validated.confidence,
            provenance: proposed_provenance(request.function),
            missing_information,
            notes,
            diff,
            created_at,
        })
    }

    /// Run a report-quality review. Returns warnings only; nothing is changed.
    pub fn review_report(
        &self,
        db: &Database,
        actor: &Actor,
        report_id: &str,
        content: &ReportContent,
    ) -> Result<AiReviewResult> {
        let assembled = assemble_report_text(content);
        if assembled.trim().chars().count() < MIN_INPUT_CHARS {
            return Err(Error::validation(
                "this report has too little content to review",
            ));
        }

        let request_id = new_id();
        let text_request = AiTextRequest {
            function: AiFunction::ReportReview,
            text: assembled.clone(),
            summary_kind: None,
            context: String::new(),
            target_type: "report".to_string(),
            target_id: report_id.to_string(),
        };
        self.begin_request(
            db,
            actor,
            &request_id,
            AiFunction::ReportReview,
            &text_request,
        )?;

        let mut redactor = Redactor::new();
        let sent = if self.should_redact() {
            redactor.redact(&assembled)
        } else {
            assembled.clone()
        };

        let provider_request = ProviderRequest {
            system: prompts::SAFETY_RULE.to_string(),
            user: prompts::user_message(
                &prompts::instruction(AiFunction::ReportReview, None),
                "",
                &sent,
            ),
            schema: ResponseSchema::Review,
            temperature: self.settings.temperature.min(0.3),
            max_output_tokens: self.settings.max_output_tokens,
            timeout_secs: self.settings.request_timeout_secs,
        };

        let response = match self.provider.complete(&provider_request) {
            Ok(response) => response,
            Err(error) => {
                self.fail_request(db, actor, &request_id, &error)?;
                return Err(error);
            }
        };

        // Quotes are validated against the text the model actually saw, then restored, so
        // a quote containing a redacted value still matches and still displays correctly.
        let validated = match schema::parse_review(&response.raw, &sent) {
            Ok(validated) => validated,
            Err(error) => {
                self.fail_request(db, actor, &request_id, &error)?;
                return Err(error);
            }
        };

        let warnings = validated
            .warnings
            .into_iter()
            .map(|mut warning| {
                warning.message = redactor.restore(&warning.message);
                warning.quote = warning.quote.map(|quote| redactor.restore(&quote));
                warning
            })
            .collect::<Vec<_>>();

        let created_at = Timestamp::now();
        self.complete_request(
            db,
            actor,
            RequestOutcome {
                request_id: &request_id,
                function: AiFunction::ReportReview,
                output_chars: validated.summary.chars().count(),
                confidence: validated.confidence,
                detail: json!({ "warnings": warnings.len(), "report_id": report_id }),
            },
        )?;
        db.with_tx(|tx| {
            audit::success(
                tx,
                actor,
                AuditAction::AiReviewRun,
                "report",
                report_id,
                json!({ "warnings": warnings.len(), "provider": self.provider.kind().as_str() }),
            )
        })?;

        Ok(AiReviewResult {
            request_id,
            provider: self.provider.kind(),
            privacy_mode: self.provider.privacy_mode(),
            model: response.model,
            summary: redactor.restore(&validated.summary),
            confidence: validated.confidence,
            warnings,
            created_at,
        })
    }

    fn should_redact(&self) -> bool {
        self.settings.redact_before_send && self.provider.kind() != AiProviderKind::Disabled
    }

    fn begin_request(
        &self,
        db: &Database,
        actor: &Actor,
        request_id: &str,
        function: AiFunction,
        request: &AiTextRequest,
    ) -> Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(request.text.as_bytes());
        let prompt_hash = format!("{:x}", hasher.finalize());

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO ai_requests (id, user_id, provider, privacy_mode, model, function,
                                          target_type, target_id, prompt_hash, input_chars,
                                          status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'pending', ?11)",
                rusqlite::params![
                    request_id,
                    actor.user_id,
                    self.provider.kind(),
                    self.provider.privacy_mode(),
                    self.provider.model(),
                    function,
                    request.target_type,
                    request.target_id,
                    prompt_hash,
                    request.text.chars().count() as i64,
                    Timestamp::now(),
                ],
            )?;
            Ok(())
        })
    }

    fn complete_request(
        &self,
        db: &Database,
        actor: &Actor,
        outcome: RequestOutcome<'_>,
    ) -> Result<()> {
        let RequestOutcome {
            request_id,
            function,
            output_chars,
            confidence,
            detail,
        } = outcome;
        db.with_tx(|tx| {
            tx.execute(
                "UPDATE ai_requests SET status = 'succeeded', output_chars = ?1, confidence = ?2,
                        completed_at = ?3 WHERE id = ?4",
                rusqlite::params![
                    output_chars as i64,
                    confidence,
                    Timestamp::now(),
                    request_id
                ],
            )?;
            audit::success(
                tx,
                actor,
                AuditAction::AiSuggestionGenerated,
                "ai_request",
                request_id,
                {
                    let mut detail = detail;
                    detail["provider"] = json!(self.provider.kind().as_str());
                    detail["privacy_mode"] = json!(self.provider.privacy_mode().as_str());
                    detail["ai_function"] = json!(function.as_str());
                    detail
                },
            )
        })
    }

    fn fail_request(
        &self,
        db: &Database,
        actor: &Actor,
        request_id: &str,
        error: &Error,
    ) -> Result<()> {
        let status = if matches!(error, Error::AiPolicy(_)) {
            "blocked"
        } else {
            "failed"
        };
        db.with_tx(|tx| {
            tx.execute(
                "UPDATE ai_requests SET status = ?1, error = ?2, completed_at = ?3 WHERE id = ?4",
                rusqlite::params![status, error.to_string(), Timestamp::now(), request_id],
            )?;
            audit::record(
                tx,
                Some(actor),
                AuditAction::AiRequestBlocked,
                "ai_request",
                request_id,
                AuditOutcome::Failure,
                json!({ "reason": error.to_string(), "status": status }),
            )
        })
    }

    fn block_request(
        &self,
        db: &Database,
        actor: &Actor,
        request_id: &str,
        error: &Error,
        fabrications: &[String],
    ) -> Result<()> {
        db.with_tx(|tx| {
            tx.execute(
                "UPDATE ai_requests SET status = 'blocked', error = ?1, completed_at = ?2
                 WHERE id = ?3",
                rusqlite::params![error.to_string(), Timestamp::now(), request_id],
            )?;
            audit::record(
                tx,
                Some(actor),
                AuditAction::AiRequestBlocked,
                "ai_request",
                request_id,
                AuditOutcome::Denied,
                json!({ "reason": "fabricated_identifiers", "values": fabrications }),
            )
        })
    }
}

/// Record what the human decided about a suggestion. Called by the UI on Accept, Edit or
/// Reject, and is what makes the audit trail meaningful.
pub fn record_decision(
    db: &Database,
    actor: &Actor,
    request_id: &str,
    decision: AiDecision,
) -> Result<()> {
    db.with_tx(|tx| {
        let changed = tx.execute(
            "UPDATE ai_requests SET decision = ?1, decided_at = ?2 WHERE id = ?3",
            rusqlite::params![decision, Timestamp::now(), request_id],
        )?;
        if changed == 0 {
            return Err(Error::not_found("ai request"));
        }
        let action = match decision {
            AiDecision::Accepted => AuditAction::AiSuggestionAccepted,
            AiDecision::Edited => AuditAction::AiSuggestionEdited,
            AiDecision::Rejected => AuditAction::AiSuggestionRejected,
        };
        audit::success(tx, actor, action, "ai_request", request_id, json!({}))
    })
}

/// The provenance a suggestion carries if accepted unchanged.
///
/// Functions that only reword the author's facts produce `AiWording`, which is
/// authoritative. Functions that produce new prose - expanding notes, drafting a summary,
/// analysing a finding - produce `AiInference`, which blocks finalization until the
/// author confirms the text as their own.
pub fn proposed_provenance(function: AiFunction) -> Provenance {
    match function {
        AiFunction::ImproveWriting
        | AiFunction::Rewrite
        | AiFunction::TechnicalRewrite
        | AiFunction::ExecutiveRewrite
        | AiFunction::Summarize => Provenance::AiWording,
        AiFunction::Expand
        | AiFunction::ExecutiveSummary
        | AiFunction::FindingAnalysis
        | AiFunction::ReportReview => Provenance::AiInference,
    }
}

/// Flatten a report into the text a reviewer sees.
///
/// Only included sections, their answers, and a compact finding list. No ids, no
/// timestamps, no evidence bytes - the reviewer is judging prose, and everything else is
/// context the model does not need.
pub fn assemble_report_text(content: &ReportContent) -> String {
    let mut out = String::new();
    out.push_str(&format!("TITLE: {}\n", content.title));
    out.push_str(&format!("TYPE: {}\n\n", content.report_type.title()));

    for section in content.sections.iter().filter(|section| section.included) {
        if !section.has_content() {
            continue;
        }
        out.push_str(&format!("## {}\n", section.title));
        for answer in section.answers.iter().filter(|answer| !answer.is_blank()) {
            out.push_str(answer.value.trim());
            out.push_str("\n\n");
        }
    }

    if !content.findings.is_empty() {
        out.push_str("## Findings\n");
        for finding in &content.findings {
            out.push_str(&format!(
                "{} [{}] {}\nAffected: {}\nDescription: {}\nRemediation: {}\n\n",
                finding.finding_ref,
                finding.severity.title(),
                finding.title,
                finding.affected_asset,
                finding.description.trim(),
                finding.remediation.trim(),
            ));
        }
    }

    if out.chars().count() > MAX_REVIEW_CHARS {
        out = prompts::truncate(&out, MAX_REVIEW_CHARS);
    }
    out
}

/// Requests recorded for one target, newest first. Used by the report's AI activity panel.
pub fn requests_for_target(
    conn: &Connection,
    target_type: &str,
    target_id: &str,
    limit: i64,
) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT id, function, provider, privacy_mode, model, status, confidence, decision,
                input_chars, output_chars, created_at, decided_at
         FROM ai_requests WHERE target_type = ?1 AND target_id = ?2
         ORDER BY created_at DESC LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(
            rusqlite::params![target_type, target_id, limit.clamp(1, 500)],
            |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "function": row.get::<_, String>(1)?,
                    "provider": row.get::<_, String>(2)?,
                    "privacy_mode": row.get::<_, String>(3)?,
                    "model": row.get::<_, String>(4)?,
                    "status": row.get::<_, String>(5)?,
                    "confidence": row.get::<_, String>(6)?,
                    "decision": row.get::<_, Option<String>>(7)?,
                    "input_chars": row.get::<_, i64>(8)?,
                    "output_chars": row.get::<_, i64>(9)?,
                    "created_at": row.get::<_, String>(10)?,
                    "decided_at": row.get::<_, Option<String>>(11)?,
                }))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests;
