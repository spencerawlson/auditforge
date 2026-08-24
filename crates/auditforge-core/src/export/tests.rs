use super::*;
use crate::models::{
    AuthorSnapshot, Classification, EngagementSnapshot, EvidenceKind, EvidenceSnapshot, Severity,
    Timestamp, VerificationStatus,
};
use crate::templates;

fn sample() -> (ReportContent, ReportTemplate) {
    let template = templates::parse_builtin()
        .unwrap()
        .into_iter()
        .find(|template| template.key == "penetration_test")
        .unwrap();

    let mut content = template.instantiate(
        "Q3 External Penetration Test",
        Classification::Confidential,
        AuthorSnapshot {
            user_id: "u1".to_string(),
            username: "s.rawlson".to_string(),
            full_name: "Spencer Rawlson Spady".to_string(),
            organization: "PLH Ecosystem, Inc.".to_string(),
            job_title: "Cloud Computing & Cybersecurity".to_string(),
            captured_at: Timestamp::now(),
        },
        EngagementSnapshot {
            name: "Northwind Perimeter".to_string(),
            client_name: "Northwind Traders".to_string(),
            ..Default::default()
        },
    );
    content.report_ref = "RPT-2026-0011".to_string();
    content.version_label = "v1.0 Final".to_string();

    let section = content.section_mut("executive_summary").unwrap();
    section.answers[0].value = "The perimeter was tested over two weeks.".to_string();

    content.findings = vec![FindingSnapshot {
        finding_ref: "FND-2026-0042".to_string(),
        title: "TLS 1.0 enabled".to_string(),
        severity: Severity::Medium,
        category: "Configuration".to_string(),
        affected_asset: "vpn.example.internal".to_string(),
        description: "The concentrator negotiates TLS 1.0.".to_string(),
        technical_details: String::new(),
        impact: "Traffic can be downgraded.".to_string(),
        likelihood: String::new(),
        risk: String::new(),
        root_cause: String::new(),
        remediation: "Disable TLS 1.0.".to_string(),
        recommendation: String::new(),
        verification_status: VerificationStatus::Unverified,
        reference_links: String::new(),
        cvss_vector: String::new(),
        cvss_score: Some(5.9),
        provenance: Default::default(),
        author_name: "Spencer Rawlson Spady".to_string(),
        evidence_refs: vec!["EVD-2026-0001".to_string()],
    }];
    content.evidence = vec![EvidenceSnapshot {
        evidence_ref: "EVD-2026-0001".to_string(),
        title: "Handshake capture".to_string(),
        description: String::new(),
        original_filename: "handshake.png".to_string(),
        relative_path: "2026/EVD-2026-0001_handshake.png".to_string(),
        mime_type: "image/png".to_string(),
        kind: EvidenceKind::Image,
        byte_size: 2048,
        sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_string(),
        captured_at: None,
    }];

    (content, template)
}

#[test]
fn json_round_trips_the_document_exactly() {
    let (content, _) = sample();
    let json = to_json(&content).unwrap();
    let restored: ReportContent = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, content);
}

#[test]
fn html_is_self_contained_and_carries_the_report() {
    let (content, template) = sample();
    let html = to_html(&content, Some(&template));

    assert!(html.starts_with("<!doctype html>"));
    assert!(html.trim_end().ends_with("</html>"));
    assert!(html.contains("Q3 External Penetration Test"));
    assert!(html.contains("Spencer Rawlson Spady"));
    assert!(html.contains("PLH Ecosystem, Inc."));
    assert!(html.contains("CONFIDENTIAL"));
    assert!(html.contains("FND-2026-0042"));
    assert!(html.contains("9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"));

    // No external requests. A URL inside report text is fine - it is escaped content, not
    // a resource reference - so assert on the things a browser would actually fetch.
    assert!(!html.contains("<script"), "the page must not carry script");
    assert!(
        !html.contains("<link"),
        "the page must not link a stylesheet"
    );
    assert!(
        !html.contains("src="),
        "the page must not load an external resource"
    );
    assert!(!html.contains("@import"));
}

#[test]
fn html_escapes_report_text_that_looks_like_markup() {
    let (mut content, template) = sample();
    content.title = "<script>alert(1)</script>".to_string();
    content.findings[0].description =
        "Payload used: <img src=x onerror=alert(1)> and \"quotes\"".to_string();

    let html = to_html(&content, Some(&template));
    assert!(!html.contains("<script>alert(1)</script>"));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<img src=x"));
    assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
}

