//! Response schemas and strict validation of model output.
//!
//! Model output is untrusted input. It is parsed against a fixed shape, every string is
//! length-capped and stripped of control characters, enums that fail to parse degrade to a
//! safe default rather than propagating, and review quotes that do not actually appear in
//! the report are dropped - a citation to text that does not exist is worse than no
//! citation.

use serde::Deserialize;
use serde_json::json;

use crate::error::{Error, Result};
use crate::models::{AiConfidence, ReviewCategory, ReviewWarning};

/// Longest suggestion accepted, as a multiple of the input length, with a floor so short
/// inputs can still be expanded.
const EXPANSION_FLOOR: usize = 4_000;
const EXPANSION_FACTOR: usize = 8;

const MAX_NOTE_CHARS: usize = 400;
const MAX_NOTES: usize = 12;
const MAX_WARNINGS: usize = 100;
const MAX_MESSAGE_CHARS: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseSchema {
    TextSuggestion,
    Review,
}

impl ResponseSchema {
    pub fn name(&self) -> &'static str {
        match self {
            ResponseSchema::TextSuggestion => "auditforge_text_suggestion",
            ResponseSchema::Review => "auditforge_report_review",
        }
    }

    /// JSON Schema handed to providers that support structured output.
    pub fn json_schema(&self) -> serde_json::Value {
        match self {
            ResponseSchema::TextSuggestion => json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["suggestion", "confidence", "missing_information", "notes"],
                "properties": {
                    "suggestion": {
                        "type": "string",
                        "description": "The rewritten passage. Facts must match the input."
                    },
                    "confidence": {
                        "type": "string",
                        "enum": ["high", "medium", "low"]
                    },
                    "missing_information": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Facts the passage would need but does not contain."
                    },
                    "notes": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Short remarks for the author. Never new facts."
                    }
                }
            }),
            ResponseSchema::Review => json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["summary", "confidence", "warnings"],
                "properties": {
                    "summary": { "type": "string" },
                    "confidence": { "type": "string", "enum": ["high", "medium", "low"] },
                    "warnings": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["category", "message", "quote"],
                            "properties": {
                                "category": {
                                    "type": "string",
                                    "enum": [
                                        "grammar", "clarity", "contradiction",
                                        "missing_information", "unsupported_claim",
                                        "inconsistent_terminology", "inconsistent_severity",
                                        "incomplete_remediation", "missing_evidence",
                                        "timeline_inconsistency", "other"
                                    ]
                                },
                                "message": { "type": "string" },
                                "quote": {
                                    "type": "string",
                                    "description":
                                        "Exact text copied from the report, or an empty string."
                                }
                            }
                        }
                    }
                }
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedSuggestion {
    pub suggestion: String,
    pub confidence: AiConfidence,
    pub missing_information: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedReview {
    pub summary: String,
    pub confidence: AiConfidence,
    pub warnings: Vec<ReviewWarning>,
}

#[derive(Deserialize)]
struct RawSuggestion {
    suggestion: String,
    #[serde(default)]
    confidence: Option<String>,
    #[serde(default)]
    missing_information: Vec<String>,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Deserialize)]
struct RawReview {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    confidence: Option<String>,
    #[serde(default)]
    warnings: Vec<RawWarning>,
}

#[derive(Deserialize)]
struct RawWarning {
    #[serde(default)]
    category: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    quote: Option<String>,
    #[serde(default)]
    section_key: Option<String>,
}

pub fn parse_text_suggestion(raw: &str, input_chars: usize) -> Result<ValidatedSuggestion> {
    let body = extract_json(raw)?;
    let parsed: RawSuggestion = serde_json::from_str(&body)
        .map_err(|e| Error::AiProvider(format!("the model returned unusable JSON: {e}")))?;

    let suggestion = sanitize(&parsed.suggestion);
    if suggestion.trim().is_empty() {
        return Err(Error::AiProvider(
            "the model returned an empty suggestion".to_string(),
        ));
    }
    let ceiling = (input_chars.saturating_mul(EXPANSION_FACTOR)).max(EXPANSION_FLOOR);
    if suggestion.chars().count() > ceiling {
        return Err(Error::AiProvider(
            "the model returned far more text than the passage could support; discarded"
                .to_string(),
        ));
    }

    Ok(ValidatedSuggestion {
        suggestion,
        confidence: parse_confidence(parsed.confidence.as_deref()),
        missing_information: clean_list(parsed.missing_information),
        notes: clean_list(parsed.notes),
    })
}

