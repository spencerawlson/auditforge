//! Report rendering to PDF.
//!
//! Turns a stored [`ReportContent`] into a finished document: cover page, table of
//! contents with real page numbers, numbered sections built from the guided answers,
//! finding tables with severity chips, per-finding detail, an evidence appendix with
//! digests, and running headers and footers carrying the classification, report id,
//! version, author and page numbers.
//!
//! The author never formats anything. Section order, numbering, typography and
//! pagination all come from the template and the layout engine.

pub mod layout;
pub mod metrics;
pub mod writer;

use std::collections::BTreeMap;

use crate::error::Result;
use crate::models::{
    Classification, EvidenceSnapshot, Provenance, ReportContent, SectionContent, Timestamp,
};
use crate::settings::{PageSize, PdfSettings};
use crate::templates::{ReportTemplate, SectionKind};

use layout::{Block, CalloutTone, Flow, Geometry, RunningFurniture, Theme, TypeScale};
use writer::{Content, Document, Font, Image};

/// Text shown where an answer is missing. Matches the wording the AI layer is instructed
/// to use, so a gap reads the same however it got there.
pub const INFORMATION_REQUIRED: &str = "Information required.";

/// Table-of-contents rows that fit on one page. Used to predict how many pages the
/// contents will take before the body is laid out.
const TOC_ROWS_PER_PAGE: usize = 34;

#[derive(Default)]
pub struct RenderOptions {
    pub settings: PdfSettings,
    /// Rendered into the footer alongside the date.
    pub footer_note: String,
}

/// Supplies evidence bytes on demand. Returning `None` means the file could not be read;
/// the appendix then cites it without an image rather than failing the whole render.
pub type EvidenceLoader<'a> = dyn Fn(&EvidenceSnapshot) -> Option<Vec<u8>> + 'a;

/// Render `content` to PDF bytes.
pub fn render(
    content: &ReportContent,
    template: Option<&ReportTemplate>,
    options: &RenderOptions,
    load_evidence: &EvidenceLoader<'_>,
) -> Result<Vec<u8>> {
    let page_size = options.settings.page_size.dimensions();
    let geometry = Geometry::new(page_size);
    let mut document = Document::new(page_size);

    document.set_metadata(
        &content.title,
        &content.author.full_name,
        content.classification.banner(),
        &format!("{}, {}", content.report_ref, content.version_label),
        &pdf_date(content.generated_at),
    );

    // Images are registered once and referenced by both layout passes.
    let images = register_images(&mut document, content, options, load_evidence);
    let blocks = build_blocks(content, template, options, &images);

    // Pass one: lay the body out to discover the headings and how many pages it takes.
    let mut probe = Flow::new(geometry, 1);
    probe.write(&blocks);
    let toc_pages = if options.settings.include_table_of_contents && !probe.toc_entries().is_empty()
    {
        probe.toc_entries().len().div_ceil(TOC_ROWS_PER_PAGE)
    } else {
        0
    };

    // Pass two: the same body, now knowing which page number it starts on.
    let mut flow = Flow::new(geometry, 1 + toc_pages);
    flow.write(&blocks);
    let total_pages = 1 + toc_pages + flow.page_count();

    let furniture = RunningFurniture {
        classification: content.classification,
        report_ref: content.report_ref.clone(),
        version_label: content.version_label.clone(),
        author: content.author.full_name.clone(),
        organization: content.author.organization.clone(),
        date: content.generated_at.date_string(),
        footer_note: options.footer_note.clone(),
    };

    document.add_page(cover_page(content, &geometry, total_pages));
    if toc_pages > 0 {
        for page in toc_pages_content(flow.toc_entries(), &geometry, &furniture, total_pages) {
            document.add_page(page);
        }
    }
    flow.finish(&mut document, &furniture, total_pages);

    document.finish()
}

// ------------------------------------------------------------------ cover

