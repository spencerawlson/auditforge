//! Page geometry, typography and the flow engine.
//!
//! Blocks are measured, then poured into pages. A block that does not fit is split where
//! splitting is meaningful - paragraphs at a line, tables at a row with the header
//! repeated - and moved whole where it is not, such as an image or a short heading that
//! would otherwise be stranded at the foot of a page.
//!
//! Running headers and footers are drawn after the flow completes, because "Page 3 of 11"
//! is not knowable until the last page exists.

use crate::models::{Classification, Severity};

use super::writer::{Content, Document, Font, Image, Rgb};

// ------------------------------------------------------------------ theme

pub struct Theme {
    pub ink: Rgb,
    pub muted: Rgb,
    pub faint: Rgb,
    pub rule: Rgb,
    pub accent: Rgb,
    pub band: Rgb,
    pub callout_background: Rgb,
    pub callout_border: Rgb,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            ink: (17, 22, 30),
            muted: (88, 98, 112),
            faint: (140, 150, 162),
            rule: (214, 219, 226),
            accent: (17, 76, 122),
            band: (243, 245, 248),
            callout_background: (252, 247, 232),
            callout_border: (222, 194, 122),
        }
    }
}

pub struct TypeScale {
    pub cover_title: f32,
    pub cover_subtitle: f32,
    pub h1: f32,
    pub h2: f32,
    pub h3: f32,
    pub body: f32,
    pub small: f32,
    pub tiny: f32,
    pub mono: f32,
}

