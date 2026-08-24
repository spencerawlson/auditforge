//! Evidence type detection and import policy.
//!
//! The declared filename is treated as a hint, never as truth: the stored MIME type and
//! kind come from sniffing the leading bytes, so renaming `payload.exe` to `screenshot.png`
//! does not change how AuditForge classifies or renders it.
//!
//! Executable and script types are refused by default. They are refused rather than
//! silently accepted because the common case - a screenshot, a log, a capture - should
//! never involve one, and a security tool that quietly stores executables next to report
//! data invites exactly the mistake it is supposed to prevent. Security work does
//! sometimes need to attach a sample, so the refusal is overridable per import with an
//! explicit acknowledgement; the override is recorded in the audit trail.

use crate::error::{Error, Result};
use crate::models::EvidenceKind;

/// Hard ceiling on a single evidence file.
pub const MAX_EVIDENCE_BYTES: u64 = 100 * 1024 * 1024;

/// Extensions treated as executable or directly runnable.
const RISKY_EXTENSIONS: &[&str] = &[
    "exe", "dll", "com", "scr", "msi", "msp", "cpl", "jar", "apk", "app", "bat", "cmd", "ps1",
    "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "sh", "bash", "zsh", "fish", "so",
    "dylib", "bin", "elf", "run", "deb", "rpm", "py", "pyc", "rb", "pl", "php", "lnk", "reg",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedType {
    pub mime_type: String,
    pub kind: EvidenceKind,
    /// True when the file looks executable by content or by extension.
    pub risky: bool,
    /// Set when the extension disagrees with the bytes.
    pub extension_mismatch: Option<String>,
}

/// Inspect the first bytes of a file and decide what it is.
pub fn detect(filename: &str, bytes: &[u8]) -> DetectedType {
    let extension = filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();

    let (sniffed_mime, sniffed_kind, executable_content) = sniff(bytes);
    let risky_extension = RISKY_EXTENSIONS.contains(&extension.as_str());

    // Where sniffing is inconclusive, fall back to the extension for text-ish formats
    // that have no magic bytes.
    let (mime_type, kind) = match sniffed_mime {
        Some(mime) => (mime.to_string(), sniffed_kind),
        None => from_extension(&extension, bytes),
    };

    let extension_mismatch = expected_extensions(&mime_type)
        .filter(|expected| !extension.is_empty() && !expected.contains(&extension.as_str()))
        .map(|expected| {
            format!(
                "file is {mime_type} but is named .{extension}; expected .{}",
                expected.join(" or .")
            )
        });

    DetectedType {
        mime_type,
        kind,
        risky: risky_extension || executable_content,
        extension_mismatch,
    }
}

/// Enforce import policy. `acknowledged` is the user's explicit override.
pub fn check_policy(detected: &DetectedType, filename: &str, acknowledged: bool) -> Result<()> {
    if detected.risky && !acknowledged {
        return Err(Error::validation(format!(
            "{filename} looks like an executable or script. \
             Re-import with the executable-evidence acknowledgement if that is intended."
        )));
    }
    Ok(())
}

/// Returns (mime, kind, looks_executable). `None` mime means "no signature matched".
fn sniff(bytes: &[u8]) -> (Option<&'static str>, EvidenceKind, bool) {
    let starts = |prefix: &[u8]| bytes.len() >= prefix.len() && &bytes[..prefix.len()] == prefix;

    if starts(b"\x89PNG\r\n\x1a\n") {
        return (Some("image/png"), EvidenceKind::Image, false);
    }
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return (Some("image/jpeg"), EvidenceKind::Image, false);
    }
    if starts(b"GIF87a") || starts(b"GIF89a") {
        return (Some("image/gif"), EvidenceKind::Image, false);
    }
    if starts(b"BM") && bytes.len() > 14 {
        return (Some("image/bmp"), EvidenceKind::Image, false);
    }
    if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return (Some("image/webp"), EvidenceKind::Image, false);
    }
    if starts(b"%PDF-") {
        return (Some("application/pdf"), EvidenceKind::Other, false);
    }
    // libpcap, both byte orders, and pcapng.
    if starts(&[0xD4, 0xC3, 0xB2, 0xA1])
        || starts(&[0xA1, 0xB2, 0xC3, 0xD4])
        || starts(&[0x0A, 0x0D, 0x0D, 0x0A])
    {
        return (
            Some("application/vnd.tcpdump.pcap"),
            EvidenceKind::Capture,
            false,
        );
    }
    if starts(b"PK\x03\x04") {
        return (Some("application/zip"), EvidenceKind::Archive, false);
    }
    if starts(&[0x1F, 0x8B]) {
        return (Some("application/gzip"), EvidenceKind::Archive, false);
    }
    if starts(b"7z\xBC\xAF\x27\x1C") {
        return (
            Some("application/x-7z-compressed"),
            EvidenceKind::Archive,
            false,
        );
    }

    // Executable signatures. Detected by content so a rename cannot hide them.
    if starts(b"MZ") {
        return (
            Some("application/vnd.microsoft.portable-executable"),
            EvidenceKind::Other,
            true,
        );
    }
    if starts(b"\x7FELF") {
        return (Some("application/x-elf"), EvidenceKind::Other, true);
    }
    if starts(b"#!") {
        return (Some("text/x-shellscript"), EvidenceKind::Text, true);
    }
    if starts(&[0xCF, 0xFA, 0xED, 0xFE]) || starts(&[0xCE, 0xFA, 0xED, 0xFE]) {
        return (Some("application/x-mach-binary"), EvidenceKind::Other, true);
    }

    (None, EvidenceKind::Other, false)
}