fn cover_page(content: &ReportContent, geometry: &Geometry, total_pages: usize) -> Content {
    let theme = Theme::default();
    let scale = TypeScale::default();
    let mut page = Content::new();

    let left = geometry.margin_x;
    let right = geometry.width - geometry.margin_x;
    let width = geometry.content_width();

    // Classification band, top and bottom - the first and last thing a reader sees.
    classification_band(
        &mut page,
        geometry,
        content.classification,
        geometry.height - 34.0,
    );
    classification_band(&mut page, geometry, content.classification, 26.0);

    let mut y = geometry.height - 190.0;

    page.tracked_text(Font::Bold, 12.0, 3.2, (left, y), theme.accent, "AUDITFORGE");
    y -= 10.0;
    page.line(left, y, left + 64.0, y, 2.0, theme.accent);
    y -= 46.0;

    let type_title = content.report_type.title().to_uppercase();
    page.tracked_text(
        Font::Bold,
        scale.cover_subtitle,
        1.6,
        (left, y),
        theme.muted,
        &type_title,
    );
    y -= 40.0;

    for line in layout::wrap(Font::Bold, scale.cover_title, &content.title, width) {
        page.text(Font::Bold, scale.cover_title, left, y, theme.ink, &line);
        y -= scale.cover_title * 1.28;
    }

    y -= 30.0;
    page.line(left, y, right, y, 0.8, theme.rule);
    y -= 34.0;

    let mut details: Vec<(&str, String)> = vec![
        ("Prepared by", content.author.full_name.clone()),
        ("Organization", content.author.organization.clone()),
        ("Role", content.author.job_title.clone()),
    ];
    if !content.engagement.name.trim().is_empty() {
        details.push(("Engagement", content.engagement.name.clone()));
    }
    if !content.engagement.client_name.trim().is_empty() {
        details.push(("Client", content.engagement.client_name.clone()));
    }
    details.push(("Report ID", content.report_ref.clone()));
    details.push(("Version", content.version_label.clone()));
    details.push((
        "Classification",
        content.classification.banner().to_string(),
    ));
    details.push(("Date", content.generated_at.display_long()));
    if let (Some(start), Some(end)) = (&content.engagement.start_date, &content.engagement.end_date)
    {
        details.push(("Period", format!("{start} to {end}")));
    }

    let key_width = 132.0;
    for (key, value) in details {
        if value.trim().is_empty() {
            continue;
        }
        page.tracked_text(
            Font::Bold,
            scale.tiny,
            0.5,
            (left, y),
            theme.faint,
            &key.to_uppercase(),
        );
        for line in layout::wrap(Font::Regular, scale.body, &value, width - key_width) {
            page.text(
                Font::Regular,
                scale.body,
                left + key_width,
                y,
                theme.ink,
                &line,
            );
            y -= scale.body * 1.35;
        }
        y -= 8.0;
    }

    if !content.engagement.team.is_empty() {
        y -= 6.0;
        page.tracked_text(Font::Bold, scale.tiny, 0.5, (left, y), theme.faint, "TEAM");
        for member in &content.engagement.team {
            page.text(
                Font::Regular,
                scale.body,
                left + key_width,
                y,
                theme.ink,
                member,
            );
            y -= scale.body * 1.35;
        }
    }

    let note = format!(
        "This document contains {} pages. Generated by AuditForge {}.",
        total_pages,
        crate::VERSION
    );
    page.text(Font::Regular, scale.tiny, left, 58.0, theme.faint, &note);

    page
}

fn classification_band(
    page: &mut Content,
    geometry: &Geometry,
    classification: Classification,
    y: f32,
) {
    let theme = Theme::default();
    let banner = classification.banner();
    let size = 8.0;
    let text_width = Font::Bold.text_width(banner, size) + 24.0;
    let x = (geometry.width - text_width) / 2.0;
    let color = match classification {
        Classification::Restricted => (155, 28, 40),
        Classification::Confidential => theme.accent,
        Classification::Internal => (88, 98, 112),
        Classification::Public => (58, 122, 74),
    };
    page.fill_rect(x, y - 4.0, text_width, size + 9.0, color);
    page.text(Font::Bold, size, x + 12.0, y + 1.0, (255, 255, 255), banner);
}

// ------------------------------------------------------------------ contents

