//! Generate a sample of every report type plus a postmortem, and print each one's
//! estimated page count and completeness verdict against the professional standard. This is
//! the manual verification for the minimum-length / completeness work: it renders real PDFs
//! and shows the page-count / validation / flagging behaviour end to end.
//!
//! ```text
//! cargo run -p auditforge-core --example standards_demo -- "<output dir>"
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use auditforge_core::models::*;
use auditforge_core::pdf::{self, RenderOptions};
use auditforge_core::reports::standards;
use auditforge_core::templates::{self, ReportTemplate};

fn author() -> AuthorSnapshot {
    AuthorSnapshot {
        user_id: "u1".into(),
        username: "srspady".into(),
        full_name: "S. Rspady".into(),
        organization: "LabNet Security".into(),
        job_title: "Security Analyst".into(),
        captured_at: Timestamp::now(),
    }
}

/// A block of genuinely substantive prose (not filler) so a sample report reaches a
/// realistic length. In a real report this is the author's own writing.
fn prose(label: &str) -> String {
    format!(
        "This section documents {label} in full. The assessment was conducted against the \
         agreed scope, and the observations below are drawn directly from the collected \
         evidence rather than assumed. Each statement can be traced to a specific artefact: a \
         command transcript, a screenshot, or a configuration export retained with the \
         engagement record. Where a control operated as intended it is noted as such, and \
         where it did not the gap is described precisely enough to be reproduced and then \
         remediated by the responsible team.",
    )
}

fn finding(reference: &str, title: &str, severity: Severity) -> FindingSnapshot {
    FindingSnapshot {
        finding_ref: reference.into(),
        title: title.into(),
        severity,
        category: "Configuration".into(),
        affected_asset: "10.10.10.5".into(),
        description: prose("the finding"),
        technical_details: prose("the technical detail behind the finding"),
        impact: prose("the business and technical impact"),
        likelihood: "Likely under the observed conditions.".into(),
        risk: prose("the resulting risk"),
        root_cause: prose("the root cause"),
        remediation: prose("the remediation the owning team should apply"),
        recommendation: prose("the longer-term recommendation"),
        verification_status: VerificationStatus::Unverified,
        reference_links: "https://example.internal/kb/123".into(),
        cvss_vector: "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H".into(),
        cvss_score: Some(9.8),
        provenance: BTreeMap::new(),
        author_name: "S. Rspady".into(),
        evidence_refs: Vec::new(),
    }
}

fn fill(content: &mut ReportContent, template: &ReportTemplate) {
    for section in &template.sections {
        let Some(sc) = content.section_mut(&section.key) else {
            continue;
        };
        for question in &section.questions {
            if let Some(answer) = sc.answers.iter_mut().find(|a| a.question_key == question.key) {
                answer.value = prose(&question.label.to_lowercase());
                answer.provenance = Provenance::UserProvided;
            }
        }
    }
    // Give the findings section something to render.
    content.findings = vec![
        finding("FND-2026-0003", "Exposed WinRM over HTTP", Severity::Critical),
        finding("FND-2026-0004", "SMB signing not required", Severity::Medium),
        finding("FND-2026-0005", "Local admin password reuse", Severity::High),
    ];
}

fn main() -> auditforge_core::Result<()> {
    let out_dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&out_dir)?;
    let options = RenderOptions::default();

    println!(
        "{:<28} {:>4} {:>4}  {:<16} {:>5}  {}",
        "REPORT TYPE", "min", "est", "status", "%", "pdf bytes"
    );
    println!("{}", "-".repeat(84));

    for template in templates::parse_builtin()? {
        let mut content = template.instantiate(
            &format!("Sample {}", template.name),
            Classification::Internal,
            author(),
            EngagementSnapshot::default(),
        );
        fill(&mut content, &template);

        let pages = pdf::estimate_pages(&content, Some(&template), &options);
        let completeness = standards::evaluate(&content, Some(&template), pages);
        let bytes = pdf::render(&content, Some(&template), &options, &|_| None)?;

        let filename = format!("sample_{}.pdf", template.key);
        std::fs::write(out_dir.join(&filename), &bytes)?;

        println!(
            "{:<28} {:>4} {:>4}  {:<16} {:>4}%  {} ({})",
            template.report_type.as_str(),
            completeness.min_pages,
            completeness.estimated_pages,
            completeness.status.as_str(),
            completeness.percent,
            bytes.len(),
            filename,
        );
        if !completeness.missing_sections.is_empty() {
            println!("    missing: {}", completeness.missing_sections.join(", "));
        }
    }

    println!("\nSample PDFs written to {}", out_dir.display());
    Ok(())
}
