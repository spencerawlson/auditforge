use std::io::Read;

use super::*;
use crate::models::{
    AuthorSnapshot, EngagementSnapshot, EvidenceKind, FindingSnapshot, Severity, VerificationStatus,
};
use crate::templates;

/// Inflate every content stream and pull out the text-showing operands, so tests can
/// assert on what a reader will actually see rather than on the file's structure alone.
fn visible_text(pdf: &[u8]) -> String {
    let mut out = String::new();
    // Match the dictionary terminator too: a bare "stream\n" also occurs inside the
    // "endstream" keyword, which would desynchronize the scan.
    let marker = b">>\nstream\n";
    let mut index = 0usize;

    while let Some(found) = find(&pdf[index..], marker) {
        let start = index + found + marker.len();
        let Some(end_offset) = find(&pdf[start..], b"\nendstream") else {
            break;
        };
        let end = start + end_offset;
        index = end + 1;

        let mut decoder = flate2::read::ZlibDecoder::new(&pdf[start..end]);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err() {
            continue; // an image stream, not page content
        }
        // Content streams carry WinAnsi bytes, which are not valid UTF-8; decode lossily
        // rather than dropping the page.
        let text = String::from_utf8_lossy(&inflated).into_owned();
        out.push_str(&extract_show_operands(&text));
        out.push('\n');
    }
    out
}

fn extract_show_operands(stream: &str) -> String {
    let mut out = String::new();
    let bytes = stream.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'(' {
            index += 1;
            continue;
        }
        let mut cursor = index + 1;
        let mut literal = String::new();
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'\\' if cursor + 1 < bytes.len() => {
                    literal.push(bytes[cursor + 1] as char);
                    cursor += 2;
                }
                b')' => break,
                byte => {
                    literal.push(byte as char);
                    cursor += 1;
                }
            }
        }
        out.push_str(&literal);
        out.push(' ');
        index = cursor + 1;
    }
    out
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Assert that rendered text contains `needle`, printing nearby text when it does not so
/// a failure says what was actually drawn.
#[track_caller]
fn assert_visible(text: &str, needle: &str) {
    if text.contains(needle) {
        return;
    }
    let probe: String = needle
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let context = text
        .find(&probe)
        .map(|at| text[at.saturating_sub(60)..(at + 200).min(text.len())].to_string())
        .unwrap_or_else(|| text.chars().take(600).collect());
    panic!(
        "rendered text does not contain {needle:?}
--- nearby ---
{context}
---"
    );
}

fn author() -> AuthorSnapshot {
    AuthorSnapshot {
        user_id: "u1".to_string(),
        username: "s.rawlson".to_string(),
        full_name: "Spencer Rawlson Spady".to_string(),
        organization: "PLH Ecosystem, Inc.".to_string(),
        job_title: "Cloud Computing & Cybersecurity".to_string(),
        captured_at: Timestamp::now(),
    }
}

fn finding(reference: &str, title: &str, severity: Severity) -> FindingSnapshot {
    FindingSnapshot {
        finding_ref: reference.to_string(),
        title: title.to_string(),
        severity,
        category: "Configuration".to_string(),
        affected_asset: "vpn.example.internal".to_string(),
        description: "The concentrator negotiates TLS 1.0 with external clients.".to_string(),
        technical_details: "openssl s_client reported Protocol : TLSv1".to_string(),
        impact: "Traffic can be downgraded and decrypted by an on-path attacker.".to_string(),
        likelihood: "Moderate".to_string(),
        risk: "Elevated for remote workers on untrusted networks.".to_string(),
        root_cause: "The default protocol list was never restricted.".to_string(),
        remediation: "Disable TLS 1.0 and 1.1 on the concentrator.".to_string(),
        recommendation: "Adopt a hardened baseline for edge devices.".to_string(),
        verification_status: VerificationStatus::Unverified,
        reference_links: "https://example.test/hardening\nhttps://example.test/tls".to_string(),
        cvss_vector: "CVSS:3.1/AV:N/AC:H/PR:N/UI:N/S:U/C:H/I:N/A:N".to_string(),
        cvss_score: Some(5.9),
        provenance: Default::default(),
        author_name: "Spencer Rawlson Spady".to_string(),
        evidence_refs: vec!["EVD-2026-0001".to_string()],
    }
}

