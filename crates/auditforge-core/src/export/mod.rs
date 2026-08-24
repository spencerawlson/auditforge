//! Export formats other than PDF.
//!
//! * **JSON** - the stored document exactly as AuditForge holds it, for archival and for
//!   feeding another tool.
//! * **HTML** - one self-contained file with inline styles, readable in a browser and
//!   printable, with no external requests.
//! * **Markdown** - for pasting into a wiki or a ticket.
//! * **CSV** - the findings table, for a spreadsheet or a tracker import.
//!
//! DOCX is deliberately absent: producing a genuinely valid `.docx` needs an OOXML
//! package, and shipping something that only *looks* like Word output would be worse than
//! not offering it. See `docs/architecture.md`.

use std::fmt::Write as _;

use crate::error::Result;
use crate::models::{FindingSnapshot, Provenance, ReportContent};
use crate::templates::{ReportTemplate, SectionKind};

/// Formats a report can be exported to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Pdf,
    Html,
    Json,
    Markdown,
    Csv,
}

impl ExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Pdf => "pdf",
            ExportFormat::Html => "html",
            ExportFormat::Json => "json",
            ExportFormat::Markdown => "md",
            ExportFormat::Csv => "csv",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ExportFormat::Pdf => "PDF document",
            ExportFormat::Html => "HTML page",
            ExportFormat::Json => "JSON data",
            ExportFormat::Markdown => "Markdown",
            ExportFormat::Csv => "Findings CSV",
        }
    }
}

/// Serialize the document exactly as stored.
pub fn to_json(content: &ReportContent) -> Result<String> {
    Ok(serde_json::to_string_pretty(content)?)
}

/// A suggested filename, safe on every target platform.
pub fn suggested_filename(content: &ReportContent, format: ExportFormat) -> String {
    let stem = format!(
        "{}_{}_{}",
        if content.report_ref.is_empty() {
            "report"
        } else {
            &content.report_ref
        },
        slug(&content.title),
        content.version_label.replace(' ', "-")
    );
    let safe = crate::security::sanitize_filename(&stem);
    format!("{safe}.{}", format.extension())
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            previous_dash = false;
        } else if !previous_dash {
            out.push('-');
            previous_dash = true;
        }
    }
    out.trim_matches('-').chars().take(60).collect()
}

// ------------------------------------------------------------------ HTML

