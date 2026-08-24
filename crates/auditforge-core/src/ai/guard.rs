//! The fabrication guard.
//!
//! The prompt tells the model not to invent facts. This module checks whether it did,
//! because a rule that is only asked for is not a rule.
//!
//! It extracts *hard identifiers* from the model's output - values whose form is
//! unambiguous and whose invention would be a serious defect in a security report - and
//! verifies that each one already appears in the text the model was given. Anything new is
//! a fabrication and the suggestion is refused.
//!
//! Deliberately limited scope: IPv4 and IPv6 addresses, CVE and CWE identifiers, CVSS
//! vectors, MAC addresses and ISO dates. Hostnames, product names and prose claims are
//! **not** covered, because no mechanical test separates an invented hostname from an
//! ordinary word containing a dot, and a guard with false positives would train users to
//! ignore it. Those remain the human reviewer's job, which is why the review pane always
//! shows the original beside the suggestion.

use std::collections::BTreeSet;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Fabrication {
    pub kind: &'static str,
    pub value: String,
}

/// Identifiers present in `output` but absent from `source`.
pub fn fabricated_identifiers(source: &str, output: &str) -> Vec<Fabrication> {
    let source_tokens = extract(source);
    let output_tokens = extract(output);

    // Compare case-insensitively: `cve-2026-1234` and `CVE-2026-1234` are the same fact.
    let known: BTreeSet<String> = source_tokens
        .iter()
        .map(|token| token.value.to_ascii_lowercase())
        .collect();

    let mut invented: Vec<Fabrication> = output_tokens
        .into_iter()
        .filter(|token| !known.contains(&token.value.to_ascii_lowercase()))
        .collect();
    invented.sort();
    invented.dedup();
    invented
}

fn extract(text: &str) -> Vec<Fabrication> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0usize;

    while index < bytes.len() {
        if !text.is_char_boundary(index) {
            index += 1;
            continue;
        }
        let matched = match_cve(text, index)
            .or_else(|| match_cwe(text, index))
            .or_else(|| match_cvss(text, index))
            .or_else(|| match_mac(text, index))
            .or_else(|| match_ipv4(text, index))
            .or_else(|| match_ipv6(text, index))
            .or_else(|| match_iso_date(text, index));

        match matched {
            Some((kind, len)) if len > 0 => {
                found.push(Fabrication {
                    kind,
                    value: text[index..index + len].to_string(),
                });
                index += len;
            }
            _ => index += text[index..].chars().next().map_or(1, |c| c.len_utf8()),
        }
    }
    found
}

fn at_word_start(text: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    let previous = text.as_bytes()[index - 1];
    !previous.is_ascii_alphanumeric() && previous != b'-' && previous != b'_'
}