fn evidence(reference: &str, mime: &str) -> EvidenceSnapshot {
    EvidenceSnapshot {
        evidence_ref: reference.to_string(),
        title: "TLS handshake capture".to_string(),
        description: "Output of the protocol negotiation test.".to_string(),
        original_filename: "handshake.png".to_string(),
        relative_path: "2026/EVD-2026-0001_handshake.png".to_string(),
        mime_type: mime.to_string(),
        kind: EvidenceKind::Image,
        byte_size: 20_480,
        sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_string(),
        captured_at: Some(Timestamp::now()),
    }
}

fn sample_content() -> (ReportContent, templates::ReportTemplate) {
    let template = templates::parse_builtin()
        .unwrap()
        .into_iter()
        .find(|template| template.key == "penetration_test")
        .unwrap();

    let mut content = template.instantiate(
        "Q3 External Penetration Test",
        Classification::Confidential,
        author(),
        EngagementSnapshot {
            id: "e1".to_string(),
            engagement_ref: "ENG-2026-0007".to_string(),
            name: "Northwind Perimeter".to_string(),
            client_name: "Northwind Traders".to_string(),
            start_date: Some("2026-09-01".to_string()),
            end_date: Some("2026-09-12".to_string()),
            team: vec!["A. Tester - Lead".to_string()],
        },
    );
    content.report_ref = "RPT-2026-0011".to_string();
    content.version_label = "v1.0 Final".to_string();

    for (section_key, question_key, value) in [
        (
            "executive_summary",
            "exec_purpose",
            "Northwind asked for an external test of the \
          perimeter ahead of a compliance review.",
        ),
        (
            "executive_summary",
            "exec_outcome",
            "The perimeter held, with one configuration \
          weakness that should be corrected.",
        ),
        (
            "scope",
            "scope_in",
            "203.0.113.0/24 and the VPN concentrator.",
        ),
        (
            "objectives",
            "objectives_primary",
            "Determine whether the perimeter can be breached \
          from the internet.",
        ),
        (
            "methodology",
            "methodology_approach",
            "Reconnaissance, service enumeration, targeted \
          testing and manual verification.",
        ),
    ] {
        let section = content.section_mut(section_key).unwrap();
        let answer = section
            .answers
            .iter_mut()
            .find(|a| a.question_key == question_key)
            .unwrap();
        answer.value = value.to_string();
    }

    content.findings = vec![
        finding(
            "FND-2026-0042",
            "TLS 1.0 enabled on the VPN concentrator",
            Severity::Medium,
        ),
        finding(
            "FND-2026-0043",
            "Verbose error messages on the login form",
            Severity::Low,
        ),
    ];
    content.evidence = vec![evidence("EVD-2026-0001", "image/png")];

    (content, template)
}

fn no_evidence(_: &EvidenceSnapshot) -> Option<Vec<u8>> {
    None
}