#[test]
fn a_url_in_report_text_is_rendered_as_text_not_as_a_link() {
    let (mut content, template) = sample();
    content.findings[0].reference_links = "https://example.test/advisory".to_string();
    let html = to_html(&content, Some(&template));

    assert!(
        html.contains("https://example.test/advisory"),
        "the reference must be readable"
    );
    assert!(
        !html.contains("<a href"),
        "report text must not become a live link"
    );
}

#[test]
fn html_omits_sections_with_nothing_in_them() {
    let (content, template) = sample();
    let html = to_html(&content, Some(&template));
    assert!(html.contains("Executive Summary"));
    assert!(
        !html.contains("Rules of Engagement"),
        "an unanswered section must be omitted"
    );
}

#[test]
fn markdown_carries_the_headings_tables_and_findings() {
    let (content, template) = sample();
    let markdown = to_markdown(&content, Some(&template));

    assert!(markdown.starts_with("# Q3 External Penetration Test"));
    assert!(markdown.contains("> CONFIDENTIAL"));
    assert!(markdown.contains("| **Prepared by** | Spencer Rawlson Spady |"));
    assert!(markdown.contains("## 1. Executive Summary"));
    assert!(markdown.contains("### FND-2026-0042 - TLS 1.0 enabled"));
    assert!(markdown.contains("| ID | Finding | Severity | Affected |"));
}

#[test]
fn markdown_escapes_pipes_so_tables_do_not_break() {
    let (mut content, template) = sample();
    content.findings[0].title = "Injection via a | pipe".to_string();
    let markdown = to_markdown(&content, Some(&template));
    assert!(markdown.contains(r"Injection via a \| pipe"));
}

#[test]
fn csv_has_a_header_and_one_row_per_finding() {
    let (content, _) = sample();
    let csv = findings_to_csv(&content.findings);
    let lines: Vec<&str> = csv.trim_end().split("\r\n").collect();

    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("id,title,severity"));
    assert!(lines[1].starts_with("FND-2026-0042,TLS 1.0 enabled,Medium"));
    assert!(lines[1].contains("5.9"));
}

#[test]
fn csv_quotes_fields_containing_separators_and_newlines() {
    let (mut content, _) = sample();
    content.findings[0].description = "First line,\nsecond \"quoted\" line".to_string();
    let csv = findings_to_csv(&content.findings);
    assert!(csv.contains("\"First line,\nsecond \"\"quoted\"\" line\""));
}

#[test]
fn csv_defuses_spreadsheet_formulas() {
    let (mut content, _) = sample();
    content.findings[0].title = "=cmd|'/c calc'!A1".to_string();
    let csv = findings_to_csv(&content.findings);
    assert!(
        csv.contains("'=cmd"),
        "a leading = must be neutralized: {csv}"
    );
    assert!(!csv.contains(",=cmd"));

    assert_eq!(csv_field("+1"), "'+1");
    assert_eq!(csv_field("-1"), "'-1");
    assert_eq!(csv_field("@SUM(A1)"), "'@SUM(A1)");
    assert_eq!(csv_field("normal"), "normal");
}

#[test]
fn empty_findings_produce_a_header_only_csv() {
    let csv = findings_to_csv(&[]);
    assert_eq!(csv.trim_end().lines().count(), 1);
}

#[test]
fn suggested_filenames_are_safe_and_carry_the_reference() {
    let (mut content, _) = sample();
    let name = suggested_filename(&content, ExportFormat::Pdf);
    assert!(name.starts_with("RPT-2026-0011"));
    assert!(name.ends_with(".pdf"));

    content.title = "../../etc/passwd".to_string();
    content.report_ref = "RPT/../../x".to_string();
    let hostile = suggested_filename(&content, ExportFormat::Html);
    assert!(!hostile.contains('/'), "got {hostile}");
    assert!(!hostile.contains(".."), "got {hostile}");
    assert!(hostile.ends_with(".html"));
}

#[test]
fn every_format_has_an_extension_and_a_label() {
    for format in [
        ExportFormat::Pdf,
        ExportFormat::Html,
        ExportFormat::Json,
        ExportFormat::Markdown,
        ExportFormat::Csv,
    ] {
        assert!(!format.extension().is_empty());
        assert!(!format.label().is_empty());
        let json = serde_json::to_string(&format).unwrap();
        let back: ExportFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(back, format);
    }
}