impl Default for TypeScale {
    fn default() -> Self {
        TypeScale {
            cover_title: 26.0,
            cover_subtitle: 13.0,
            h1: 15.0,
            h2: 11.5,
            h3: 10.0,
            body: 9.8,
            small: 8.6,
            tiny: 7.4,
            mono: 8.4,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Geometry {
    pub width: f32,
    pub height: f32,
    pub margin_x: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    /// Space reserved above the text block for the running header.
    pub header_gap: f32,
    /// Space reserved below the text block for the running footer.
    pub footer_gap: f32,
}

impl Geometry {
    pub fn new(page_size: (f32, f32)) -> Self {
        Geometry {
            width: page_size.0,
            height: page_size.1,
            margin_x: 56.0,
            margin_top: 64.0,
            margin_bottom: 58.0,
            header_gap: 26.0,
            footer_gap: 24.0,
        }
    }

    pub fn content_width(&self) -> f32 {
        self.width - self.margin_x * 2.0
    }

    pub fn top(&self) -> f32 {
        self.height - self.margin_top
    }

    pub fn bottom(&self) -> f32 {
        self.margin_bottom
    }
}

// ------------------------------------------------------------------ blocks

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalloutTone {
    /// "Information required." and similar gaps the author still has to fill.
    Gap,
    /// Provenance and process notes.
    Note,
}

#[derive(Debug, Clone)]
pub enum Block {
    PageBreak,
    Space(f32),
    Rule,
    /// `level` 1 or 2; level 1 headings become table-of-contents entries.
    Heading {
        level: u8,
        number: String,
        text: String,
    },
    /// A question prompt above its answer.
    Label(String),
    Paragraph(String),
    Bullets(Vec<String>),
    KeyValues(Vec<(String, String)>),
    Monospace(String),
    Callout {
        tone: CalloutTone,
        text: String,
    },
    /// Column widths are fractions of the content width and must sum to 1.
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        widths: Vec<f32>,
        /// Column rendered as a severity chip.
        severity_column: Option<usize>,
    },
    Image {
        resource: String,
        pixel_width: u32,
        pixel_height: u32,
        caption: String,
    },
}

/// One entry in the generated table of contents.
#[derive(Debug, Clone)]
pub struct TocEntry {
    pub number: String,
    pub title: String,
    pub page: usize,
}

/// Everything the header and footer need.
pub struct RunningFurniture {
    pub classification: Classification,
    pub report_ref: String,
    pub version_label: String,
    pub author: String,
    pub organization: String,
    pub date: String,
    pub footer_note: String,
}

// ------------------------------------------------------------------ flow

pub struct Flow {
    pub geometry: Geometry,
    pub theme: Theme,
    pub scale: TypeScale,
    pages: Vec<Content>,
    cursor: f32,
    toc: Vec<TocEntry>,
    /// Pages before the flow starts (cover, then any table of contents).
    page_offset: usize,
}

impl Flow {
    pub fn new(geometry: Geometry, page_offset: usize) -> Self {
        let mut flow = Flow {
            geometry,
            theme: Theme::default(),
            scale: TypeScale::default(),
            pages: Vec::new(),
            cursor: 0.0,
            toc: Vec::new(),
            page_offset,
        };
        flow.begin_page();
        flow
    }

    pub fn toc_entries(&self) -> &[TocEntry] {
        &self.toc
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn begin_page(&mut self) {
        self.pages.push(Content::new());
        self.cursor = self.geometry.top() - self.geometry.header_gap;
    }

    fn page(&mut self) -> &mut Content {
        self.pages.last_mut().expect("a page always exists")
    }

    /// Lowest baseline the flow may use before starting a new page.
    fn floor(&self) -> f32 {
        self.geometry.bottom() + self.geometry.footer_gap
    }

    /// Current page number as the reader will see it.
    fn current_page_number(&self) -> usize {
        self.page_offset + self.pages.len()
    }

    fn ensure(&mut self, needed: f32) {
        if self.cursor - needed < self.floor() {
            self.begin_page();
        }
    }

    pub fn write(&mut self, blocks: &[Block]) {
        for block in blocks {
            self.write_block(block);
        }
    }

    fn write_block(&mut self, block: &Block) {
        match block {
            Block::PageBreak => {
                // Only break if something is already on the page; a break at the top of a
                // fresh page would leave a blank one.
                if self.cursor < self.geometry.top() - self.geometry.header_gap - 0.5 {
                    self.begin_page();
                }
            }
            Block::Space(height) => {
                self.cursor -= height;
                if self.cursor < self.floor() {
                    self.begin_page();
                }
            }
            Block::Rule => {
                self.ensure(10.0);
                let y = self.cursor - 4.0;
                let (x1, x2) = (
                    self.geometry.margin_x,
                    self.geometry.width - self.geometry.margin_x,
                );
                let color = self.theme.rule;
                self.page().line(x1, y, x2, y, 0.6, color);
                self.cursor = y - 8.0;
            }
            Block::Heading {
                level,
                number,
                text,
            } => self.heading(*level, number, text),
            Block::Label(text) => self.label(text),
            Block::Paragraph(text) => self.paragraph(text),
            Block::Bullets(items) => self.bullets(items),
            Block::KeyValues(pairs) => self.key_values(pairs),
            Block::Monospace(text) => self.monospace(text),
            Block::Callout { tone, text } => self.callout(*tone, text),
            Block::Table {
                headers,
                rows,
                widths,
                severity_column,
            } => self.table(headers, rows, widths, *severity_column),
            Block::Image {
                resource,
                pixel_width,
                pixel_height,
                caption,
            } => self.image(resource, *pixel_width, *pixel_height, caption),
        }
    }

    fn heading(&mut self, level: u8, number: &str, text: &str) {
        let (size, font, space_before, space_after) = if level == 1 {
            (self.scale.h1, Font::Bold, 18.0, 9.0)
        } else {
            (self.scale.h2, Font::Bold, 12.0, 5.0)
        };

        // Keep a heading with at least two lines of what follows it.
        let needed = space_before + size * 1.35 + space_after + self.scale.body * 2.4;
        self.ensure(needed);
        self.cursor -= space_before;

        let label = if number.is_empty() {
            text.to_string()
        } else {
            format!("{number}  {text}")
        };
        let x = self.geometry.margin_x;
        let baseline = self.cursor - size;
        let color = if level == 1 {
            self.theme.accent
        } else {
            self.theme.ink
        };
        self.page().text(font, size, x, baseline, color, &label);
        self.cursor = baseline - space_after;

        if level == 1 {
            let y = self.cursor + space_after - 4.0;
            let x2 = self.geometry.width - self.geometry.margin_x;
            let rule = self.theme.rule;
            self.page().line(x, y, x2, y, 0.6, rule);
            self.cursor -= 4.0;
            self.toc.push(TocEntry {
                number: number.to_string(),
                title: text.to_string(),
                page: self.current_page_number(),
            });
        }
    }

    fn label(&mut self, text: &str) {
        let size = self.scale.tiny;
        self.ensure(size * 1.6 + self.scale.body * 2.4);
        let baseline = self.cursor - size;
        let x = self.geometry.margin_x;
        let color = self.theme.faint;
        self.page().tracked_text(
            Font::Bold,
            size,
            0.4,
            (x, baseline),
            color,
            &text.to_uppercase(),
        );
        self.cursor = baseline - 5.0;
    }

    fn paragraph(&mut self, text: &str) {
        let size = self.scale.body;
        let leading = size * 1.42;
        for source_line in text.split('\n') {
            if source_line.trim().is_empty() {
                self.cursor -= leading * 0.5;
                continue;
            }
            let lines = wrap(
                Font::Regular,
                size,
                source_line.trim(),
                self.geometry.content_width(),
            );
            for line in lines {
                self.ensure(leading);
                let baseline = self.cursor - size;
                let x = self.geometry.margin_x;
                let color = self.theme.ink;
                self.page()
                    .text(Font::Regular, size, x, baseline, color, &line);
                self.cursor = baseline - (leading - size);
            }
        }
        self.cursor -= 4.0;
    }

    fn bullets(&mut self, items: &[String]) {
        let size = self.scale.body;
        let leading = size * 1.42;
        let indent = 14.0;
        for item in items {
            let lines = wrap(
                Font::Regular,
                size,
                item.trim(),
                self.geometry.content_width() - indent,
            );
            for (index, line) in lines.iter().enumerate() {
                self.ensure(leading);
                let baseline = self.cursor - size;
                let x = self.geometry.margin_x;
                let (ink, accent) = (self.theme.ink, self.theme.accent);
                if index == 0 {
                    self.page()
                        .text(Font::Regular, size, x, baseline, accent, "\u{2022}");
                }
                self.page()
                    .text(Font::Regular, size, x + indent, baseline, ink, line);
                self.cursor = baseline - (leading - size);
            }
        }
        self.cursor -= 4.0;
    }

    fn key_values(&mut self, pairs: &[(String, String)]) {
        let size = self.scale.body;
        let leading = size * 1.5;
        let key_width = (self.geometry.content_width() * 0.28).min(150.0);
        for (key, value) in pairs {
            let value_lines = wrap(
                Font::Regular,
                size,
                value,
                self.geometry.content_width() - key_width - 8.0,
            );
            let needed = leading * value_lines.len().max(1) as f32;
            self.ensure(needed);

            let baseline = self.cursor - size;
            let x = self.geometry.margin_x;
            let (muted, ink) = (self.theme.muted, self.theme.ink);
            self.page().text(Font::Bold, size, x, baseline, muted, key);
            for (index, line) in value_lines.iter().enumerate() {
                let y = baseline - leading * index as f32;
                self.page()
                    .text(Font::Regular, size, x + key_width + 8.0, y, ink, line);
            }
            self.cursor =
                baseline - leading * (value_lines.len().max(1) - 1) as f32 - (leading - size);
        }
        self.cursor -= 4.0;
    }

    fn monospace(&mut self, text: &str) {
        let size = self.scale.mono;
        let leading = size * 1.35;
        let padding = 8.0;
        for source_line in text.split('\n') {
            let lines = wrap(
                Font::Mono,
                size,
                source_line,
                self.geometry.content_width() - padding * 2.0,
            );
            for line in lines {
                self.ensure(leading);
                let baseline = self.cursor - size;
                let x = self.geometry.margin_x;
                let content_width = self.geometry.content_width();
                let (band, muted) = (self.theme.band, self.theme.ink);
                self.page()
                    .fill_rect(x, baseline - 3.0, content_width, leading, band);
                self.page()
                    .text(Font::Mono, size, x + padding, baseline, muted, &line);
                self.cursor = baseline - (leading - size);
            }
        }
        self.cursor -= 6.0;
    }

    fn callout(&mut self, tone: CalloutTone, text: &str) {
        let size = self.scale.small;
        let leading = size * 1.4;
        let padding = 8.0;
        let lines = wrap(
            Font::Regular,
            size,
            text,
            self.geometry.content_width() - padding * 2.0 - 4.0,
        );
        let height = leading * lines.len() as f32 + padding * 1.6;
        self.ensure(height + 6.0);

        let top = self.cursor;
        let x = self.geometry.margin_x;
        let width = self.geometry.content_width();
        let (background, border) = match tone {
            CalloutTone::Gap => (self.theme.callout_background, self.theme.callout_border),
            CalloutTone::Note => (self.theme.band, self.theme.rule),
        };
        self.page()
            .fill_rect(x, top - height, width, height, background);
        self.page().fill_rect(x, top - height, 2.5, height, border);

        let ink = self.theme.ink;
        for (index, line) in lines.iter().enumerate() {
            let baseline = top - padding - size - leading * index as f32;
            self.page()
                .text(Font::Regular, size, x + padding + 4.0, baseline, ink, line);
        }
        self.cursor = top - height - 8.0;
    }

    fn table(
        &mut self,
        headers: &[String],
        rows: &[Vec<String>],
        widths: &[f32],
        severity_column: Option<usize>,
    ) {
        let size = self.scale.small;
        let leading = size * 1.35;
        let cell_padding = 5.0;
        let content_width = self.geometry.content_width();
        let columns: Vec<f32> = widths
            .iter()
            .map(|fraction| fraction * content_width)
            .collect();

        let header_height = leading + cell_padding * 1.6;
        self.ensure(header_height + leading * 2.0);
        self.draw_table_header(headers, &columns, size, cell_padding, header_height);

        for row in rows {
            let wrapped: Vec<Vec<String>> = row
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    let width = columns.get(index).copied().unwrap_or(60.0);
                    wrap(Font::Regular, size, cell, width - cell_padding * 2.0)
                })
                .collect();
            let lines = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
            let row_height = leading * lines as f32 + cell_padding * 1.4;

            if self.cursor - row_height < self.floor() {
                self.begin_page();
                self.draw_table_header(headers, &columns, size, cell_padding, header_height);
            }

            let top = self.cursor;
            let mut x = self.geometry.margin_x;
            for (index, cell_lines) in wrapped.iter().enumerate() {
                let width = columns.get(index).copied().unwrap_or(60.0);
                if Some(index) == severity_column {
                    if let Some(text) = cell_lines.first() {
                        self.severity_chip(x + cell_padding, top - cell_padding - size, text);
                    }
                } else {
                    let ink = self.theme.ink;
                    for (line_index, line) in cell_lines.iter().enumerate() {
                        let baseline = top - cell_padding - size - leading * line_index as f32;
                        self.page().text(
                            Font::Regular,
                            size,
                            x + cell_padding,
                            baseline,
                            ink,
                            line,
                        );
                    }
                }
                x += width;
            }

            let rule = self.theme.rule;
            let (x1, x2) = (
                self.geometry.margin_x,
                self.geometry.margin_x + content_width,
            );
            self.page()
                .line(x1, top - row_height, x2, top - row_height, 0.4, rule);
            self.cursor = top - row_height;
        }
        self.cursor -= 8.0;
    }

    fn draw_table_header(
        &mut self,
        headers: &[String],
        columns: &[f32],
        size: f32,
        cell_padding: f32,
        header_height: f32,
    ) {
        let top = self.cursor;
        let content_width = self.geometry.content_width();
        let margin_x = self.geometry.margin_x;
        let (band, muted, rule) = (self.theme.band, self.theme.muted, self.theme.rule);
        self.page().fill_rect(
            margin_x,
            top - header_height,
            content_width,
            header_height,
            band,
        );
        let mut x = margin_x;
        for (index, header) in headers.iter().enumerate() {
            let baseline = top - cell_padding - size;
            self.page()
                .text(Font::Bold, size, x + cell_padding, baseline, muted, header);
            x += columns.get(index).copied().unwrap_or(60.0);
        }
        let (x1, x2) = (
            self.geometry.margin_x,
            self.geometry.margin_x + content_width,
        );
        self.page()
            .line(x1, top - header_height, x2, top - header_height, 0.7, rule);
        self.cursor = top - header_height;
    }

    fn severity_chip(&mut self, x: f32, baseline: f32, text: &str) {
        let size = self.scale.tiny;
        let color = text
            .parse::<Severity>()
            .map(|severity| severity.rgb())
            .unwrap_or_else(|_| {
                Severity::ALL
                    .iter()
                    .find(|severity| severity.title().eq_ignore_ascii_case(text))
                    .map(|severity| severity.rgb())
                    .unwrap_or(self.theme.muted)
            });
        let width = Font::Bold.text_width(text, size) + 10.0;
        self.page()
            .fill_rect(x, baseline - 2.5, width, size + 5.0, color);
        self.page()
            .text(Font::Bold, size, x + 5.0, baseline, (255, 255, 255), text);
    }

    fn image(&mut self, resource: &str, pixel_width: u32, pixel_height: u32, caption: &str) {
        let max_width = self.geometry.content_width();
        // Never taller than most of a text block, so a screenshot cannot push everything
        // else off the page.
        let max_height = (self.geometry.top() - self.floor()) * 0.62;
        let (mut width, mut height) = (pixel_width.max(1) as f32, pixel_height.max(1) as f32);
        let scale = (max_width / width).min(max_height / height).min(1.0);
        width *= scale;
        height *= scale;

        let caption_height = if caption.trim().is_empty() {
            0.0
        } else {
            self.scale.small * 1.8
        };
        self.ensure(height + caption_height + 10.0);

        let x = self.geometry.margin_x;
        let top = self.cursor;
        let rule = self.theme.rule;
        self.page().image(resource, x, top - height, width, height);
        self.page()
            .stroke_rect(x, top - height, width, height, 0.5, rule);
        self.cursor = top - height - 4.0;

        if caption_height > 0.0 {
            let size = self.scale.small;
            let baseline = self.cursor - size;
            let muted = self.theme.muted;
            self.page()
                .text(Font::Italic, size, x, baseline, muted, caption);
            self.cursor = baseline - 8.0;
        }
        self.cursor -= 6.0;
    }

    /// Draw the running furniture and hand the pages to the document.
    pub fn finish(self, document: &mut Document, furniture: &RunningFurniture, total_pages: usize) {
        let geometry = self.geometry;
        let theme = self.theme;
        let scale = self.scale;
        let offset = self.page_offset;

        for (index, mut content) in self.pages.into_iter().enumerate() {
            let page_number = offset + index + 1;
            draw_header(&mut content, &geometry, &theme, &scale, furniture);
            draw_footer(
                &mut content,
                &geometry,
                &theme,
                &scale,
                furniture,
                page_number,
                total_pages,
            );
            document.add_page(content);
        }
    }
}

pub fn draw_header(
    content: &mut Content,
    geometry: &Geometry,
    theme: &Theme,
    scale: &TypeScale,
    furniture: &RunningFurniture,
) {
    let y = geometry.height - geometry.margin_top + 16.0;
    let left = geometry.margin_x;
    let right = geometry.width - geometry.margin_x;

    content.text(
        Font::Bold,
        scale.tiny,
        left,
        y,
        theme.muted,
        &furniture.report_ref,
    );

    let banner = furniture.classification.banner();
    let banner_width = Font::Bold.text_width(banner, scale.tiny);
    content.text(
        Font::Bold,
        scale.tiny,
        (geometry.width - banner_width) / 2.0,
        y,
        theme.accent,
        banner,
    );

    let version_width = Font::Regular.text_width(&furniture.version_label, scale.tiny);
    content.text(
        Font::Regular,
        scale.tiny,
        right - version_width,
        y,
        theme.muted,
        &furniture.version_label,
    );

    content.line(left, y - 6.0, right, y - 6.0, 0.5, theme.rule);
}

pub fn draw_footer(
    content: &mut Content,
    geometry: &Geometry,
    theme: &Theme,
    scale: &TypeScale,
    furniture: &RunningFurniture,
    page_number: usize,
    total_pages: usize,
) {
    let y = geometry.margin_bottom - 18.0;
    let left = geometry.margin_x;
    let right = geometry.width - geometry.margin_x;

    content.line(left, y + 14.0, right, y + 14.0, 0.5, theme.rule);

    let attribution = if furniture.organization.trim().is_empty() {
        furniture.author.clone()
    } else {
        format!("{} - {}", furniture.author, furniture.organization)
    };
    content.text(
        Font::Regular,
        scale.tiny,
        left,
        y,
        theme.faint,
        &attribution,
    );

    let center = format!("Page {page_number} of {total_pages}");
    let center_width = Font::Regular.text_width(&center, scale.tiny);
    content.text(
        Font::Regular,
        scale.tiny,
        (geometry.width - center_width) / 2.0,
        y,
        theme.muted,
        &center,
    );

    let right_text = if furniture.footer_note.trim().is_empty() {
        furniture.date.clone()
    } else {
        format!("{} - {}", furniture.footer_note, furniture.date)
    };
    let right_width = Font::Regular.text_width(&right_text, scale.tiny);
    content.text(
        Font::Regular,
        scale.tiny,
        right - right_width,
        y,
        theme.faint,
        &right_text,
    );
}

/// Greedy line breaking. Words longer than the measure are split rather than allowed to
/// run into the margin, which is what happens with long URLs and base64 blobs.
pub fn wrap(font: Font, size: f32, text: &str, max_width: f32) -> Vec<String> {
    if max_width <= 0.0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if font.text_width(&candidate, size) <= max_width {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if font.text_width(word, size) <= max_width {
            current = word.to_string();
        } else {
            // The word is wider than the measure on its own. Emit every full chunk and
            // carry the remainder, so the next word can still join it.
            let mut chunks = split_long_word(font, size, word, max_width);
            let remainder = chunks.pop().unwrap_or_default();
            lines.extend(chunks);
            current = remainder;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn split_long_word(font: Font, size: f32, word: &str, max_width: f32) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in word.chars() {
        let candidate = format!("{current}{ch}");
        if font.text_width(&candidate, size) > max_width && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current.push(ch);
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Scale an image so the caller can reason about the space it will take.
pub fn image_display_size(image: &Image, max_width: f32, max_height: f32) -> (f32, f32) {
    let (width, height) = (image.width.max(1) as f32, image.height.max(1) as f32);
    let scale = (max_width / width).min(max_height / height).min(1.0);
    (width * scale, height * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> Geometry {
        Geometry::new((595.28, 841.89))
    }

    fn furniture() -> RunningFurniture {
        RunningFurniture {
            classification: Classification::Confidential,
            report_ref: "RPT-2026-0001".to_string(),
            version_label: "v1.0 Final".to_string(),
            author: "Spencer Rawlson Spady".to_string(),
            organization: "PLH Ecosystem, Inc.".to_string(),
            date: "2026-08-24".to_string(),
            footer_note: String::new(),
        }
    }

    #[test]
    fn wrapping_respects_the_measure() {
        let text = "The perimeter firewall permitted inbound traffic on several ports that the \
                    rules of engagement placed out of scope.";
        let lines = wrap(Font::Regular, 10.0, text, 200.0);
        assert!(lines.len() > 1);
        for line in &lines {
            assert!(
                Font::Regular.text_width(line, 10.0) <= 200.5,
                "line too wide: {line:?}"
            );
        }
        assert_eq!(
            lines.join(" "),
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }

    #[test]
    fn a_word_longer_than_the_line_is_split_rather_than_overflowing() {
        let blob = "A".repeat(400);
        let lines = wrap(Font::Regular, 10.0, &blob, 120.0);
        assert!(lines.len() > 1);
        for line in &lines {
            assert!(
                Font::Regular.text_width(line, 10.0) <= 120.5,
                "overflowing line"
            );
        }
        assert_eq!(lines.concat(), blob, "no characters may be lost");
    }

    #[test]
    fn empty_text_produces_one_empty_line() {
        assert_eq!(wrap(Font::Regular, 10.0, "", 100.0), vec![String::new()]);
        assert_eq!(wrap(Font::Regular, 10.0, "   ", 100.0), vec![String::new()]);
    }

    #[test]
    fn long_content_flows_onto_more_pages() {
        let mut flow = Flow::new(geometry(), 1);
        let paragraph =
            "Testing produced a number of observations that are recorded here. ".repeat(20);
        for index in 0..12 {
            flow.write(&[
                Block::Heading {
                    level: 1,
                    number: format!("{}.", index + 1),
                    text: format!("Section {}", index + 1),
                },
                Block::Paragraph(paragraph.clone()),
            ]);
        }
        assert!(
            flow.page_count() > 3,
            "expected several pages, got {}",
            flow.page_count()
        );
        assert_eq!(flow.toc_entries().len(), 12);
    }

    #[test]
    fn table_of_contents_page_numbers_increase_and_respect_the_offset() {
        let mut flow = Flow::new(geometry(), 2);
        for index in 0..8 {
            flow.write(&[
                Block::Heading {
                    level: 1,
                    number: format!("{}.", index + 1),
                    text: format!("Section {}", index + 1),
                },
                Block::Paragraph("Body text. ".repeat(160)),
            ]);
        }
        let entries = flow.toc_entries();
        assert_eq!(
            entries[0].page, 3,
            "the first body page follows the two front pages"
        );
        assert!(entries.windows(2).all(|pair| pair[0].page <= pair[1].page));
    }

    #[test]
    fn a_page_break_at_the_top_of_a_page_does_not_leave_a_blank_one() {
        let mut flow = Flow::new(geometry(), 0);
        flow.write(&[Block::PageBreak, Block::PageBreak]);
        assert_eq!(flow.page_count(), 1);

        flow.write(&[Block::Paragraph("Something".to_string()), Block::PageBreak]);
        assert_eq!(flow.page_count(), 2);
    }

    #[test]
    fn a_heading_is_not_stranded_at_the_foot_of_a_page() {
        let mut flow = Flow::new(geometry(), 0);
        // Fill nearly a whole page, then add a heading.
        flow.write(&[Block::Paragraph("Filler line. ".repeat(300))]);
        let before = flow.page_count();
        flow.write(&[Block::Heading {
            level: 1,
            number: "9.".to_string(),
            text: "Conclusion".to_string(),
        }]);
        let entry = flow.toc_entries().last().unwrap();
        assert_eq!(
            entry.page,
            flow.page_count(),
            "the heading records the page it actually landed on"
        );
        assert!(flow.page_count() >= before);
    }

    #[test]
    fn tables_split_across_pages_and_repeat_their_header() {
        let mut flow = Flow::new(geometry(), 0);
        let rows: Vec<Vec<String>> = (0..90)
            .map(|index| {
                vec![
                    format!("FND-2026-{index:04}"),
                    format!("Finding number {index} with a reasonably long title"),
                    "High".to_string(),
                ]
            })
            .collect();
        flow.write(&[Block::Table {
            headers: vec!["ID".into(), "Finding".into(), "Severity".into()],
            rows,
            widths: vec![0.22, 0.58, 0.2],
            severity_column: Some(2),
        }]);
        assert!(flow.page_count() > 1, "90 rows must not fit on one page");
    }

    #[test]
    fn every_block_kind_renders_without_panicking() {
        let mut flow = Flow::new(geometry(), 0);
        flow.write(&[
            Block::Heading {
                level: 1,
                number: "1.".into(),
                text: "Heading".into(),
            },
            Block::Heading {
                level: 2,
                number: String::new(),
                text: "Subheading".into(),
            },
            Block::Label("What happened?".into()),
            Block::Paragraph("Body text.\n\nWith a blank line.".into()),
            Block::Bullets(vec![
                "First".into(),
                "Second item that is quite a lot longer".into(),
            ]),
            Block::KeyValues(vec![("Author".into(), "Spencer Rawlson Spady".into())]),
            Block::Monospace("$ nmap -sV 10.0.0.0/24\nStarting Nmap".into()),
            Block::Callout {
                tone: CalloutTone::Gap,
                text: "Information required.".into(),
            },
            Block::Callout {
                tone: CalloutTone::Note,
                text: "Assisted by AI.".into(),
            },
            Block::Rule,
            Block::Space(12.0),
            Block::Table {
                headers: vec!["A".into(), "B".into()],
                rows: vec![vec!["1".into(), "2".into()]],
                widths: vec![0.5, 0.5],
                severity_column: None,
            },
        ]);
        assert!(flow.page_count() >= 1);
    }

    #[test]
    fn the_flow_hands_finished_pages_to_the_document() {
        let mut flow = Flow::new(geometry(), 1);
        flow.write(&[Block::Paragraph("Content. ".repeat(400))]);
        let pages = flow.page_count();

        let mut document = Document::new((595.28, 841.89));
        flow.finish(&mut document, &furniture(), pages + 1);
        assert_eq!(document.page_count(), pages);
        assert!(document.finish().is_ok());
    }

    #[test]
    fn image_scaling_never_enlarges_and_preserves_aspect() {
        let png = tiny_png(800, 400);
        let image = Image::decode(&png).unwrap();
        let (width, height) = image_display_size(&image, 480.0, 500.0);
        assert!((width / height - 2.0).abs() < 0.01);
        // Single-precision scaling can land a hair over the bound; a fraction of a point
        // is invisible, so allow it rather than pretending f32 is exact.
        assert!(width <= 480.01, "got {width}");

        let small = Image::decode(&tiny_png(40, 20)).unwrap();
        let (width, height) = image_display_size(&small, 480.0, 500.0);
        assert_eq!(
            (width, height),
            (40.0, 20.0),
            "small images must not be upscaled"
        );
    }

    fn tiny_png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&vec![200u8; (width * height * 3) as usize])
                .unwrap();
        }
        out
    }
}
