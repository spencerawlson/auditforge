//! Word-level diff for the "Changes" pane of the AI review interface.
//!
//! A longest-common-subsequence diff over whitespace-delimited tokens. Each token keeps
//! its trailing whitespace so the segments can be concatenated back into the original
//! text exactly.

use crate::models::{DiffKind, DiffSegment};

/// Beyond this many tokens on either side the quadratic table is not worth building; the
/// pane falls back to showing the two versions whole, which is what a reader wants for a
/// change that large anyway.
const MAX_TOKENS: usize = 3_000;

pub fn word_diff(before: &str, after: &str) -> Vec<DiffSegment> {
    let old_tokens = tokenize(before);
    let new_tokens = tokenize(after);

    if old_tokens.is_empty() && new_tokens.is_empty() {
        return Vec::new();
    }
    if old_tokens.len() > MAX_TOKENS || new_tokens.len() > MAX_TOKENS {
        let mut segments = Vec::new();
        push(&mut segments, DiffKind::Delete, before);
        push(&mut segments, DiffKind::Insert, after);
        return segments;
    }

    let table = lcs_table(&old_tokens, &new_tokens);
    let mut segments = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);

    while i < old_tokens.len() && j < new_tokens.len() {
        if old_tokens[i] == new_tokens[j] {
            push(&mut segments, DiffKind::Equal, old_tokens[i]);
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            push(&mut segments, DiffKind::Delete, old_tokens[i]);
            i += 1;
        } else {
            push(&mut segments, DiffKind::Insert, new_tokens[j]);
            j += 1;
        }
    }
    while i < old_tokens.len() {
        push(&mut segments, DiffKind::Delete, old_tokens[i]);
        i += 1;
    }
    while j < new_tokens.len() {
        push(&mut segments, DiffKind::Insert, new_tokens[j]);
        j += 1;
    }
    segments
}

/// True when the two texts differ in more than whitespace.
///
/// Tokens carry their trailing whitespace, so re-wrapping a paragraph still produces
/// delete/insert segments. Rebuilding both sides and comparing them whitespace-normalized
/// is what actually answers "did the wording change?".
pub fn has_changes(segments: &[DiffSegment]) -> bool {
    let before: String = segments
        .iter()
        .filter(|segment| segment.kind != DiffKind::Insert)
        .map(|segment| segment.text.as_str())
        .collect();
    let after: String = segments
        .iter()
        .filter(|segment| segment.kind != DiffKind::Delete)
        .map(|segment| segment.text.as_str())
        .collect();
    normalize_whitespace(&before) != normalize_whitespace(&after)
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split into tokens that each carry their trailing whitespace, so joining the segments
/// reproduces the input byte for byte.
fn tokenize(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            tokens.push(&text[start..index]);
            start = index;
        } else {
            index += 1;
        }
    }
    if start < text.len() {
        tokens.push(&text[start..]);
    }
    tokens
}

fn lcs_table(old_tokens: &[&str], new_tokens: &[&str]) -> Vec<Vec<u32>> {
    let mut table = vec![vec![0u32; new_tokens.len() + 1]; old_tokens.len() + 1];
    for i in (0..old_tokens.len()).rev() {
        for j in (0..new_tokens.len()).rev() {
            table[i][j] = if old_tokens[i] == new_tokens[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    table
}

/// Append to the previous segment when the kind matches, so the pane renders runs rather
/// than one span per word.
fn push(segments: &mut Vec<DiffSegment>, kind: DiffKind, text: &str) {
    match segments.last_mut() {
        Some(last) if last.kind == kind => last.text.push_str(text),
        _ => segments.push(DiffSegment {
            kind,
            text: text.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebuild(segments: &[DiffSegment], include: &[DiffKind]) -> String {
        segments
            .iter()
            .filter(|segment| include.contains(&segment.kind))
            .map(|segment| segment.text.as_str())
            .collect()
    }

    #[test]
    fn identical_text_is_all_equal() {
        let segments = word_diff("the same words", "the same words");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].kind, DiffKind::Equal);
        assert!(!has_changes(&segments));
    }

    #[test]
    fn the_diff_reconstructs_both_sides_exactly() {
        let before = "The server  was\nmisconfigured and exposed.";
        let after = "The server was badly misconfigured.";
        let segments = word_diff(before, after);

        assert_eq!(
            rebuild(&segments, &[DiffKind::Equal, DiffKind::Delete]),
            before
        );
        assert_eq!(
            rebuild(&segments, &[DiffKind::Equal, DiffKind::Insert]),
            after
        );
        assert!(has_changes(&segments));
    }

    #[test]
    fn an_inserted_word_is_marked_as_an_insert() {
        let segments = word_diff("the host was patched", "the host was fully patched");
        let inserted = rebuild(&segments, &[DiffKind::Insert]);
        assert!(inserted.contains("fully"), "got {inserted:?}");
        assert!(rebuild(&segments, &[DiffKind::Delete]).trim().is_empty());
    }

    #[test]
    fn a_deleted_word_is_marked_as_a_delete() {
        let segments = word_diff("the very old host", "the old host");
        assert!(rebuild(&segments, &[DiffKind::Delete]).contains("very"));
        assert!(rebuild(&segments, &[DiffKind::Insert]).trim().is_empty());
    }

    #[test]
    fn writing_from_nothing_is_one_insert() {
        let segments = word_diff("", "brand new text");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].kind, DiffKind::Insert);
        assert_eq!(segments[0].text, "brand new text");
    }

    #[test]
    fn clearing_text_is_one_delete() {
        let segments = word_diff("text to remove", "");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].kind, DiffKind::Delete);
    }

    #[test]
    fn empty_on_both_sides_produces_nothing() {
        assert!(word_diff("", "").is_empty());
    }

    #[test]
    fn runs_are_merged_rather_than_split_per_word() {
        let segments = word_diff("a b c d e", "a x y z e");
        let kinds: Vec<DiffKind> = segments.iter().map(|s| s.kind).collect();
        assert!(
            kinds.windows(2).all(|pair| pair[0] != pair[1]),
            "adjacent kinds must differ"
        );
    }

    #[test]
    fn very_large_inputs_fall_back_without_hanging() {
        let before = "word ".repeat(MAX_TOKENS + 10);
        let after = "other ".repeat(MAX_TOKENS + 10);
        let segments = word_diff(&before, &after);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].kind, DiffKind::Delete);
        assert_eq!(segments[1].kind, DiffKind::Insert);
    }

    #[test]
    fn multibyte_text_is_handled() {
        let segments = word_diff("die Prüfung ✅ lief", "die Prüfung lief");
        assert_eq!(
            rebuild(&segments, &[DiffKind::Equal, DiffKind::Insert]),
            "die Prüfung lief"
        );
        assert!(rebuild(&segments, &[DiffKind::Delete]).contains('✅'));
    }

    #[test]
    fn whitespace_only_changes_are_not_reported_as_changes() {
        let segments = word_diff("one two", "one  two");
        assert!(
            !has_changes(&segments),
            "reflowing whitespace is not an edit worth flagging"
        );
    }
}