#[test]
fn a_report_renders_to_a_structurally_valid_pdf() {
    let (content, template) = sample_content();
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();

    assert!(pdf.starts_with(b"%PDF-1.7"));
    assert!(pdf.ends_with(b"%%EOF\n"));
    assert!(
        pdf.len() > 4_000,
        "a full report should not be a stub: {} bytes",
        pdf.len()
    );

    let text = String::from_utf8_lossy(&pdf);
    let count: usize = text
        .split("/Count ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap();
    assert!(
        count >= 3,
        "cover, contents and body should span several pages, got {count}"
    );
}

#[test]
fn the_cover_carries_the_author_organization_role_and_identifiers() {
    let (content, template) = sample_content();
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    let text = visible_text(&pdf);

    assert_visible(&text, "AUDITFORGE");
    assert_visible(&text, "PENETRATION TEST REPORT");
    assert_visible(&text, "Q3 External Penetration Test");
    assert_visible(&text, "Spencer Rawlson Spady");
    assert_visible(&text, "PLH Ecosystem, Inc.");
    assert_visible(&text, "Cloud Computing & Cybersecurity");
    assert_visible(&text, "RPT-2026-0011");
    assert_visible(&text, "v1.0 Final");
    assert_visible(&text, "Northwind Perimeter");
}

#[test]
fn the_classification_appears_on_the_cover_and_in_the_running_header() {
    let (mut content, template) = sample_content();
    content.classification = Classification::Restricted;
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    let text = visible_text(&pdf);
    assert!(
        text.matches("RESTRICTED").count() >= 3,
        "cover band plus every page header"
    );
}

#[test]
fn pages_are_numbered_with_a_total() {
    let (content, template) = sample_content();
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    let text = visible_text(&pdf);
    assert!(
        text.contains("Page 2 of "),
        "got: {}",
        &text[..text.len().min(400)]
    );
    assert_visible(&text, "Page 3 of ");
}

#[test]
fn the_table_of_contents_lists_sections_with_page_numbers() {
    let (content, template) = sample_content();
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    let text = visible_text(&pdf);

    assert_visible(&text, "Contents");
    assert_visible(&text, "1.  Executive Summary");
    assert_visible(&text, "Findings");
}

#[test]
fn turning_the_table_of_contents_off_removes_it() {
    let (content, template) = sample_content();
    let mut options = RenderOptions::default();
    options.settings.include_table_of_contents = false;
    let pdf = render(&content, Some(&template), &options, &no_evidence).unwrap();
    assert!(!visible_text(&pdf).contains("Contents"));
}

#[test]
fn findings_appear_in_a_table_and_in_detail() {
    let (content, template) = sample_content();
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    let text = visible_text(&pdf);

    assert_visible(&text, "FND-2026-0042");
    assert_visible(&text, "TLS 1.0 enabled on the VPN concentrator");
    assert_visible(&text, "Medium");
    assert_visible(&text, "Disable TLS 1.0 and 1.1 on the concentrator.");
    assert_visible(&text, "CVSS:3.1/AV:N");
    assert_visible(&text, "Severity is assigned by the author.");
}

#[test]
fn the_evidence_appendix_cites_the_digest() {
    let (content, template) = sample_content();
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    let text = visible_text(&pdf);

    assert_visible(&text, "EVD-2026-0001");
    assert_visible(&text, "handshake.png");
    assert_visible(
        &text,
        "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
    );
    assert_visible(&text, "20.0 KiB");
}

#[test]
fn evidence_images_are_embedded_when_the_bytes_are_available() {
    let (content, template) = sample_content();
    let png = tiny_png();
    let loader = |_: &EvidenceSnapshot| Some(png.clone());

    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &loader,
    )
    .unwrap();
    let raw = String::from_utf8_lossy(&pdf);
    assert!(raw.contains("/Subtype /Image"));
    assert!(raw.contains("/ColorSpace /DeviceRGB"));
}

#[test]
fn unreadable_evidence_is_still_cited_without_failing_the_render() {
    let (content, template) = sample_content();
    let loader = |_: &EvidenceSnapshot| Some(b"not an image at all".to_vec());
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &loader,
    )
    .unwrap();
    let text = visible_text(&pdf);
    assert!(
        text.contains("EVD-2026-0001"),
        "the appendix must still cite the item"
    );
    assert!(!String::from_utf8_lossy(&pdf).contains("/Subtype /Image"));
}

#[test]
fn excluded_and_empty_sections_do_not_appear() {
    let (mut content, template) = sample_content();
    content.section_mut("scope").unwrap().included = false;
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    let text = visible_text(&pdf);

    assert!(
        !text.contains("203.0.113.0/24"),
        "excluded section content must not render"
    );
    // Rules of engagement was never answered, so it is omitted rather than left blank.
    assert!(!text.contains("Rules of Engagement"));
}

#[test]
fn section_numbering_is_contiguous_after_omissions() {
    let (content, template) = sample_content();
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    let text = visible_text(&pdf);
    for number in [
        "1.  Executive Summary",
        "2.  Scope",
        "3.  Objectives",
        "4.  Methodology",
    ] {
        assert!(text.contains(number), "missing heading {number}");
    }
}

#[test]
fn a_gap_marked_as_missing_renders_as_information_required() {
    let (mut content, template) = sample_content();
    let section = content.section_mut("scope").unwrap();
    let answer = section
        .answers
        .iter_mut()
        .find(|a| a.question_key == "scope_limitations")
        .unwrap();
    answer.value = "The client could not provide credentials in time.".to_string();
    answer.provenance = Provenance::MissingInformation;

    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    let text = visible_text(&pdf);
    assert!(text.contains(INFORMATION_REQUIRED));
    assert_visible(&text, "The client could not provide credentials in time.");
}