fn toc_pages_content(
    entries: &[layout::TocEntry],
    geometry: &Geometry,
    furniture: &RunningFurniture,
    total_pages: usize,
) -> Vec<Content> {
    let theme = Theme::default();
    let scale = TypeScale::default();
    let mut pages = Vec::new();

    for (page_index, chunk) in entries.chunks(TOC_ROWS_PER_PAGE).enumerate() {
        let mut page = Content::new();
        layout::draw_header(&mut page, geometry, &theme, &scale, furniture);
        layout::draw_footer(
            &mut page,
            geometry,
            &theme,
            &scale,
            furniture,
            2 + page_index,
            total_pages,
        );

        let left = geometry.margin_x;
        let right = geometry.width - geometry.margin_x;
        let mut y = geometry.top() - geometry.header_gap;

        if page_index == 0 {
            page.text(
                Font::Bold,
                scale.h1,
                left,
                y - scale.h1,
                theme.accent,
                "Contents",
            );
            y -= scale.h1 + 14.0;
            page.line(left, y, right, y, 0.6, theme.rule);
            y -= 20.0;
        }

        for entry in chunk {
            let label = if entry.number.is_empty() {
                entry.title.clone()
            } else {
                format!("{}  {}", entry.number, entry.title)
            };
            let page_text = entry.page.to_string();
            let page_width = Font::Regular.text_width(&page_text, scale.body);

            page.text(Font::Regular, scale.body, left, y, theme.ink, &label);
            page.text(
                Font::Regular,
                scale.body,
                right - page_width,
                y,
                theme.muted,
                &page_text,
            );

            // Dot leader between the title and the page number.
            let label_end = left + Font::Regular.text_width(&label, scale.body) + 6.0;
            let leader_end = right - page_width - 6.0;
            if leader_end > label_end {
                page.line(label_end, y + 2.0, leader_end, y + 2.0, 0.4, theme.rule);
            }
            y -= scale.body * 1.9;
        }

        pages.push(page);
    }
    pages
}

// ------------------------------------------------------------------ body

fn register_images(
    document: &mut Document,
    content: &ReportContent,
    options: &RenderOptions,
    load_evidence: &EvidenceLoader<'_>,
) -> BTreeMap<String, (String, u32, u32)> {
    let mut images = BTreeMap::new();
    if !options.settings.include_evidence_images {
        return images;
    }
    for evidence in &content.evidence {
        if !evidence.mime_type.starts_with("image/") {
            continue;
        }
        let Some(bytes) = load_evidence(evidence) else {
            continue;
        };
        match Image::decode(&bytes) {
            Ok(image) => {
                let (width, height) = (image.width, image.height);
                let resource = document.add_image(&image);
                images.insert(evidence.evidence_ref.clone(), (resource, width, height));
            }
            Err(error) => {
                tracing::info!(
                    evidence = %evidence.evidence_ref,
                    %error,
                    "evidence image could not be embedded; citing it without a preview"
                );
            }
        }
    }
    images
}

fn build_blocks(
    content: &ReportContent,
    template: Option<&ReportTemplate>,
    options: &RenderOptions,
    images: &BTreeMap<String, (String, u32, u32)>,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut number = 0usize;

    for section in content.sections.iter().filter(|section| section.included) {
        let kind = template
            .and_then(|template| template.section(&section.key))
            .map(|section| section.kind)
            .unwrap_or(SectionKind::Guided);

        // A section with nothing in it is omitted rather than printed empty; the
        // readiness check is where gaps are surfaced, not the finished document.
        let has_body = match kind {
            SectionKind::Guided => section.has_content(),
            SectionKind::Findings => !content.findings.is_empty(),
            SectionKind::Evidence => {
                options.settings.include_evidence_appendix && !content.evidence.is_empty()
            }
        };
        if !has_body {
            continue;
        }

        number += 1;
        blocks.push(Block::Heading {
            level: 1,
            number: format!("{number}."),
            text: section.title.clone(),
        });

        match kind {
            SectionKind::Guided => guided_section(&mut blocks, section, template, options),
            SectionKind::Findings => findings_section(&mut blocks, content),
            SectionKind::Evidence => evidence_section(&mut blocks, content, images),
        }
    }

    if let Some(note) = ai_assistance_note(content) {
        number += 1;
        blocks.push(Block::Heading {
            level: 1,
            number: format!("{number}."),
            text: "Statement of AI Assistance".to_string(),
        });
        blocks.push(Block::Paragraph(note));
    }

    blocks
}