/// Parse a review. `source` is the text the reviewer was given; quotes are checked
/// against it.
pub fn parse_review(raw: &str, source: &str) -> Result<ValidatedReview> {
    let body = extract_json(raw)?;
    let parsed: RawReview = serde_json::from_str(&body)
        .map_err(|e| Error::AiProvider(format!("the model returned unusable JSON: {e}")))?;

    let normalized_source = normalize(source);
    let warnings = parsed
        .warnings
        .into_iter()
        .take(MAX_WARNINGS)
        .filter_map(|warning| {
            let message = sanitize(&warning.message);
            let message = truncate(&message, MAX_MESSAGE_CHARS);
            if message.trim().is_empty() {
                return None;
            }
            let category = warning
                .category
                .parse::<ReviewCategory>()
                .unwrap_or(ReviewCategory::Other);
            let quote = warning
                .quote
                .map(|quote| sanitize(&quote))
                .filter(|quote| !quote.trim().is_empty())
                .filter(|quote| normalized_source.contains(&normalize(quote)))
                .map(|quote| truncate(&quote, 300));
            Some(ReviewWarning {
                category,
                message,
                section_key: warning
                    .section_key
                    .map(|key| truncate(&sanitize(&key), 100)),
                quote,
            })
        })
        .collect();

    Ok(ValidatedReview {
        summary: truncate(&sanitize(&parsed.summary), MAX_MESSAGE_CHARS * 4),
        confidence: parse_confidence(parsed.confidence.as_deref()),
        warnings,
    })
}

/// Pull the JSON object out of a reply, tolerating a markdown fence or leading prose.
///
/// Providers are asked for JSON only, and the strict ones deliver it; local models often
/// wrap it. Recovering here is preferable to failing a call for cosmetic reasons.
fn extract_json(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::AiProvider("the model returned nothing".to_string()));
    }
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Ok(trimmed.to_string());
    }
    let start = trimmed
        .find('{')
        .ok_or_else(|| Error::AiProvider("the model did not return JSON".to_string()))?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| Error::AiProvider("the model did not return JSON".to_string()))?;
    if end <= start {
        return Err(Error::AiProvider(
            "the model did not return JSON".to_string(),
        ));
    }
    Ok(trimmed[start..=end].to_string())
}

fn parse_confidence(raw: Option<&str>) -> AiConfidence {
    // An unreadable or absent confidence is treated as Medium: the value is advisory, and
    // failing an otherwise good suggestion over it would be pedantic.
    raw.and_then(|value| {
        value
            .trim()
            .to_ascii_lowercase()
            .parse::<AiConfidence>()
            .ok()
    })
    .unwrap_or(AiConfidence::Medium)
}

fn clean_list(items: Vec<String>) -> Vec<String> {
    items
        .into_iter()
        .map(|item| truncate(&sanitize(&item), MAX_NOTE_CHARS))
        .filter(|item| !item.trim().is_empty())
        .take(MAX_NOTES)
        .collect()
}

/// Strip control characters that could corrupt a rendered document or a terminal.
fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect::<String>()
        .replace("\r\n", "\n")
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

