//! Redaction of obviously sensitive strings before text is sent to a cloud model.
//!
//! Scope, stated plainly: this catches structured, mechanically recognisable values -
//! IP addresses, emails, cloud access key ids, PEM private key blocks, bearer tokens and
//! `password=`-style assignments. It does **not** attempt to recognise hostnames,
//! project code names, staff names or anything else that needs judgement, because a
//! matcher loose enough to catch those would mangle ordinary prose.
//!
//! Redaction is therefore a safety net over the real control, which is the privacy mode:
//! if text must not leave the machine, use local inference or turn AI off.
//!
//! Each distinct value maps to a stable placeholder, and [`Redactor::restore`] puts the
//! originals back into the model's reply, so the author sees real values in the
//! suggestion even though the provider never did.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveKind {
    PrivateKeyBlock,
    AccessKeyId,
    BearerToken,
    Credential,
    Email,
    IpV4,
    IpV6,
}

impl SensitiveKind {
    fn tag(&self) -> &'static str {
        match self {
            SensitiveKind::PrivateKeyBlock => "KEY",
            SensitiveKind::AccessKeyId => "ACCESS_KEY",
            SensitiveKind::BearerToken => "TOKEN",
            SensitiveKind::Credential => "CREDENTIAL",
            SensitiveKind::Email => "EMAIL",
            SensitiveKind::IpV4 => "IP",
            SensitiveKind::IpV6 => "IP6",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            SensitiveKind::PrivateKeyBlock => "private key block",
            SensitiveKind::AccessKeyId => "cloud access key id",
            SensitiveKind::BearerToken => "bearer token",
            SensitiveKind::Credential => "credential assignment",
            SensitiveKind::Email => "email address",
            SensitiveKind::IpV4 => "IPv4 address",
            SensitiveKind::IpV6 => "IPv6 address",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detection {
    pub kind: SensitiveKind,
    pub text: String,
}

/// Find sensitive spans without altering the text. Used to warn before a cloud call.
pub fn scan(text: &str) -> Vec<Detection> {
    spans(text)
        .into_iter()
        .map(|span| Detection {
            kind: span.kind,
            text: text[span.start..span.end].to_string(),
        })
        .collect()
}

/// Replaces sensitive values with placeholders and can put them back afterwards.
#[derive(Debug, Default)]
pub struct Redactor {
    /// placeholder -> original
    mapping: BTreeMap<String, String>,
    /// original -> placeholder, so the same value always gets the same placeholder
    reverse: BTreeMap<String, String>,
    counters: BTreeMap<&'static str, usize>,
}

impl Redactor {
    pub fn new() -> Self {
        Redactor::default()
    }

    pub fn redact(&mut self, text: &str) -> String {
        let spans = spans(text);
        if spans.is_empty() {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0usize;
        for span in spans {
            if span.start < cursor {
                continue;
            }
            out.push_str(&text[cursor..span.start]);
            let original = &text[span.start..span.end];
            out.push_str(&self.placeholder_for(span.kind, original));
            cursor = span.end;
        }
        out.push_str(&text[cursor..]);
        out
    }

    fn placeholder_for(&mut self, kind: SensitiveKind, original: &str) -> String {
        if let Some(existing) = self.reverse.get(original) {
            return existing.clone();
        }
        let counter = self.counters.entry(kind.tag()).or_insert(0);
        *counter += 1;
        let placeholder = format!("[REDACTED_{}_{}]", kind.tag(), counter);
        self.mapping
            .insert(placeholder.clone(), original.to_string());
        self.reverse
            .insert(original.to_string(), placeholder.clone());
        placeholder
    }

    /// Put the original values back. Applied to model output before the author sees it.
    pub fn restore(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (placeholder, original) in &self.mapping {
            if out.contains(placeholder.as_str()) {
                out = out.replace(placeholder.as_str(), original);
            }
        }
        out
    }

    pub fn redacted_count(&self) -> usize {
        self.mapping.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mapping.is_empty()
    }
}

#[derive(Debug, Clone, Copy)]
struct Span {
    kind: SensitiveKind,
    start: usize,
    end: usize,
}

/// Collect non-overlapping spans, earliest first. Where two matchers overlap the one
/// that started earlier wins; ties go to the longer match.
fn spans(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut found: Vec<Span> = Vec::new();

    let mut index = 0usize;
    while index < bytes.len() {
        if !text.is_char_boundary(index) {
            index += 1;
            continue;
        }
        let rest = &text[index..];
        let matched = match_pem(rest)
            .or_else(|| match_credential(rest))
            .or_else(|| match_bearer(rest))
            .or_else(|| match_access_key(rest))
            .or_else(|| match_email(text, index))
            .or_else(|| match_ipv4(text, index))
            .or_else(|| match_ipv6(text, index));

        match matched {
            Some((kind, len)) if len > 0 => {
                found.push(Span {
                    kind,
                    start: index,
                    end: index + len,
                });
                index += len;
            }
            _ => {
                index += rest.chars().next().map_or(1, |c| c.len_utf8());
            }
        }
    }

    found
}

fn match_pem(rest: &str) -> Option<(SensitiveKind, usize)> {
    if !rest.starts_with("-----BEGIN") {
        return None;
    }
    // Redact through the matching END marker; if there is none, redact the rest.
    match rest.find("-----END") {
        Some(end_start) => {
            let tail = &rest[end_start + "-----END".len()..];
            let terminator = tail.find("-----").map(|p| p + 5).unwrap_or(tail.len());
            Some((
                SensitiveKind::PrivateKeyBlock,
                end_start + "-----END".len() + terminator,
            ))
        }
        None => Some((SensitiveKind::PrivateKeyBlock, rest.len())),
    }
}

const CREDENTIAL_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "api_key",
    "apikey",
    "api-key",
    "token",
    "authorization",
    "client_secret",
    "private_key",
];

fn match_credential(rest: &str) -> Option<(SensitiveKind, usize)> {
    let lowered_prefix: String = rest
        .chars()
        .take(20)
        .collect::<String>()
        .to_ascii_lowercase();
    let key = CREDENTIAL_KEYS
        .iter()
        .find(|candidate| lowered_prefix.starts_with(**candidate))?;
    let mut cursor = key.len();
    let bytes = rest.as_bytes();

    while cursor < bytes.len() && (bytes[cursor] == b' ' || bytes[cursor] == b'\t') {
        cursor += 1;
    }
    if cursor >= bytes.len() || !matches!(bytes[cursor], b'=' | b':') {
        return None;
    }
    cursor += 1;
    while cursor < bytes.len() && (bytes[cursor] == b' ' || bytes[cursor] == b'\t') {
        cursor += 1;
    }
    // Quoted value: take everything to the closing quote.
    if cursor < bytes.len() && matches!(bytes[cursor], b'"' | b'\'') {
        let quote = bytes[cursor];
        let mut end = cursor + 1;
        while end < bytes.len() && bytes[end] != quote {
            end += 1;
        }
        let end = (end + 1).min(bytes.len());
        return (end > cursor + 1).then_some((SensitiveKind::Credential, end));
    }
    let start_of_value = cursor;
    while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor == start_of_value {
        return None;
    }
    // `Authorization: Bearer <token>` puts the scheme where the value would be; take the
    // word after it too, or the token itself would survive.
    const SCHEMES: &[&str] = &["bearer", "basic", "token", "digest", "apikey"];
    if SCHEMES.contains(&rest[start_of_value..cursor].to_ascii_lowercase().as_str()) {
        let mut after = cursor;
        while after < bytes.len() && (bytes[after] == b' ' || bytes[after] == b'\t') {
            after += 1;
        }
        let token_start = after;
        while after < bytes.len() && !bytes[after].is_ascii_whitespace() {
            after += 1;
        }
        if after > token_start {
            cursor = after;
        }
    }
    Some((SensitiveKind::Credential, cursor))
}

fn match_bearer(rest: &str) -> Option<(SensitiveKind, usize)> {
    let prefix = "bearer ";
    let head: String = rest
        .chars()
        .take(prefix.len())
        .collect::<String>()
        .to_ascii_lowercase();
    if head != prefix {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut cursor = prefix.len();
    let start = cursor;
    while cursor < bytes.len()
        && (bytes[cursor].is_ascii_alphanumeric() || matches!(bytes[cursor], b'.' | b'_' | b'-'))
    {
        cursor += 1;
    }
    (cursor - start >= 16).then_some((SensitiveKind::BearerToken, cursor))
}

fn match_access_key(rest: &str) -> Option<(SensitiveKind, usize)> {
    const PREFIXES: &[&str] = &["AKIA", "ASIA", "AIDA", "AROA", "AGPA", "ANPA"];
    if !PREFIXES.iter().any(|prefix| rest.starts_with(prefix)) {
        return None;
    }
    let bytes = rest.as_bytes();
    if bytes.len() < 20 {
        return None;
    }
    let candidate = &bytes[..20];
    if !candidate
        .iter()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
        return None;
    }
    // Reject when the key id is only part of a longer run of key characters.
    if bytes.len() > 20 && (bytes[20].is_ascii_alphanumeric()) {
        return None;
    }
    Some((SensitiveKind::AccessKeyId, 20))
}

fn is_email_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn match_email(text: &str, index: usize) -> Option<(SensitiveKind, usize)> {
    let bytes = text.as_bytes();
    // Only start a match at the beginning of the local part.
    if index > 0 && is_email_char(bytes[index - 1]) {
        return None;
    }
    let mut cursor = index;
    while cursor < bytes.len() && is_email_char(bytes[cursor]) {
        cursor += 1;
    }
    if cursor == index || cursor >= bytes.len() || bytes[cursor] != b'@' {
        return None;
    }
    let domain_start = cursor + 1;
    let mut domain_end = domain_start;
    while domain_end < bytes.len()
        && (bytes[domain_end].is_ascii_alphanumeric() || matches!(bytes[domain_end], b'.' | b'-'))
    {
        domain_end += 1;
    }
    let domain = &text[domain_start..domain_end];
    if !domain.contains('.') || domain.ends_with('.') || domain.len() < 3 {
        return None;
    }
    Some((SensitiveKind::Email, domain_end - index))
}

fn match_ipv4(text: &str, index: usize) -> Option<(SensitiveKind, usize)> {
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
        let digits_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() && cursor - digits_start < 3 {
            cursor += 1;
        }
        if cursor == digits_start {
            return None;
        }
        let value: u32 = text[digits_start..cursor].parse().ok()?;
        if value > 255 {
            return None;
        }
    }
    // A trailing digit or dot means this was part of something longer, such as a version
    // number like 10.0.0.5.1 or a decimal.
    if cursor < bytes.len() && (bytes[cursor].is_ascii_digit() || bytes[cursor] == b'.') {
        return None;
    }
    Some((SensitiveKind::IpV4, cursor - index))
}

fn match_ipv6(text: &str, index: usize) -> Option<(SensitiveKind, usize)> {
    let bytes = text.as_bytes();
    let is_ipv6_char = |b: u8| b.is_ascii_hexdigit() || b == b':';
    if index > 0 && is_ipv6_char(bytes[index - 1]) {
        return None;
    }
    let mut cursor = index;
    while cursor < bytes.len() && is_ipv6_char(bytes[cursor]) {
        cursor += 1;
    }
    // Trim a trailing colon so "fe80::1:" does not swallow the punctuation.
    while cursor > index && bytes[cursor - 1] == b':' && !text[index..cursor].ends_with("::") {
        cursor -= 1;
    }
    let candidate = &text[index..cursor];
    if candidate.len() < 3 {
        return None;
    }
    let colons = candidate.matches(':').count();
    // Two or more colons rules out clock times and "key: value" pairs.
    if colons < 2 {
        return None;
    }
    candidate.parse::<std::net::Ipv6Addr>().ok()?;
    Some((SensitiveKind::IpV6, cursor - index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_addresses_are_found_and_restored() {
        let mut redactor = Redactor::new();
        let original = "Host 10.20.30.40 answered; 192.168.1.1 did not.";
        let redacted = redactor.redact(original);
        assert!(!redacted.contains("10.20.30.40"));
        assert!(!redacted.contains("192.168.1.1"));
        assert!(redacted.contains("[REDACTED_IP_1]"));
        assert_eq!(redactor.restore(&redacted), original);
    }

    #[test]
    fn the_same_value_reuses_one_placeholder() {
        let mut redactor = Redactor::new();
        let redacted = redactor.redact("10.0.0.1 talked to 10.0.0.1 twice");
        assert_eq!(redacted.matches("[REDACTED_IP_1]").count(), 2);
        assert_eq!(redactor.redacted_count(), 1);
    }

    #[test]
    fn version_numbers_and_decimals_are_left_alone() {
        let mut redactor = Redactor::new();
        let text = "Upgraded OpenSSL to 3.0.11 and saw 99.5 percent success on 1.2.3.4.5";
        let redacted = redactor.redact(text);
        assert_eq!(redacted, text, "got {redacted}");
        assert!(redactor.is_empty());
    }

    #[test]
    fn emails_and_access_keys_are_redacted() {
        let mut redactor = Redactor::new();
        let redacted =
            redactor.redact("Contact soc@example.com using AKIAIOSFODNN7EXAMPLE for access");
        assert!(!redacted.contains("soc@example.com"));
        assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(redacted.contains("[REDACTED_EMAIL_1]"));
        assert!(redacted.contains("[REDACTED_ACCESS_KEY_1]"));
    }

    #[test]
    fn credential_assignments_and_bearer_tokens_are_redacted() {
        let mut redactor = Redactor::new();
        let redacted = redactor.redact(
            "password=hunter2-correct api_key: \"sk-abc123\" Authorization: Bearer abcdefghijklmnopqrstuv",
        );
        assert!(!redacted.contains("hunter2-correct"));
        assert!(!redacted.contains("sk-abc123"));
        assert!(!redacted.contains("abcdefghijklmnopqrstuv"));
    }

    #[test]
    fn pem_blocks_are_redacted_whole() {
        let mut redactor = Redactor::new();
        let text = "Found this:\n-----BEGIN RSA PRIVATE KEY-----\nMIIEow...\n-----END RSA PRIVATE KEY-----\nin the repo.";
        let redacted = redactor.redact(text);
        assert!(!redacted.contains("MIIEow"));
        assert!(redacted.contains("in the repo."));
        assert!(redacted.contains("[REDACTED_KEY_1]"));
    }

    #[test]
    fn ipv6_is_recognized_but_clock_times_are_not() {
        let mut redactor = Redactor::new();
        let redacted = redactor.redact("At 09:15 the host fe80::1ff:fe23:4567:890a reset");
        assert!(redacted.contains("09:15"), "got {redacted}");
        assert!(!redacted.contains("fe80::1ff"), "got {redacted}");
    }

    #[test]
    fn restore_is_the_exact_inverse_for_mixed_content() {
        let mut redactor = Redactor::new();
        let original = "Attacker at 203.0.113.9 used creds for ops@example.org; \
                        token Bearer aaaaaaaaaaaaaaaaaaaa was replayed against 203.0.113.10.";
        let redacted = redactor.redact(original);
        assert_eq!(redactor.restore(&redacted), original);
    }

    #[test]
    fn scanning_reports_without_modifying() {
        let detections = scan("Ping 8.8.8.8 from admin@example.com");
        let kinds: Vec<_> = detections.iter().map(|d| d.kind).collect();
        assert!(kinds.contains(&SensitiveKind::IpV4));
        assert!(kinds.contains(&SensitiveKind::Email));
    }

    #[test]
    fn plain_prose_is_untouched() {
        let mut redactor = Redactor::new();
        let text = "The web server rejected the request because the certificate had expired.";
        assert_eq!(redactor.redact(text), text);
    }

    #[test]
    fn multibyte_text_does_not_panic_or_corrupt() {
        let mut redactor = Redactor::new();
        let text = "Der Server 10.1.2.3 antwortete nicht - Zeitüberschreitung, naïve Prüfung ✅";
        let redacted = redactor.redact(text);
        assert!(redacted.contains("Zeitüberschreitung"));
        assert!(redacted.contains("✅"));
        assert_eq!(redactor.restore(&redacted), text);
    }
}