pub fn to_html(content: &ReportContent, template: Option<&ReportTemplate>) -> String {
    let mut out = String::with_capacity(16 * 1024);
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    let _ = writeln!(
        out,
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{} - {}</title>",
        escape(&content.title),
        escape(&content.report_ref)
    );
    out.push_str(HTML_STYLE);
    out.push_str("</head>\n<body>\n");

    let _ = writeln!(
        out,
        "<div class=\"banner\">{}</div>",
        escape(content.classification.banner())
    );

    out.push_str("<header class=\"cover\">\n");
    out.push_str("<p class=\"wordmark\">AUDITFORGE</p>\n");
    let _ = writeln!(
        out,
        "<p class=\"kind\">{}</p>",
        escape(content.report_type.title())
    );
    let _ = writeln!(out, "<h1>{}</h1>", escape(&content.title));
    out.push_str("<dl class=\"meta\">\n");
    for (key, value) in [
        ("Prepared by", content.author.full_name.as_str()),
        ("Organization", content.author.organization.as_str()),
        ("Role", content.author.job_title.as_str()),
        ("Engagement", content.engagement.name.as_str()),
        ("Client", content.engagement.client_name.as_str()),
        ("Report ID", content.report_ref.as_str()),
        ("Version", content.version_label.as_str()),
        ("Classification", content.classification.banner()),
    ] {
        if value.trim().is_empty() {
            continue;
        }
        let _ = writeln!(out, "<dt>{}</dt><dd>{}</dd>", escape(key), escape(value));
    }
    let _ = writeln!(
        out,
        "<dt>Date</dt><dd>{}</dd>",
        escape(&content.generated_at.display_long())
    );
    out.push_str("</dl>\n</header>\n");

    let mut number = 0usize;
    for section in content.sections.iter().filter(|section| section.included) {
        let kind = template
            .and_then(|template| template.section(&section.key))
            .map(|section| section.kind)
            .unwrap_or(SectionKind::Guided);

        let has_body = match kind {
            SectionKind::Guided => section.has_content(),
            SectionKind::Findings => !content.findings.is_empty(),
            SectionKind::Evidence => !content.evidence.is_empty(),
        };
        if !has_body {
            continue;
        }
        number += 1;

        let _ = writeln!(
            out,
            "<section>\n<h2><span class=\"num\">{}.</span> {}</h2>",
            number,
            escape(&section.title)
        );

        match kind {
            SectionKind::Guided => {
                for answer in section.answers.iter().filter(|answer| !answer.is_blank()) {
                    let label = template
                        .and_then(|template| template.question(&section.key, &answer.question_key))
                        .map(|question| question.label.clone())
                        .unwrap_or_else(|| answer.question_key.clone());
                    let _ = writeln!(out, "<h3>{}</h3>", escape(&label));
                    if answer.provenance == Provenance::MissingInformation {
                        let _ = writeln!(
                            out,
                            "<p class=\"gap\">Information required. {}</p>",
                            escape(answer.value.trim())
                        );
                    } else {
                        out.push_str(&paragraphs(&answer.value));
                    }
                }
            }
            SectionKind::Findings => {
                out.push_str(
                    "<table>\n<thead><tr><th>ID</th><th>Finding</th><th>Severity</th>\
                     <th>Affected</th></tr></thead>\n<tbody>\n",
                );
                for finding in &content.findings {
                    let _ = writeln!(
                        out,
                        "<tr><td>{}</td><td>{}</td>\
                         <td><span class=\"sev sev-{}\">{}</span></td><td>{}</td></tr>",
                        escape(&finding.finding_ref),
                        escape(&finding.title),
                        finding.severity.as_str(),
                        escape(finding.severity.title()),
                        escape(&finding.affected_asset)
                    );
                }
                out.push_str("</tbody>\n</table>\n");

                for finding in &content.findings {
                    let _ = writeln!(
                        out,
                        "<article class=\"finding\">\n<h3>{} - {}</h3>",
                        escape(&finding.finding_ref),
                        escape(&finding.title)
                    );
                    for (label, value) in finding_fields(finding) {
                        if value.trim().is_empty() {
                            continue;
                        }
                        let _ = writeln!(out, "<h4>{}</h4>", escape(label));
                        out.push_str(&paragraphs(&value));
                    }
                    out.push_str("</article>\n");
                }
            }
            SectionKind::Evidence => {
                out.push_str(
                    "<table>\n<thead><tr><th>ID</th><th>Title</th><th>File</th>\
                     <th>SHA-256</th></tr></thead>\n<tbody>\n",
                );
                for evidence in &content.evidence {
                    let _ = writeln!(
                        out,
                        "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"hash\">{}</td></tr>",
                        escape(&evidence.evidence_ref),
                        escape(&evidence.title),
                        escape(&evidence.original_filename),
                        escape(&evidence.sha256)
                    );
                }
                out.push_str("</tbody>\n</table>\n");
            }
        }
        out.push_str("</section>\n");
    }

    let _ = writeln!(
        out,
        "<footer><p>{} - {} - {} - generated by AuditForge {}</p></footer>",
        escape(&content.report_ref),
        escape(&content.version_label),
        escape(&content.author.full_name),
        escape(crate::VERSION)
    );
    let _ = writeln!(
        out,
        "<div class=\"banner\">{}</div>",
        escape(content.classification.banner())
    );
    out.push_str("</body>\n</html>\n");
    out
}

fn paragraphs(text: &str) -> String {
    let mut out = String::new();
    for block in text.split("\n\n") {
        let trimmed = block.trim();
        if trimmed.is_empty() {
            continue;
        }
        let _ = writeln!(out, "<p>{}</p>", escape(trimmed).replace('\n', "<br>"));
    }
    out
}

/// Escape everything that could change the structure of the page. Report text is author
/// input and may legitimately contain payloads captured during testing.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

const HTML_STYLE: &str = r#"<style>
:root { color-scheme: light; }
* { box-sizing: border-box; }
body { margin: 0 auto; padding: 0 2rem 4rem; max-width: 52rem; background: #fff; color: #11161e;
  font: 15px/1.6 -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif; }