#[test]
fn a_report_with_ai_assisted_text_carries_a_statement_of_assistance() {
    let (mut content, template) = sample_content();
    let section = content.section_mut("executive_summary").unwrap();
    section.answers[0].provenance = Provenance::AiWording;

    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    let text = visible_text(&pdf);
    assert_visible(&text, "Statement of AI Assistance");
    assert_visible(
        &text,
        "All facts, findings, severities and conclusions are the author's.",
    );
}

#[test]
fn a_report_with_no_ai_assistance_omits_the_statement() {
    let (content, template) = sample_content();
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    assert!(!visible_text(&pdf).contains("Statement of AI Assistance"));
}

#[test]
fn provenance_markers_are_off_by_default_and_can_be_switched_on() {
    let (mut content, template) = sample_content();
    content.section_mut("executive_summary").unwrap().answers[0].provenance = Provenance::AiWording;

    let default_text = visible_text(
        &render(
            &content,
            Some(&template),
            &RenderOptions::default(),
            &no_evidence,
        )
        .unwrap(),
    );
    assert!(!default_text.contains("Provenance: AI wording"));

    let mut options = RenderOptions::default();
    options.settings.show_provenance_markers = true;
    let marked = visible_text(&render(&content, Some(&template), &options, &no_evidence).unwrap());
    assert!(marked.contains("Provenance: AI wording"));
}

#[test]
fn letter_and_a4_both_render() {
    let (content, template) = sample_content();
    for page_size in [PageSize::A4, PageSize::Letter] {
        let mut options = RenderOptions::default();
        options.settings.page_size = page_size;
        let pdf = render(&content, Some(&template), &options, &no_evidence).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        let (width, _) = page_dimensions(page_size);
        assert!(String::from_utf8_lossy(&pdf).contains(&format!("/MediaBox [0 0 {width}")));
    }
}

#[test]
fn a_long_report_paginates_and_the_contents_stay_consistent() {
    let (mut content, template) = sample_content();
    // Enough findings to run well past one page.
    content.findings = (0..40)
        .map(|index| {
            finding(
                &format!("FND-2026-{index:04}"),
                &format!("Finding number {index}"),
                Severity::High,
            )
        })
        .collect();

    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    let raw = String::from_utf8_lossy(&pdf);
    let count: usize = raw
        .split("/Count ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap();
    assert!(count > 8, "expected a long document, got {count} pages");

    let text = visible_text(&pdf);
    assert!(
        text.contains(&format!("Page {count} of {count}")),
        "last page must be numbered"
    );
}

#[test]
fn rendering_without_a_template_still_produces_a_document() {
    let (content, _) = sample_content();
    let pdf = render(&content, None, &RenderOptions::default(), &no_evidence).unwrap();
    let text = visible_text(&pdf);
    assert_visible(&text, "Q3 External Penetration Test");
    // Without the template the question keys are humanized instead of using their labels.
    // Question labels render as tracked capitals.
    assert_visible(&text, "EXEC PURPOSE");
}

#[test]
fn an_almost_empty_report_still_renders_a_cover() {
    let template = templates::parse_builtin()
        .unwrap()
        .into_iter()
        .find(|t| t.key == "custom_report")
        .unwrap();
    let mut content = template.instantiate(
        "Empty",
        Classification::Internal,
        author(),
        EngagementSnapshot::default(),
    );
    content.report_ref = "RPT-2026-0002".to_string();
    content.version_label = "v0.1 Draft".to_string();

    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    assert!(visible_text(&pdf).contains("Empty"));
}

#[test]
fn text_outside_winansi_does_not_break_the_document() {
    let (mut content, template) = sample_content();
    content.title = "Prüfung ✅ 日本語 — report".to_string();
    let pdf = render(
        &content,
        Some(&template),
        &RenderOptions::default(),
        &no_evidence,
    )
    .unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    assert!(visible_text(&pdf).contains("Pr"));
}

#[test]
fn human_sizes_are_readable() {
    assert_eq!(human_size(0), "0 B");
    assert_eq!(human_size(512), "512 B");
    assert_eq!(human_size(2048), "2.0 KiB");
    assert_eq!(human_size(5 * 1024 * 1024), "5.0 MiB");
}

#[test]
fn keys_are_humanized_for_display() {
    assert_eq!(humanize("exec_purpose"), "Exec Purpose");
    assert_eq!(humanize("false_positive"), "False Positive");
    assert_eq!(humanize(""), "");
}

fn tiny_png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 4, 3);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[128u8; 4 * 3 * 3]).unwrap();
    }
    out
}
