//! Tabular answers.
//!
//! Some parts of a report are a list of rows, not prose: the assets in scope, the ports
//! found open, the tools used. Writing those as a paragraph loses the structure, and a
//! reader who wants to know whether 10.0.0.5 was in scope has to read a sentence to find
//! out.
//!
//! A table question stores its rows in the same `AnswerValue.value` string as every other
//! answer, encoded as a JSON array of objects keyed by column key. That choice matters:
//! it means tables inherit versioning, the content hash, immutability, diffing, export
//! and search without any of those subsystems learning a new shape.
//!
//! Two invariants keep the rest of the system honest:
//!
//! 1. **An empty table serialises to the empty string**, not to `[]`. `AnswerValue::is_blank`
//!    is a plain `value.trim().is_empty()` check used by the readiness gate, the section
//!    `has_content` test and every renderer. If an empty table stored `[]` it would read
//!    as answered everywhere, and a required-but-empty table would pass finalization.
//!
//! 2. **Rows are keyed by column key, not by position.** Reordering or inserting a column
//!    in a template must not silently shift every stored value one column to the left.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One column of a table question, declared by the template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateColumn {
    pub key: String,
    pub label: String,
    /// Width hint as a PERCENT of the table, used by the PDF layout. Columns without
    /// one share whatever is left over.
    ///
    /// An integer rather than a float so the template structs can keep deriving `Eq` —
    /// and because float equality is the wrong tool for comparing two templates anyway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u16>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub help: String,
}

/// The decoded rows of a table answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableValue {
    pub rows: Vec<BTreeMap<String, String>>,
}

impl TableValue {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Cell lookup that treats a missing column and an empty one the same way, so a row
    /// stored before a column existed renders as blank rather than panicking.
    pub fn cell(&self, row: usize, key: &str) -> &str {
        self.rows
            .get(row)
            .and_then(|r| r.get(key))
            .map(String::as_str)
            .unwrap_or("")
    }

    /// Rows as ordered cell vectors, one entry per declared column.
    pub fn as_grid(&self, columns: &[TemplateColumn]) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                columns
                    .iter()
                    .map(|c| row.get(&c.key).cloned().unwrap_or_default())
                    .collect()
            })
            .collect()
    }
}

/// Decode a stored answer into rows.
///
/// Deliberately tolerant: anything that is not a JSON array of objects yields an empty
/// table rather than an error. A malformed answer must not be able to fail an export —
/// the author sees an empty table and can fix it, which is recoverable, where a failed
/// PDF render of a finalized report is not.
pub fn parse(value: &str) -> TableValue {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return TableValue::default();
    }
    let parsed: Result<Vec<BTreeMap<String, serde_json::Value>>, _> = serde_json::from_str(trimmed);
    let Ok(raw) = parsed else {
        return TableValue::default();
    };
    let rows = raw
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|(k, v)| {
                    let text = match v {
                        serde_json::Value::String(s) => s,
                        serde_json::Value::Null => String::new(),
                        other => other.to_string(),
                    };
                    (k, text)
                })
                .collect::<BTreeMap<String, String>>()
        })
        .collect();
    TableValue { rows }
}

/// Encode rows back to the stored form, dropping rows that carry nothing.
///
/// Returns the empty string when no row survives — see invariant 1 in the module docs.
pub fn serialize(table: &TableValue) -> String {
    let kept: Vec<&BTreeMap<String, String>> = table
        .rows
        .iter()
        .filter(|row| row.values().any(|v| !v.trim().is_empty()))
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    let trimmed: Vec<BTreeMap<&str, &str>> = kept
        .iter()
        .map(|row| {
            row.iter()
                .filter(|(_, v)| !v.trim().is_empty())
                .map(|(k, v)| (k.as_str(), v.trim()))
                .collect()
        })
        .collect();
    serde_json::to_string(&trimmed).unwrap_or_default()
}

/// Normalize an answer that belongs to a table question.
///
/// Applied on save so the stored value is canonical regardless of what the client sent:
/// blank rows removed, cell values trimmed, and a table with no content collapsed to the
/// empty string so the readiness gate sees it as unanswered.
pub fn normalize(value: &str) -> String {
    serialize(&parse(value))
}