/// Case- and whitespace-insensitive form, so a quote that differs only in wrapping still
/// matches the report text it came from.
fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_formed_suggestion_parses() {
        let raw = r#"{"suggestion":"The host was unpatched.","confidence":"high",
                      "missing_information":["patch level"],"notes":["shortened"]}"#;
        let parsed = parse_text_suggestion(raw, 100).unwrap();
        assert_eq!(parsed.suggestion, "The host was unpatched.");
        assert_eq!(parsed.confidence, AiConfidence::High);
        assert_eq!(parsed.missing_information, vec!["patch level"]);
    }

    #[test]
    fn json_wrapped_in_a_markdown_fence_is_recovered() {
        let raw = "Sure!\n```json\n{\"suggestion\":\"Text.\",\"confidence\":\"low\"}\n```\n";
        let parsed = parse_text_suggestion(raw, 100).unwrap();
        assert_eq!(parsed.suggestion, "Text.");
        assert_eq!(parsed.confidence, AiConfidence::Low);
    }

    #[test]
    fn junk_and_empty_replies_are_refused() {
        assert!(parse_text_suggestion("", 10).is_err());
        assert!(parse_text_suggestion("I cannot help with that.", 10).is_err());
        assert!(parse_text_suggestion(r#"{"suggestion":""}"#, 10).is_err());
        assert!(parse_text_suggestion(r#"{"confidence":"high"}"#, 10).is_err());
        assert!(parse_text_suggestion("{not json at all}", 10).is_err());
    }

    #[test]
    fn a_runaway_response_is_refused() {
        let huge = "x".repeat(60_000);
        let raw = json!({ "suggestion": huge, "confidence": "high" }).to_string();
        let err = parse_text_suggestion(&raw, 50).unwrap_err();
        assert_eq!(err.kind(), "ai_provider");
    }

    #[test]
    fn short_inputs_may_still_be_expanded_up_to_the_floor() {
        let text = "y".repeat(3_000);
        let raw = json!({ "suggestion": text, "confidence": "high" }).to_string();
        assert!(parse_text_suggestion(&raw, 20).is_ok());
    }

    #[test]
    fn control_characters_are_stripped_but_newlines_survive() {
        let raw = json!({
            "suggestion": "line one\nline two\u{0007}\u{001b}[31mred",
            "confidence": "medium"
        })
        .to_string();
        let parsed = parse_text_suggestion(&raw, 100).unwrap();
        assert!(parsed.suggestion.contains("line one\nline two"));
        assert!(!parsed.suggestion.contains('\u{0007}'));
        assert!(!parsed.suggestion.contains('\u{001b}'));
    }

    #[test]
    fn an_unreadable_confidence_degrades_to_medium() {
        let raw = json!({ "suggestion": "Text.", "confidence": "extremely certain" }).to_string();
        assert_eq!(
            parse_text_suggestion(&raw, 10).unwrap().confidence,
            AiConfidence::Medium
        );
        let raw = json!({ "suggestion": "Text." }).to_string();
        assert_eq!(
            parse_text_suggestion(&raw, 10).unwrap().confidence,
            AiConfidence::Medium
        );
    }

    #[test]
    fn note_lists_are_capped_and_cleaned() {
        let many: Vec<String> = (0..50).map(|i| format!("note {i}")).collect();
        let raw = json!({
            "suggestion": "Text.",
            "confidence": "high",
            "notes": many,
            "missing_information": ["", "   ", "real gap"]
        })
        .to_string();
        let parsed = parse_text_suggestion(&raw, 100).unwrap();
        assert_eq!(parsed.notes.len(), MAX_NOTES);
        assert_eq!(parsed.missing_information, vec!["real gap"]);
    }

    #[test]
    fn review_warnings_keep_quotes_that_exist_in_the_report() {
        let source = "The firewall was misconfigured.\nRemediation is pending.";
        let raw = json!({
            "summary": "Two issues.",
            "confidence": "medium",
            "warnings": [
                {"category": "clarity", "message": "Vague.",
                 "quote": "The firewall was misconfigured."},
                {"category": "unsupported_claim", "message": "No support.",
                 "quote": "The attacker exfiltrated 4GB of data."}
            ]
        })
        .to_string();

        let review = parse_review(&raw, source).unwrap();
        assert_eq!(review.warnings.len(), 2);
        assert!(review.warnings[0].quote.is_some());
        assert!(
            review.warnings[1].quote.is_none(),
            "a quote that is not in the report must be dropped"
        );
    }

    #[test]
    fn quotes_match_across_differing_whitespace_and_case() {
        let source = "The  firewall\nwas misconfigured.";
        let raw = json!({
            "summary": "",
            "warnings": [{"category": "clarity", "message": "m",
                          "quote": "the firewall was misconfigured."}]
        })
        .to_string();
        assert!(parse_review(&raw, source).unwrap().warnings[0]
            .quote
            .is_some());
    }

    #[test]
    fn unknown_warning_categories_become_other_and_empty_messages_are_dropped() {
        let raw = json!({
            "summary": "s",
            "warnings": [
                {"category": "vibes", "message": "Something."},
                {"category": "grammar", "message": "   "}
            ]
        })
        .to_string();
        let review = parse_review(&raw, "anything").unwrap();
        assert_eq!(review.warnings.len(), 1);
        assert_eq!(review.warnings[0].category, ReviewCategory::Other);
    }

    #[test]
    fn a_flood_of_warnings_is_capped() {
        let warnings: Vec<serde_json::Value> = (0..500)
            .map(|i| json!({"category": "clarity", "message": format!("issue {i}")}))
            .collect();
        let raw = json!({ "summary": "s", "warnings": warnings }).to_string();
        assert_eq!(
            parse_review(&raw, "text").unwrap().warnings.len(),
            MAX_WARNINGS
        );
    }

    #[test]
    fn the_published_schemas_are_valid_json_objects() {
        for schema in [ResponseSchema::TextSuggestion, ResponseSchema::Review] {
            let value = schema.json_schema();
            assert_eq!(value["type"], "object");
            assert!(value["required"].as_array().is_some());
            assert!(!schema.name().is_empty());
        }
    }
}