fn guided_section(
    blocks: &mut Vec<Block>,
    section: &SectionContent,
    template: Option<&ReportTemplate>,
    options: &RenderOptions,
) {
    for answer in &section.answers {
        let label = template
            .and_then(|template| template.question(&section.key, &answer.question_key))
            .map(|question| question.label.clone())
            .unwrap_or_else(|| humanize(&answer.question_key));

        if answer.is_blank() {
            continue;
        }

        blocks.push(Block::Label(label));
        if answer.provenance == Provenance::MissingInformation {
            blocks.push(Block::Callout {
                tone: CalloutTone::Gap,
                text: format!("{INFORMATION_REQUIRED} {}", answer.value.trim()),
            });
            continue;
        }

        blocks.push(Block::Paragraph(answer.value.trim().to_string()));
        if options.settings.show_provenance_markers && answer.provenance != Provenance::UserProvided
        {
            blocks.push(Block::Callout {
                tone: CalloutTone::Note,
                text: format!("Provenance: {}", answer.provenance.label()),
            });
        }
    }
}

fn findings_section(blocks: &mut Vec<Block>, content: &ReportContent) {
    blocks.push(Block::Paragraph(format!(
        "{} finding{} {} recorded. Severity is assigned by the author.",
        content.findings.len(),
        if content.findings.len() == 1 { "" } else { "s" },
        if content.findings.len() == 1 {
            "is"
        } else {
            "are"
        },
    )));

    blocks.push(Block::Table {
        headers: vec![
            "ID".to_string(),
            "Finding".to_string(),
            "Severity".to_string(),
            "Affected".to_string(),
            "Status".to_string(),
        ],
        rows: content
            .findings
            .iter()
            .map(|finding| {
                vec![
                    finding.finding_ref.clone(),
                    finding.title.clone(),
                    finding.severity.title().to_string(),
                    finding.affected_asset.clone(),
                    humanize(finding.verification_status.as_str()),
                ]
            })
            .collect(),
        widths: vec![0.16, 0.36, 0.13, 0.22, 0.13],
        severity_column: Some(2),
    });

    for finding in &content.findings {
        blocks.push(Block::Heading {
            level: 2,
            number: String::new(),
            text: format!("{} - {}", finding.finding_ref, finding.title),
        });

        let mut facts = vec![
            ("Severity".to_string(), finding.severity.title().to_string()),
            (
                "Status".to_string(),
                humanize(finding.verification_status.as_str()),
            ),
        ];
        if !finding.affected_asset.trim().is_empty() {
            facts.push(("Affected asset".to_string(), finding.affected_asset.clone()));
        }
        if !finding.category.trim().is_empty() {
            facts.push(("Category".to_string(), finding.category.clone()));
        }
        if let Some(score) = finding.cvss_score {
            let vector = if finding.cvss_vector.trim().is_empty() {
                String::new()
            } else {
                format!(" ({})", finding.cvss_vector)
            };
            facts.push(("CVSS".to_string(), format!("{score:.1}{vector}")));
        }
        if !finding.evidence_refs.is_empty() {
            facts.push(("Evidence".to_string(), finding.evidence_refs.join(", ")));
        }
        blocks.push(Block::KeyValues(facts));

        for (label, value) in [
            ("Description", &finding.description),
            ("Technical detail", &finding.technical_details),
            ("Impact", &finding.impact),
            ("Likelihood", &finding.likelihood),
            ("Risk", &finding.risk),
            ("Root cause", &finding.root_cause),
            ("Remediation", &finding.remediation),
            ("Recommendation", &finding.recommendation),
        ] {
            if value.trim().is_empty() {
                continue;
            }
            blocks.push(Block::Label(label.to_string()));
            blocks.push(Block::Paragraph(value.trim().to_string()));
        }

        if !finding.reference_links.trim().is_empty() {
            blocks.push(Block::Label("References".to_string()));
            blocks.push(Block::Bullets(
                finding
                    .reference_links
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect(),
            ));
        }
    }
}

