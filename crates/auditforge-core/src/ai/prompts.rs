//! Prompt construction.
//!
//! Every prompt carries the same non-negotiable rule, and every prompt sends the minimum
//! text needed for the task: the passage being worked on plus a short caller-supplied
//! context line. Whole engagements, finding tables and databases are never sent.

use crate::models::{AiFunction, SummaryKind};

/// Longest context line included alongside the passage.
pub const MAX_CONTEXT_CHARS: usize = 600;
/// Longest passage sent in one request.
pub const MAX_INPUT_CHARS: usize = 12_000;

/// The rule the model is held to, and which [`super::guard`] then verifies mechanically.
pub const SAFETY_RULE: &str = "\
You are an editing assistant inside AuditForge, a security and compliance reporting tool.
Your role is language and organisation. You are not an authority on the facts.

ABSOLUTE RULES
1. Never introduce a fact that is not present in the text you were given. This includes
   IP addresses, hostnames, URLs, vulnerabilities, CVE or CWE identifiers, CVSS scores or
   vectors, timestamps, dates, attack techniques, tool names, affected systems, evidence,
   remediation steps, root causes, severities, credentials, people and organisations.
2. Never change a fact that is present. Numbers, identifiers, names and quantities must
   survive your edit exactly as written.
3. Never assign or imply a severity, risk rating or CVSS score.
4. If something needed for the requested output is missing, do not guess. Name what is
   missing in `missing_information` and write \"Information required.\" in place of the
   detail. Where a cause is not established, write exactly this sentence and nothing more:
   \"Insufficient information to determine root cause.\"
5. Keep the author's meaning. Improve grammar, clarity, structure and tone only.
6. Placeholders of the form [REDACTED_...] are redacted values. Reproduce them exactly and
   never expand, guess at, or comment on what they stand for.

Reply with JSON only, matching the schema you are given. No prose outside the JSON.";

/// Instruction specific to each function.
pub fn instruction(function: AiFunction, summary_kind: Option<SummaryKind>) -> String {
    match function {
        AiFunction::ImproveWriting => "\
Improve the passage: fix grammar and spelling, tighten wording, and make the structure \
clear. Keep the author's voice. Do not add, remove or alter any technical fact."
            .to_string(),
        AiFunction::Rewrite => "\
Rewrite the passage so it reads well, preserving its factual meaning exactly. You may \
reorganise sentences; you may not introduce or drop information."
            .to_string(),
        AiFunction::TechnicalRewrite => "\
Rewrite these informal notes as professional technical prose suitable for a security \
report. Use precise, neutral language. Every technical detail in the output must come \
from the notes."
            .to_string(),
        AiFunction::ExecutiveRewrite => "\
Rewrite this for a non-technical executive reader. Lead with consequence and decision, \
not mechanism. Keep it short. Do not soften or exaggerate the facts, and do not add a \
risk rating."
            .to_string(),
        AiFunction::Summarize => {
            let audience = match summary_kind.unwrap_or(SummaryKind::Technical) {
                SummaryKind::Executive => {
                    "an executive summary for a decision maker: outcome, consequence, and what \
                     is being asked of them"
                }
                SummaryKind::Technical => {
                    "a technical summary for an engineer: what was found and what it affects"
                }
                SummaryKind::Incident => {
                    "an incident summary: what happened, what was affected, what was done, and \
                     what is still open"
                }
                SummaryKind::Finding => {
                    "a finding summary: the issue, the affected asset, and the consequence"
                }
            };
            format!(
                "Summarise the passage as {audience}. Use only facts present in the passage. \
                 Do not add a severity or a rating."
            )
        }
        AiFunction::Expand => "\
Expand these notes into a complete, professional report section. You may add connective \
prose, structure and explanation of terms already used. You may not add new facts. \
Wherever the section would need a detail the notes do not contain, write \
\"Information required.\" and list that detail in `missing_information`."
            .to_string(),
        AiFunction::ExecutiveSummary => "\
Draft an executive summary from the material provided. Use only what is present. If the \
material does not support a statement about overall risk, say so rather than inventing a \
position."
            .to_string(),
        AiFunction::FindingAnalysis => "\
Review this finding for internal consistency and completeness as written. Report what is \
unclear, contradictory or missing. Do not propose a severity, and do not supply technical \
detail the finding does not already contain."
            .to_string(),
        AiFunction::ReportReview => "\
Review this report for quality problems. Report only what you can point at in the text. \
Categories: grammar, clarity, contradiction, missing_information, unsupported_claim, \
inconsistent_terminology, inconsistent_severity, incomplete_remediation, missing_evidence, \
timeline_inconsistency, other. For each warning, quote the exact passage it refers to, \
copied verbatim from the report. Do not rewrite anything: return warnings only."
            .to_string(),
    }
}

/// Assemble the user message. Kept deliberately plain so what is sent is easy to audit.
pub fn user_message(instruction: &str, context: &str, text: &str) -> String {
    let mut message = String::with_capacity(instruction.len() + context.len() + text.len() + 128);
    message.push_str(instruction);
    let context = truncate(context.trim(), MAX_CONTEXT_CHARS);
    if !context.is_empty() {
        message.push_str("\n\nCONTEXT (for orientation only, do not copy into the output):\n");
        message.push_str(&context);
    }
    message.push_str("\n\nPASSAGE:\n<<<\n");
    message.push_str(&truncate(text, MAX_INPUT_CHARS));
    message.push_str("\n>>>");
    message
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push_str("\n[truncated]");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_safety_rule_names_every_forbidden_invention() {
        for forbidden in [
            "IP addresses",
            "hostnames",
            "CVE",
            "CVSS",
            "timestamps",
            "root causes",
            "severities",
            "credentials",
            "organisations",
        ] {
            assert!(
                SAFETY_RULE.contains(forbidden),
                "safety rule is missing: {forbidden}"
            );
        }
        assert!(SAFETY_RULE.contains("Information required."));
        assert!(SAFETY_RULE.contains("Insufficient information to determine root cause."));
        assert!(SAFETY_RULE.contains("[REDACTED_"));
    }

    #[test]
    fn every_function_has_an_instruction() {
        for function in AiFunction::ALL {
            let text = instruction(*function, None);
            assert!(!text.trim().is_empty(), "{function} has no instruction");
        }
    }

    #[test]
    fn summarize_varies_by_audience() {
        let executive = instruction(AiFunction::Summarize, Some(SummaryKind::Executive));
        let technical = instruction(AiFunction::Summarize, Some(SummaryKind::Technical));
        assert_ne!(executive, technical);
        assert!(executive.contains("executive"));
    }

    #[test]
    fn the_review_instruction_forbids_rewriting() {
        let text = instruction(AiFunction::ReportReview, None);
        assert!(text.contains("warnings only"));
        assert!(text.contains("verbatim"));
    }

    #[test]
    fn the_user_message_delimits_the_passage_and_caps_context() {
        let message = user_message("Do the thing", &"c".repeat(5_000), "the passage");
        assert!(message.contains("PASSAGE:\n<<<\nthe passage\n>>>"));
        assert!(message.contains("[truncated]"));
        assert!(message.len() < 5_000 + 1_000);
    }

    #[test]
    fn context_is_omitted_when_empty() {
        let message = user_message("Do the thing", "   ", "text");
        assert!(!message.contains("CONTEXT"));
    }

    #[test]
    fn truncation_is_char_safe_for_multibyte_text() {
        let text = "üüüüüüüüüü";
        let out = truncate(text, 3);
        assert!(out.starts_with("üüü"));
        assert!(out.contains("[truncated]"));
    }
}