.banner { position: sticky; top: 0; margin: 0 -2rem; padding: .45rem 2rem; background: #114c7a;
  color: #fff; font-size: .72rem; font-weight: 700; letter-spacing: .16em; text-align: center; }
.cover { padding: 3rem 0 2rem; border-bottom: 1px solid #d6dbe2; }
.wordmark { margin: 0; font-size: .78rem; font-weight: 700; letter-spacing: .32em; color: #114c7a; }
.kind { margin: 1.6rem 0 .4rem; font-size: .8rem; font-weight: 700; letter-spacing: .14em;
  text-transform: uppercase; color: #586270; }
h1 { margin: 0 0 1.5rem; font-size: 2.1rem; line-height: 1.2; }
.meta { display: grid; grid-template-columns: 9.5rem 1fr; gap: .3rem 1rem; margin: 0; font-size: .92rem; }
.meta dt { font-size: .68rem; font-weight: 700; letter-spacing: .08em; text-transform: uppercase;
  color: #8c96a2; padding-top: .18rem; }
.meta dd { margin: 0; }
section { margin-top: 2.6rem; }
h2 { font-size: 1.28rem; color: #114c7a; border-bottom: 1px solid #d6dbe2; padding-bottom: .4rem; }
h2 .num { color: #8c96a2; margin-right: .35rem; }
h3 { font-size: .72rem; font-weight: 700; letter-spacing: .07em; text-transform: uppercase;
  color: #8c96a2; margin: 1.5rem 0 .3rem; }
.finding h3 { font-size: 1.02rem; letter-spacing: 0; text-transform: none; color: #11161e; }
h4 { font-size: .7rem; font-weight: 700; letter-spacing: .07em; text-transform: uppercase;
  color: #8c96a2; margin: 1rem 0 .2rem; }
p { margin: .4rem 0; }
p.gap { padding: .6rem .8rem; border-left: 3px solid #dec27a; background: #fcf7e8; }
table { width: 100%; border-collapse: collapse; margin: 1rem 0; font-size: .88rem; }
th { text-align: left; background: #f3f5f8; color: #586270; font-size: .72rem;
  letter-spacing: .05em; text-transform: uppercase; }
th, td { padding: .48rem .6rem; border-bottom: 1px solid #e4e8ee; vertical-align: top; }
td.hash { font-family: ui-monospace, "Cascadia Mono", Menlo, Consolas, monospace; font-size: .76rem;
  word-break: break-all; }
.finding { margin: 1.6rem 0; padding: 1.1rem 1.2rem; border: 1px solid #e4e8ee; border-radius: 8px; }
.sev { display: inline-block; padding: .1rem .5rem; border-radius: 999px; color: #fff;
  font-size: .72rem; font-weight: 700; }
.sev-critical { background: #9b1c28; } .sev-high { background: #c84a1e; }
.sev-medium { background: #b78914; } .sev-low { background: #2a6a8f; }
.sev-informational { background: #5e6876; }
footer { margin-top: 3rem; padding-top: 1rem; border-top: 1px solid #d6dbe2; color: #8c96a2;
  font-size: .8rem; }
@media print {
  body { max-width: none; padding: 0 1cm; font-size: 10.5pt; }
  .banner { position: static; }
  section { break-inside: auto; } .finding { break-inside: avoid; }
}
</style>
"#;

// ------------------------------------------------------------------ Markdown

pub fn to_markdown(content: &ReportContent, template: Option<&ReportTemplate>) -> String {
    let mut out = String::with_capacity(8 * 1024);
    let _ = writeln!(out, "# {}\n", content.title);
    let _ = writeln!(out, "> {}\n", content.classification.banner());
    let _ = writeln!(out, "| | |\n|---|---|");
    for (key, value) in [
        ("Prepared by", content.author.full_name.as_str()),
        ("Organization", content.author.organization.as_str()),
        ("Role", content.author.job_title.as_str()),
        ("Engagement", content.engagement.name.as_str()),
        ("Report ID", content.report_ref.as_str()),
        ("Version", content.version_label.as_str()),
    ] {
        if value.trim().is_empty() {
            continue;
        }
        let _ = writeln!(out, "| **{}** | {} |", key, escape_pipes(value));
    }
    let _ = writeln!(
        out,
        "| **Date** | {} |\n",
        content.generated_at.display_long()
    );

    let mut number = 0usize;
    for section in content.sections.iter().filter(|section| section.included) {
        let kind = template
            .and_then(|template| template.section(&section.key))
            .map(|section| section.kind)
            .unwrap_or(SectionKind::Guided);
        let has_body = match kind {
            SectionKind::Guided => section.has_content(),
            SectionKind::Findings => !content.findings.is_empty(),
            SectionKind::Evidence => !content.evidence.is_empty(),
        };
        if !has_body {
            continue;
        }
        number += 1;
        let _ = writeln!(out, "\n## {}. {}\n", number, section.title);

        match kind {
            SectionKind::Guided => {
                for answer in section.answers.iter().filter(|answer| !answer.is_blank()) {
                    let label = template
                        .and_then(|t| t.question(&section.key, &answer.question_key))
                        .map(|question| question.label.clone())
                        .unwrap_or_else(|| answer.question_key.clone());
                    let _ = writeln!(out, "**{label}**\n");
                    if answer.provenance == Provenance::MissingInformation {
                        let _ = writeln!(out, "> Information required. {}\n", answer.value.trim());
                    } else {
                        let _ = writeln!(out, "{}\n", answer.value.trim());
                    }
                }
            }
            SectionKind::Findings => {
                let _ = writeln!(
                    out,
                    "| ID | Finding | Severity | Affected |\n|---|---|---|---|"
                );
                for finding in &content.findings {
                    let _ = writeln!(
                        out,
                        "| {} | {} | {} | {} |",
                        finding.finding_ref,
                        escape_pipes(&finding.title),
                        finding.severity.title(),
                        escape_pipes(&finding.affected_asset)
                    );
                }
                for finding in &content.findings {
                    let _ = writeln!(out, "\n### {} - {}\n", finding.finding_ref, finding.title);
                    for (label, value) in finding_fields(finding) {
                        if value.trim().is_empty() {
                            continue;
                        }
                        let _ = writeln!(out, "**{}**\n\n{}\n", label, value.trim());
                    }
                }
            }
            SectionKind::Evidence => {
                let _ = writeln!(out, "| ID | Title | File | SHA-256 |\n|---|---|---|---|");
                for evidence in &content.evidence {
                    let _ = writeln!(
                        out,
                        "| {} | {} | {} | `{}` |",
                        evidence.evidence_ref,
                        escape_pipes(&evidence.title),
                        escape_pipes(&evidence.original_filename),
                        evidence.sha256
                    );
                }
            }
        }
    }

    let _ = writeln!(
        out,
        "\n---\n\n{} - {} - generated by AuditForge {}",
        content.report_ref,
        content.version_label,
        crate::VERSION
    );
    out
}

fn escape_pipes(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn finding_fields(finding: &FindingSnapshot) -> Vec<(&'static str, String)> {
    vec![
        ("Severity", finding.severity.title().to_string()),
        ("Affected asset", finding.affected_asset.clone()),
        ("Description", finding.description.clone()),
        ("Technical detail", finding.technical_details.clone()),
        ("Impact", finding.impact.clone()),
        ("Likelihood", finding.likelihood.clone()),
        ("Risk", finding.risk.clone()),
        ("Root cause", finding.root_cause.clone()),
        ("Remediation", finding.remediation.clone()),
        ("Recommendation", finding.recommendation.clone()),
        ("References", finding.reference_links.clone()),
    ]
}

// ------------------------------------------------------------------ CSV

/// Findings as CSV, RFC 4180 quoting.
pub fn findings_to_csv(findings: &[FindingSnapshot]) -> String {
    let headers = [
        "id",
        "title",
        "severity",
        "category",
        "affected_asset",
        "verification_status",
        "cvss_score",
        "cvss_vector",
        "description",
        "impact",
        "likelihood",
        "risk",
        "root_cause",
        "remediation",
        "recommendation",
        "references",
        "author",
        "evidence",
    ];
    let mut out = headers.join(",");
    out.push_str("\r\n");

    for finding in findings {
        let row = [
            finding.finding_ref.clone(),
            finding.title.clone(),
            finding.severity.title().to_string(),
            finding.category.clone(),
            finding.affected_asset.clone(),
            finding.verification_status.as_str().to_string(),
            finding
                .cvss_score
                .map(|score| format!("{score:.1}"))
                .unwrap_or_default(),
            finding.cvss_vector.clone(),
            finding.description.clone(),
            finding.impact.clone(),
            finding.likelihood.clone(),
            finding.risk.clone(),
            finding.root_cause.clone(),
            finding.remediation.clone(),
            finding.recommendation.clone(),
            finding.reference_links.clone(),
            finding.author_name.clone(),
            finding.evidence_refs.join(" "),
        ];
        out.push_str(
            &row.iter()
                .map(|field| csv_field(field))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    }
    out
}

/// Quote a CSV field.
///
/// A leading `=`, `+`, `-` or `@` is prefixed with an apostrophe: spreadsheet software
/// treats those as formulas, and a finding title beginning with `=` should not execute
/// when the file is opened.
fn csv_field(value: &str) -> String {
    let mut field = value.to_string();
    if field.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        field.insert(0, '\'');
    }
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field
    }
}

#[cfg(test)]
mod tests;