/// Column widths as fractions summing to 1, for the PDF table block.
pub fn widths(columns: &[TemplateColumn]) -> Vec<f32> {
    if columns.is_empty() {
        return Vec::new();
    }
    let declared: f32 = columns
        .iter()
        .filter_map(|c| c.width)
        .map(|w| w as f32 / 100.0)
        .sum();
    let undeclared = columns.iter().filter(|c| c.width.is_none()).count();
    // Any column without an explicit width shares out whatever is left.
    let remainder = (1.0 - declared).max(0.0);
    let each = if undeclared > 0 {
        remainder / undeclared as f32
    } else {
        0.0
    };
    let mut out: Vec<f32> = columns
        .iter()
        .map(|c| c.width.map(|w| w as f32 / 100.0).unwrap_or(each))
        .collect();
    // Guard against a template whose declared widths do not sum to 1.
    let total: f32 = out.iter().sum();
    if total > 0.0 && (total - 1.0).abs() > 0.001 {
        for w in &mut out {
            *w /= total;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn empty_table_serialises_to_empty_string_so_it_reads_as_unanswered() {
        // The whole readiness gate depends on this: `[]` would look answered.
        assert_eq!(serialize(&TableValue::default()), "");
        assert_eq!(normalize("[]"), "");
        assert_eq!(normalize(r#"[{"host":"","port":"  "}]"#), "");
    }

    #[test]
    fn blank_rows_are_dropped_and_cells_trimmed() {
        let value = r#"[{"host":" 10.0.0.5 ","port":"22"},{"host":"","port":""},{"host":"10.0.0.6","port":"80"}]"#;
        let table = parse(&normalize(value));
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.cell(0, "host"), "10.0.0.5");
        assert_eq!(table.cell(1, "host"), "10.0.0.6");
    }

    #[test]
    fn rows_are_keyed_by_column_so_reordering_columns_does_not_shift_values() {
        let stored = serialize(&TableValue {
            rows: vec![row(&[("host", "10.0.0.5"), ("port", "22"), ("service", "ssh")])],
        });
        let before = vec![
            TemplateColumn { key: "host".into(), label: "Host".into(), width: None, help: String::new() },
            TemplateColumn { key: "port".into(), label: "Port".into(), width: None, help: String::new() },
            TemplateColumn { key: "service".into(), label: "Service".into(), width: None, help: String::new() },
        ];
        let reordered = vec![before[2].clone(), before[0].clone(), before[1].clone()];
        let table = parse(&stored);
        assert_eq!(table.as_grid(&before), vec![vec!["10.0.0.5", "22", "ssh"]]);
        assert_eq!(table.as_grid(&reordered), vec![vec!["ssh", "10.0.0.5", "22"]]);
    }

    #[test]
    fn a_column_added_later_renders_blank_rather_than_failing() {
        let table = parse(r#"[{"host":"10.0.0.5"}]"#);
        let columns = vec![
            TemplateColumn { key: "host".into(), label: "Host".into(), width: None, help: String::new() },
            TemplateColumn { key: "notes".into(), label: "Notes".into(), width: None, help: String::new() },
        ];
        assert_eq!(table.as_grid(&columns), vec![vec!["10.0.0.5", ""]]);
    }

    #[test]
    fn malformed_json_yields_an_empty_table_not_an_error() {
        // An export of a finalized report must not be able to fail on bad stored data.
        assert!(parse("not json at all").is_empty());
        assert!(parse(r#"{"not":"an array"}"#).is_empty());
        assert!(parse("[1,2,3]").is_empty());
    }

    #[test]
    fn non_string_cells_are_coerced_rather_than_dropped() {
        let table = parse(r#"[{"port":443,"open":true}]"#);
        assert_eq!(table.cell(0, "port"), "443");
        assert_eq!(table.cell(0, "open"), "true");
    }

    #[test]
    fn widths_fill_and_normalise_to_one() {
        let cols = vec![
            TemplateColumn { key: "a".into(), label: "A".into(), width: Some(50), help: String::new() },
            TemplateColumn { key: "b".into(), label: "B".into(), width: None, help: String::new() },
            TemplateColumn { key: "c".into(), label: "C".into(), width: None, help: String::new() },
        ];
        let w = widths(&cols);
        assert_eq!(w.len(), 3);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 0.001);
        assert!((w[0] - 0.5).abs() < 0.001);

        // A template whose widths oversum is rescaled instead of producing a broken table.
        let bad = vec![
            TemplateColumn { key: "a".into(), label: "A".into(), width: Some(80), help: String::new() },
            TemplateColumn { key: "b".into(), label: "B".into(), width: Some(80), help: String::new() },
        ];
        let w = widths(&bad);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 0.001);
    }
}