fn match_prefixed_id(
    text: &str,
    index: usize,
    prefix: &str,
    kind: &'static str,
) -> Option<(&'static str, usize)> {
    if !at_word_start(text, index) {
        return None;
    }
    let rest = &text[index..];
    // Compare bytes rather than slicing: `prefix.len()` is not guaranteed to fall on a
    // character boundary when the text contains multi-byte characters.
    if rest.len() < prefix.len()
        || !rest.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
    {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut cursor = prefix.len();
    let digits_start = cursor;
    while cursor < bytes.len() && (bytes[cursor].is_ascii_digit() || bytes[cursor] == b'-') {
        cursor += 1;
    }
    // Trim a trailing hyphen so "CVE-2026-1234-" does not absorb the punctuation.
    while cursor > digits_start && bytes[cursor - 1] == b'-' {
        cursor -= 1;
    }
    let body = &rest[digits_start..cursor];
    if body.chars().filter(|c| c.is_ascii_digit()).count() < 4 {
        return None;
    }
    Some((kind, cursor))
}

fn match_cve(text: &str, index: usize) -> Option<(&'static str, usize)> {
    match_prefixed_id(text, index, "CVE-", "cve")
}

fn match_cwe(text: &str, index: usize) -> Option<(&'static str, usize)> {
    match_prefixed_id(text, index, "CWE-", "cwe")
}

fn match_cvss(text: &str, index: usize) -> Option<(&'static str, usize)> {
    if !at_word_start(text, index) {
        return None;
    }
    let rest = &text[index..];
    if rest.len() < 5 || !rest.as_bytes()[..5].eq_ignore_ascii_case(b"CVSS:") {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut cursor = 5;
    while cursor < bytes.len()
        && (bytes[cursor].is_ascii_alphanumeric() || matches!(bytes[cursor], b'.' | b'/' | b':'))
    {
        cursor += 1;
    }
    while cursor > 5 && matches!(bytes[cursor - 1], b'.' | b'/' | b':') {
        cursor -= 1;
    }
    (cursor > 8).then_some(("cvss_vector", cursor))
}

fn match_mac(text: &str, index: usize) -> Option<(&'static str, usize)> {
    if !at_word_start(text, index) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut cursor = index;
    for group in 0..6 {
        if group > 0 {
            if cursor >= bytes.len() || !matches!(bytes[cursor], b':' | b'-') {
                return None;
            }
            cursor += 1;
        }
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_hexdigit() && cursor - start < 2 {
            cursor += 1;
        }
        if cursor - start != 2 {
            return None;
        }
    }
    if cursor < bytes.len() && (bytes[cursor].is_ascii_hexdigit() || bytes[cursor] == b':') {
        return None;
    }
    Some(("mac_address", cursor - index))
}

fn match_ipv4(text: &str, index: usize) -> Option<(&'static str, usize)> {
    let bytes = text.as_bytes();
    if index > 0 && (bytes[index - 1].is_ascii_digit() || bytes[index - 1] == b'.') {
        return None;
    }
    let mut cursor = index;
    for octet in 0..4 {
        if octet > 0 {
            if cursor >= bytes.len() || bytes[cursor] != b'.' {
                return None;
            }
            cursor += 1;
        }
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() && cursor - start < 3 {
            cursor += 1;
        }
        if cursor == start || text[start..cursor].parse::<u32>().ok()? > 255 {
            return None;
        }
    }
    if cursor < bytes.len() && (bytes[cursor].is_ascii_digit() || bytes[cursor] == b'.') {
        return None;
    }
    // Include a CIDR suffix so 10.0.0.0/8 and 10.0.0.0/16 are different facts.
    if cursor + 1 < bytes.len() && bytes[cursor] == b'/' && bytes[cursor + 1].is_ascii_digit() {
        let mut suffix = cursor + 1;
        while suffix < bytes.len() && bytes[suffix].is_ascii_digit() && suffix - cursor <= 2 {
            suffix += 1;
        }
        if text[cursor + 1..suffix].parse::<u32>().ok()? <= 32 {
            cursor = suffix;
        }
    }
    Some(("ipv4", cursor - index))
}

fn match_ipv6(text: &str, index: usize) -> Option<(&'static str, usize)> {
    let bytes = text.as_bytes();
    let is_char = |b: u8| b.is_ascii_hexdigit() || b == b':';
    if index > 0 && is_char(bytes[index - 1]) {
        return None;
    }
    let mut cursor = index;
    while cursor < bytes.len() && is_char(bytes[cursor]) {
        cursor += 1;
    }
    while cursor > index && bytes[cursor - 1] == b':' && !text[index..cursor].ends_with("::") {
        cursor -= 1;
    }
    let candidate = &text[index..cursor];
    if candidate.matches(':').count() < 2 {
        return None;
    }
    candidate.parse::<std::net::Ipv6Addr>().ok()?;
    Some(("ipv6", cursor - index))
}

fn match_iso_date(text: &str, index: usize) -> Option<(&'static str, usize)> {
    let bytes = text.as_bytes();
    if index + 10 > bytes.len() || !at_word_start(text, index) || !text.is_char_boundary(index + 10)
    {
        return None;
    }
    let candidate = &text[index..index + 10];
    if candidate.as_bytes()[4] != b'-' || candidate.as_bytes()[7] != b'-' {
        return None;
    }
    chrono::NaiveDate::parse_from_str(candidate, "%Y-%m-%d").ok()?;
    if index + 10 < bytes.len() && bytes[index + 10].is_ascii_digit() {
        return None;
    }
    Some(("date", 10))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(fabrications: &[Fabrication]) -> Vec<&str> {
        fabrications.iter().map(|f| f.value.as_str()).collect()
    }

    #[test]
    fn rewording_without_new_identifiers_passes() {
        let source = "the box at 10.0.0.5 was running an old sshd";
        let output = "The host at 10.0.0.5 was running an outdated SSH daemon.";
        assert!(fabricated_identifiers(source, output).is_empty());
    }

    #[test]
    fn an_invented_ip_is_caught() {
        let source = "The host at 10.0.0.5 was vulnerable.";
        let output = "The hosts at 10.0.0.5 and 10.0.0.6 were vulnerable.";
        assert_eq!(
            values(&fabricated_identifiers(source, output)),
            vec!["10.0.0.6"]
        );
    }

    #[test]
    fn an_invented_cve_is_caught_case_insensitively() {
        let source = "An outdated OpenSSH version is installed.";
        let output = "The installed OpenSSH is affected by CVE-2024-6387.";
        let found = fabricated_identifiers(source, output);
        assert_eq!(values(&found), vec!["CVE-2024-6387"]);
        assert_eq!(found[0].kind, "cve");

        // Present in the source in a different case is still the same fact.
        assert!(fabricated_identifiers("see cve-2024-6387", "See CVE-2024-6387.").is_empty());
    }

    #[test]
    fn an_invented_cvss_vector_or_score_vector_is_caught() {
        let source = "The service accepts unauthenticated requests.";
        let output = "Rated CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H by the assistant.";
        let found = fabricated_identifiers(source, output);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "cvss_vector");
    }

    #[test]
    fn invented_dates_macs_and_ipv6_are_caught() {
        assert_eq!(
            values(&fabricated_identifiers(
                "It happened last week.",
                "It happened on 2026-08-24."
            )),
            vec!["2026-08-24"]
        );
        assert_eq!(
            values(&fabricated_identifiers(
                "A device joined.",
                "Device 00:1a:2b:3c:4d:5e joined."
            )),
            vec!["00:1a:2b:3c:4d:5e"]
        );
        assert_eq!(
            values(&fabricated_identifiers(
                "The host replied.",
                "fe80::1ff:fe23:4567:890a replied."
            )),
            vec!["fe80::1ff:fe23:4567:890a"]
        );
    }

    #[test]
    fn cidr_suffixes_are_part_of_the_fact() {
        let found = fabricated_identifiers("Scope was 10.0.0.0/24.", "Scope was 10.0.0.0/16.");
        assert_eq!(values(&found), vec!["10.0.0.0/16"]);
        assert!(
            fabricated_identifiers("Scope was 10.0.0.0/24.", "The 10.0.0.0/24 range.").is_empty()
        );
    }

    #[test]
    fn ordinary_prose_and_version_numbers_are_not_flagged() {
        let source = "Notes.";
        for output in [
            "The certificate expired and the service refused connections.",
            "OpenSSL 3.0.11 and Python 3.12 were installed.",
            "Roughly 99.5 percent of requests succeeded.",
            "Contact the team at 09:15 tomorrow.",
        ] {
            assert!(
                fabricated_identifiers(source, output).is_empty(),
                "false positive on: {output}"
            );
        }
    }

    #[test]
    fn dropping_facts_is_not_treated_as_fabrication() {
        // Removing detail is a different problem, visible in the diff; the guard only
        // looks for invention.
        let source = "Hosts 10.0.0.5 and 10.0.0.6 were affected.";
        let output = "Two hosts were affected.";
        assert!(fabricated_identifiers(source, output).is_empty());
    }

    #[test]
    fn redaction_placeholders_are_not_identifiers() {
        let source = "Host [REDACTED_IP_1] answered.";
        let output = "The host [REDACTED_IP_1] answered the probe.";
        assert!(fabricated_identifiers(source, output).is_empty());
    }

    #[test]
    fn multibyte_output_does_not_panic() {
        let found = fabricated_identifiers("Prüfung", "Die Prüfung ergab 10.0.0.9 – ✅");
        assert_eq!(values(&found), vec!["10.0.0.9"]);
    }
}