fn evidence_section(
    blocks: &mut Vec<Block>,
    content: &ReportContent,
    images: &BTreeMap<String, (String, u32, u32)>,
) {
    blocks.push(Block::Paragraph(
        "Each item below is stored in the evidence vault. The SHA-256 digest is of the file as \
         it was imported, so a reader can verify that the copy they hold is unmodified."
            .to_string(),
    ));

    blocks.push(Block::Table {
        headers: vec![
            "ID".to_string(),
            "Title".to_string(),
            "File".to_string(),
            "Size".to_string(),
        ],
        rows: content
            .evidence
            .iter()
            .map(|evidence| {
                vec![
                    evidence.evidence_ref.clone(),
                    evidence.title.clone(),
                    evidence.original_filename.clone(),
                    human_size(evidence.byte_size),
                ]
            })
            .collect(),
        widths: vec![0.18, 0.34, 0.34, 0.14],
        severity_column: None,
    });

    for evidence in &content.evidence {
        blocks.push(Block::Heading {
            level: 2,
            number: String::new(),
            text: format!("{} - {}", evidence.evidence_ref, evidence.title),
        });
        if !evidence.description.trim().is_empty() {
            blocks.push(Block::Paragraph(evidence.description.trim().to_string()));
        }
        let mut facts = vec![
            (
                "Original filename".to_string(),
                evidence.original_filename.clone(),
            ),
            ("Type".to_string(), evidence.mime_type.clone()),
            ("Size".to_string(), human_size(evidence.byte_size)),
        ];
        if let Some(captured_at) = evidence.captured_at {
            facts.push(("Captured".to_string(), captured_at.display_long()));
        }
        blocks.push(Block::KeyValues(facts));
        blocks.push(Block::Label("SHA-256".to_string()));
        blocks.push(Block::Monospace(evidence.sha256.clone()));

        if let Some((resource, width, height)) = images.get(&evidence.evidence_ref) {
            blocks.push(Block::Image {
                resource: resource.clone(),
                pixel_width: *width,
                pixel_height: *height,
                caption: format!("{} - {}", evidence.evidence_ref, evidence.title),
            });
        }
    }
}

/// A short statement of what in the document was machine-assisted, or `None` when none of
/// it was. Reporting this is part of the human-control principle: a reader can see where
/// an assistant helped with wording.
fn ai_assistance_note(content: &ReportContent) -> Option<String> {
    let mut wording = 0usize;
    let mut inference = 0usize;
    let mut gaps = 0usize;

    for section in content.sections.iter().filter(|section| section.included) {
        for answer in section.answers.iter().filter(|answer| !answer.is_blank()) {
            match answer.provenance {
                Provenance::AiWording => wording += 1,
                Provenance::AiInference => inference += 1,
                Provenance::MissingInformation => gaps += 1,
                Provenance::UserProvided => {}
            }
        }
    }
    for finding in &content.findings {
        for provenance in finding.provenance.values() {
            match provenance {
                Provenance::AiWording => wording += 1,
                Provenance::AiInference => inference += 1,
                Provenance::MissingInformation => gaps += 1,
                Provenance::UserProvided => {}
            }
        }
    }

    if wording == 0 && inference == 0 && gaps == 0 {
        return None;
    }

    let mut note = String::from(
        "An AI assistant was used to help with language in this report. All facts, findings, \
         severities and conclusions are the author's. ",
    );
    if wording > 0 {
        note.push_str(&format!(
            "{wording} passage{} were reworded by the assistant from the author's own material. ",
            if wording == 1 { "" } else { "s" }
        ));
    }
    if inference > 0 {
        note.push_str(&format!(
            "{inference} passage{} contain assistant-drafted prose that the author has reviewed. ",
            if inference == 1 { "" } else { "s" }
        ));
    }
    if gaps > 0 {
        note.push_str(&format!(
            "{gaps} item{} marked as requiring further information. ",
            if gaps == 1 { " is" } else { "s are" }
        ));
    }
    Some(note.trim_end().to_string())
}

fn humanize(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for (index, word) in key.split(['_', '-']).enumerate() {
        if word.is_empty() {
            continue;
        }
        if index > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

fn human_size(bytes: i64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes.max(0) as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", value as i64, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// PDF date syntax, e.g. `D:20260824091500Z`.
fn pdf_date(timestamp: Timestamp) -> String {
    format!("D:{}Z", timestamp.inner().format("%Y%m%d%H%M%S"))
}

/// Page size for a rendered document, exposed for callers that lay out previews.
pub fn page_dimensions(size: PageSize) -> (f32, f32) {
    size.dimensions()
}

#[cfg(test)]
mod tests;