fn from_extension(extension: &str, bytes: &[u8]) -> (String, EvidenceKind) {
    let looks_text = std::str::from_utf8(&bytes[..bytes.len().min(4096)]).is_ok();
    match extension {
        "json" if looks_text => ("application/json".to_string(), EvidenceKind::Json),
        "csv" if looks_text => ("text/csv".to_string(), EvidenceKind::Csv),
        "log" | "out" if looks_text => ("text/plain".to_string(), EvidenceKind::Log),
        "txt" | "md" | "conf" | "cfg" | "ini" | "yaml" | "yml" | "xml" | "html" if looks_text => {
            ("text/plain".to_string(), EvidenceKind::Text)
        }
        "pcap" | "pcapng" | "cap" => (
            "application/vnd.tcpdump.pcap".to_string(),
            EvidenceKind::Capture,
        ),
        _ if looks_text => ("text/plain".to_string(), EvidenceKind::Text),
        _ => ("application/octet-stream".to_string(), EvidenceKind::Other),
    }
}

fn expected_extensions(mime_type: &str) -> Option<Vec<&'static str>> {
    match mime_type {
        "image/png" => Some(vec!["png"]),
        "image/jpeg" => Some(vec!["jpg", "jpeg"]),
        "image/gif" => Some(vec!["gif"]),
        "application/pdf" => Some(vec!["pdf"]),
        "application/zip" => Some(vec!["zip"]),
        "application/gzip" => Some(vec!["gz", "tgz"]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";

    #[test]
    fn images_are_detected_by_signature() {
        let detected = detect("screenshot.png", PNG);
        assert_eq!(detected.mime_type, "image/png");
        assert_eq!(detected.kind, EvidenceKind::Image);
        assert!(!detected.risky);
        assert!(detected.extension_mismatch.is_none());

        let jpeg = detect("photo.jpg", &[0xFF, 0xD8, 0xFF, 0xE0, 0x00]);
        assert_eq!(jpeg.kind, EvidenceKind::Image);
    }

    #[test]
    fn a_renamed_executable_is_still_recognised_as_executable() {
        let detected = detect("screenshot.png", b"MZ\x90\x00\x03");
        assert!(
            detected.risky,
            "PE header must be caught regardless of the name"
        );
        assert_eq!(detected.kind, EvidenceKind::Other);
        assert!(detected.extension_mismatch.is_none() || detected.risky);
        assert!(check_policy(&detected, "screenshot.png", false).is_err());
        assert!(check_policy(&detected, "screenshot.png", true).is_ok());
    }

    #[test]
    fn elf_and_shell_scripts_are_flagged() {
        assert!(detect("tool", b"\x7FELF\x02\x01").risky);
        assert!(detect("collect.txt", b"#!/bin/bash\nid\n").risky);
    }

    #[test]
    fn risky_extensions_are_flagged_even_with_harmless_content() {
        let detected = detect("payload.ps1", b"Write-Output 'hello'");
        assert!(detected.risky);
        assert!(check_policy(&detected, "payload.ps1", false).is_err());
    }

    #[test]
    fn extension_mismatch_is_reported_for_known_formats() {
        let detected = detect("evidence.jpg", PNG);
        assert_eq!(detected.mime_type, "image/png");
        let message = detected
            .extension_mismatch
            .expect("mismatch should be reported");
        assert!(message.contains("image/png"), "got {message}");
    }

    #[test]
    fn text_formats_fall_back_to_the_extension() {
        assert_eq!(
            detect("scan.json", b"{\"ports\":[22]}").kind,
            EvidenceKind::Json
        );
        assert_eq!(
            detect("hosts.csv", b"ip,port\n10.0.0.1,22\n").kind,
            EvidenceKind::Csv
        );
        assert_eq!(
            detect("auth.log", b"Aug 24 09:15 sshd: accepted").kind,
            EvidenceKind::Log
        );
        assert_eq!(detect("notes.txt", b"free text").kind, EvidenceKind::Text);
    }

    #[test]
    fn captures_and_archives_are_recognised() {
        assert_eq!(
            detect("capture.pcap", &[0xD4, 0xC3, 0xB2, 0xA1]).kind,
            EvidenceKind::Capture
        );
        assert_eq!(
            detect("bundle.zip", b"PK\x03\x04\x14").kind,
            EvidenceKind::Archive
        );
        assert_eq!(
            detect("logs.gz", &[0x1F, 0x8B, 0x08]).kind,
            EvidenceKind::Archive
        );
    }

    #[test]
    fn unknown_binary_content_is_other_and_not_risky() {
        let detected = detect("mystery.dat", &[0x00, 0x01, 0x02, 0xFF, 0xFE]);
        assert_eq!(detected.mime_type, "application/octet-stream");
        assert_eq!(detected.kind, EvidenceKind::Other);
        assert!(!detected.risky);
        assert!(check_policy(&detected, "mystery.dat", false).is_ok());
    }

    #[test]
    fn empty_and_tiny_inputs_do_not_panic() {
        for bytes in [&b""[..], &b"M"[..], &b"\x89"[..], &b"PK"[..]] {
            let _ = detect("thing.bin", bytes);
        }
    }
}
