//! A minimal, self-contained PDF 1.7 writer.
//!
//! Written rather than pulled in so the document model, typography and page furniture are
//! entirely under this project's control, the output is deterministic, and generating a
//! report needs no system libraries on Kali or Ubuntu.
//!
//! Scope: the base-14 Type 1 fonts with WinAnsi encoding, filled and stroked rectangles,
//! lines, and image XObjects for PNG and JPEG evidence. Everything the report renderer
//! needs, and nothing else.

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::error::{Error, Result};

/// 1-based object number.
pub type ObjectId = usize;

/// Font slots exposed to content streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Mono,
}

impl Font {
    pub fn resource_name(&self) -> &'static str {
        match self {
            Font::Regular => "F1",
            Font::Bold => "F2",
            Font::Italic => "F3",
            Font::BoldItalic => "F4",
            Font::Mono => "F5",
        }
    }

    fn base_font(&self) -> &'static str {
        match self {
            Font::Regular => "Helvetica",
            Font::Bold => "Helvetica-Bold",
            Font::Italic => "Helvetica-Oblique",
            Font::BoldItalic => "Helvetica-BoldOblique",
            Font::Mono => "Courier",
        }
    }

    fn widths(&self) -> &'static [u16; 256] {
        match self {
            Font::Regular => &super::metrics::HELVETICA,
            Font::Bold => &super::metrics::HELVETICA_BOLD,
            Font::Italic => &super::metrics::HELVETICA_OBLIQUE,
            Font::BoldItalic => &super::metrics::HELVETICA_BOLD_OBLIQUE,
            Font::Mono => &super::metrics::COURIER,
        }
    }

    /// Width of `text` at `size` points.
    pub fn text_width(&self, text: &str, size: f32) -> f32 {
        let widths = self.widths();
        let total: u32 = encode_winansi(text)
            .iter()
            .map(|byte| u32::from(widths[*byte as usize]))
            .sum();
        (total as f32) * size / 1000.0
    }
}

pub type Rgb = (u8, u8, u8);

/// A page's drawing operations.
#[derive(Default)]
pub struct Content {
    ops: Vec<u8>,
}

impl Content {
    pub fn new() -> Self {
        Content::default()
    }

    /// Draw a single line of text with its baseline at `(x, y)`.
    pub fn text(&mut self, font: Font, size: f32, x: f32, y: f32, color: Rgb, text: &str) {
        if text.is_empty() {
            return;
        }
        self.set_fill(color);
        let _ = write!(
            self.ops,
            "BT /{} {} Tf {} {} Td ",
            font.resource_name(),
            fmt(size),
            fmt(x),
            fmt(y)
        );
        self.ops.push(b'(');
        self.ops
            .extend_from_slice(&escape_string(&encode_winansi(text)));
        self.ops.extend_from_slice(b") Tj ET\n");
    }

    /// Draw text with extra spacing between characters, used for small-caps-like labels.
    /// `at` is the baseline origin, taken as a pair to keep the parameter list short.
    pub fn tracked_text(
        &mut self,
        font: Font,
        size: f32,
        tracking: f32,
        at: (f32, f32),
        color: Rgb,
        text: &str,
    ) {
        let (x, y) = at;
        if text.is_empty() {
            return;
        }
        self.set_fill(color);
        let _ = write!(
            self.ops,
            "BT {} Tc /{} {} Tf {} {} Td ",
            fmt(tracking),
            font.resource_name(),
            fmt(size),
            fmt(x),
            fmt(y)
        );
        self.ops.push(b'(');
        self.ops
            .extend_from_slice(&escape_string(&encode_winansi(text)));
        self.ops.extend_from_slice(b") Tj 0 Tc ET\n");
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Rgb) {
        self.set_fill(color);
        let _ = writeln!(
            self.ops,
            "{} {} {} {} re f",
            fmt(x),
            fmt(y),
            fmt(width),
            fmt(height)
        );
    }

    pub fn stroke_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        line_width: f32,
        color: Rgb,
    ) {
        self.set_stroke(color);
        let _ = writeln!(
            self.ops,
            "{} w {} {} {} {} re S",
            fmt(line_width),
            fmt(x),
            fmt(y),
            fmt(width),
            fmt(height)
        );
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: Rgb) {
        self.set_stroke(color);
        let _ = writeln!(
            self.ops,
            "{} w {} {} m {} {} l S",
            fmt(width),
            fmt(x1),
            fmt(y1),
            fmt(x2),
            fmt(y2)
        );
    }

    /// Place a previously registered image with its lower-left corner at `(x, y)`.
    pub fn image(&mut self, name: &str, x: f32, y: f32, width: f32, height: f32) {
        let _ = writeln!(
            self.ops,
            "q {} 0 0 {} {} {} cm /{} Do Q",
            fmt(width),
            fmt(height),
            fmt(x),
            fmt(y),
            name
        );
    }

    fn set_fill(&mut self, (r, g, b): Rgb) {
        let _ = writeln!(
            self.ops,
            "{} {} {} rg",
            fmt(f32::from(r) / 255.0),
            fmt(f32::from(g) / 255.0),
            fmt(f32::from(b) / 255.0)
        );
    }

    fn set_stroke(&mut self, (r, g, b): Rgb) {
        let _ = writeln!(
            self.ops,
            "{} {} {} RG",
            fmt(f32::from(r) / 255.0),
            fmt(f32::from(g) / 255.0),
            fmt(f32::from(b) / 255.0)
        );
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// A decoded image ready to embed.
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Already-encoded stream data.
    data: Vec<u8>,
    filter: &'static str,
    color_space: &'static str,
}

impl Image {
    /// Decode PNG or JPEG bytes. Anything else is refused: the renderer only draws
    /// formats it can decode itself, so evidence is never handed to an external decoder.
    pub fn decode(bytes: &[u8]) -> Result<Image> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            return Image::from_png(bytes);
        }
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Image::from_jpeg(bytes);
        }
        Err(Error::validation(
            "only PNG and JPEG evidence can be embedded in a PDF",
        ))
    }

    fn from_png(bytes: &[u8]) -> Result<Image> {
        let decoder = png::Decoder::new(bytes);
        let mut reader = decoder
            .read_info()
            .map_err(|e| Error::validation(format!("that PNG could not be read: {e}")))?;
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader
            .next_frame(&mut buffer)
            .map_err(|e| Error::validation(format!("that PNG could not be decoded: {e}")))?;
        buffer.truncate(info.buffer_size());

        // Normalize to 8-bit RGB, compositing any alpha onto white so a transparent
        // screenshot does not render as a black rectangle.
        let rgb = match (info.color_type, info.bit_depth) {
            (png::ColorType::Rgb, png::BitDepth::Eight) => buffer,
            (png::ColorType::Rgba, png::BitDepth::Eight) => composite_on_white(&buffer, 4, 3),
            (png::ColorType::Grayscale, png::BitDepth::Eight) => buffer
                .iter()
                .flat_map(|value| [*value, *value, *value])
                .collect(),
            (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => {
                let gray = composite_on_white(&buffer, 2, 1);
                gray.iter()
                    .flat_map(|value| [*value, *value, *value])
                    .collect()
            }
            _ => {
                return Err(Error::validation(
                    "that PNG uses a colour format the report renderer cannot embed; \
                     re-save it as 8-bit RGB",
                ))
            }
        };

        Ok(Image {
            width: info.width,
            height: info.height,
            data: deflate(&rgb)?,
            filter: "FlateDecode",
            color_space: "DeviceRGB",
        })
    }

    /// JPEG data is passed through untouched: PDF understands DCTDecode natively, so
    /// there is no decode step and no re-encoding loss.
    fn from_jpeg(bytes: &[u8]) -> Result<Image> {
        let (width, height, components) = jpeg_dimensions(bytes)
            .ok_or_else(|| Error::validation("that JPEG has no readable frame header"))?;
        let color_space = match components {
            1 => "DeviceGray",
            3 => "DeviceRGB",
            _ => {
                return Err(Error::validation(
                    "CMYK JPEGs cannot be embedded; re-save the image as RGB",
                ))
            }
        };
        Ok(Image {
            width,
            height,
            data: bytes.to_vec(),
            filter: "DCTDecode",
            color_space,
        })
    }
}

fn composite_on_white(buffer: &[u8], stride: usize, color_bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(buffer.len() / stride * color_bytes);
    for pixel in buffer.chunks_exact(stride) {
        let alpha = f32::from(pixel[color_bytes]) / 255.0;
        for channel in &pixel[..color_bytes] {
            let value = f32::from(*channel) * alpha + 255.0 * (1.0 - alpha);
            out.push(value.round().clamp(0.0, 255.0) as u8);
        }
    }
    out
}

/// Read width, height and component count from a JPEG's start-of-frame marker.
fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    let mut index = 2; // skip SOI
    while index + 9 < bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }
        let marker = bytes[index + 1];
        // Standalone markers carry no length.
        if matches!(marker, 0xD8 | 0xD9 | 0x01) || (0xD0..=0xD7).contains(&marker) {
            index += 2;
            continue;
        }
        let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]) as usize;
        // SOF0..SOF15, excluding the DHT/JPG/DAC markers that share the range.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let height = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]);
            let width = u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]);
            let components = bytes[index + 9];
            if width == 0 || height == 0 {
                return None;
            }
            return Some((u32::from(width), u32::from(height), components));
        }
        index += 2 + length;
    }
    None
}

/// Assembles objects and serializes the file.
pub struct Document {
    objects: Vec<Option<Vec<u8>>>,
    page_size: (f32, f32),
    pages: Vec<PendingPage>,
    images: Vec<(String, ObjectId)>,
    title: String,
    author: String,
    subject: String,
    keywords: String,
    created_at: String,
}

struct PendingPage {
    content: Vec<u8>,
}

impl Document {
    pub fn new(page_size: (f32, f32)) -> Self {
        Document {
            objects: vec![None],
            page_size,
            pages: Vec::new(),
            images: Vec::new(),
            title: String::new(),
            author: String::new(),
            subject: String::new(),
            keywords: String::new(),
            created_at: String::new(),
        }
    }

    pub fn set_metadata(
        &mut self,
        title: &str,
        author: &str,
        subject: &str,
        keywords: &str,
        created_at: &str,
    ) {
        self.title = title.to_string();
        self.author = author.to_string();
        self.subject = subject.to_string();
        self.keywords = keywords.to_string();
        self.created_at = created_at.to_string();
    }

    fn allocate(&mut self, body: Vec<u8>) -> ObjectId {
        self.objects.push(Some(body));
        self.objects.len() - 1
    }

    /// Register an image and return the resource name to use in content streams.
    pub fn add_image(&mut self, image: &Image) -> String {
        let name = format!("Im{}", self.images.len() + 1);
        let mut body = Vec::new();
        let _ = write!(
            body,
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /{} \
             /BitsPerComponent 8 /Filter /{} /Length {} >>\nstream\n",
            image.width,
            image.height,
            image.color_space,
            image.filter,
            image.data.len()
        );
        body.extend_from_slice(&image.data);
        body.extend_from_slice(b"\nendstream");
        let id = self.allocate(body);
        self.images.push((name.clone(), id));
        name
    }

    pub fn add_page(&mut self, content: Content) {
        self.pages.push(PendingPage {
            content: content.ops,
        });
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Serialize the whole file.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        let fonts: Vec<(Font, ObjectId)> = [
            Font::Regular,
            Font::Bold,
            Font::Italic,
            Font::BoldItalic,
            Font::Mono,
        ]
        .into_iter()
        .map(|font| {
            let body = format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>",
                font.base_font()
            );
            let id = self.allocate(body.into_bytes());
            (font, id)
        })
        .collect();

        let font_resources = fonts
            .iter()
            .map(|(font, id)| format!("/{} {} 0 R", font.resource_name(), id))
            .collect::<Vec<_>>()
            .join(" ");
        let image_resources = self
            .images
            .iter()
            .map(|(name, id)| format!("/{name} {id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");

        // The pages node must be referenced by every page, so reserve its id first.
        self.objects.push(None);
        let pages_id = self.objects.len() - 1;

        let pending = std::mem::take(&mut self.pages);
        let mut page_ids = Vec::with_capacity(pending.len());
        for page in pending {
            let compressed = deflate(&page.content)?;
            let mut stream = Vec::new();
            let _ = write!(
                stream,
                "<< /Length {} /Filter /FlateDecode >>\nstream\n",
                compressed.len()
            );
            stream.extend_from_slice(&compressed);
            stream.extend_from_slice(b"\nendstream");
            let content_id = self.allocate(stream);

            let resources = if image_resources.is_empty() {
                format!("<< /Font << {font_resources} >> >>")
            } else {
                format!("<< /Font << {font_resources} >> /XObject << {image_resources} >> >>")
            };
            let body = format!(
                "<< /Type /Page /Parent {} 0 R /MediaBox [0 0 {} {}] /Resources {} \
                 /Contents {} 0 R >>",
                pages_id,
                fmt(self.page_size.0),
                fmt(self.page_size.1),
                resources,
                content_id
            );
            page_ids.push(self.allocate(body.into_bytes()));
        }

        let kids = page_ids
            .iter()
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        self.objects[pages_id] = Some(
            format!(
                "<< /Type /Pages /Count {} /Kids [{}] >>",
                page_ids.len(),
                kids
            )
            .into_bytes(),
        );

        let info_id = self.allocate(
            format!(
                "<< /Title ({}) /Author ({}) /Subject ({}) /Keywords ({}) \
                 /Creator (AuditForge {}) /Producer (AuditForge) /CreationDate ({}) >>",
                pdf_text(&self.title),
                pdf_text(&self.author),
                pdf_text(&self.subject),
                pdf_text(&self.keywords),
                crate::VERSION,
                pdf_text(&self.created_at),
            )
            .into_bytes(),
        );
        let catalog_id =
            self.allocate(format!("<< /Type /Catalog /Pages {pages_id} 0 R >>").into_bytes());

        Ok(self.serialize(catalog_id, info_id))
    }

    fn serialize(self, catalog_id: ObjectId, info_id: ObjectId) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 * 1024);
        out.extend_from_slice(b"%PDF-1.7\n");
        // Binary comment so tools treat the file as binary rather than text.
        out.extend_from_slice(&[b'%', 0xE2, 0xE3, 0xCF, 0xD3, b'\n']);

        let mut offsets = vec![0usize; self.objects.len()];
        for (index, object) in self.objects.iter().enumerate().skip(1) {
            let body = object.as_ref().map(Vec::as_slice).unwrap_or(b"null");
            offsets[index] = out.len();
            let _ = writeln!(out, "{index} 0 obj");
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }

        let xref_offset = out.len();
        let _ = write!(out, "xref\n0 {}\n", self.objects.len());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            // Exactly 20 bytes per entry, including the space before the line ending.
            let _ = writeln!(out, "{offset:010} 00000 n ");
        }

        let _ = write!(
            out,
            "trailer\n<< /Size {} /Root {} 0 R /Info {} 0 R >>\nstartxref\n{}\n%%EOF\n",
            self.objects.len(),
            catalog_id,
            info_id,
            xref_offset
        );
        out
    }
}

fn deflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .map_err(|e| Error::internal(format!("compression failed: {e}")))?;
    encoder
        .finish()
        .map_err(|e| Error::internal(format!("compression failed: {e}")))
}

/// Format a number without a trailing `.0`, and without scientific notation, which PDF
/// readers reject.
fn fmt(value: f32) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let rounded = (value * 1000.0).round() / 1000.0;
    if (rounded - rounded.trunc()).abs() < f32::EPSILON {
        format!("{}", rounded as i64)
    } else {
        let text = format!("{rounded:.3}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Map a Rust string to WinAnsi bytes, substituting characters the encoding lacks.
///
/// Report text is overwhelmingly Latin-1, and the substitutions cover the typographic
/// characters that word processors insert. Anything still unmapped becomes `?` rather than
/// silently vanishing, so a reader can see that something was dropped.
pub fn encode_winansi(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\n' | '\r' | '\t' => out.push(b' '),
            '\u{2018}' | '\u{2019}' | '\u{201B}' => out.push(b'\''),
            '\u{201C}' | '\u{201D}' | '\u{201F}' => out.push(b'"'),
            '\u{2013}' => out.push(b'-'),
            '\u{2014}' => {
                out.push(b'-');
                out.push(b'-');
            }
            '\u{2026}' => out.extend_from_slice(b"..."),
            '\u{00A0}' => out.push(b' '),
            '\u{2022}' => out.push(0x95),
            '\u{20AC}' => out.push(0x80),
            '\u{2122}' => out.push(0x99),
            c if (c as u32) < 0x20 => out.push(b' '),
            c if (c as u32) < 0x100 => out.push(c as u8),
            _ => out.push(b'?'),
        }
    }
    out
}

fn escape_string(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 8);
    for byte in bytes {
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(*byte);
    }
    out
}

fn pdf_text(text: &str) -> String {
    String::from_utf8_lossy(&escape_string(&encode_winansi(text))).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_document() -> Vec<u8> {
        let mut doc = Document::new((595.28, 841.89));
        doc.set_metadata(
            "Penetration Test Report",
            "Spencer Rawlson Spady",
            "CONFIDENTIAL",
            "auditforge",
            "D:20260824091500Z",
        );
        let mut content = Content::new();
        content.text(Font::Bold, 18.0, 56.0, 760.0, (20, 24, 31), "AuditForge");
        content.line(56.0, 750.0, 540.0, 750.0, 0.8, (200, 200, 200));
        doc.add_page(content);
        doc.finish().unwrap()
    }

    #[test]
    fn a_document_has_a_header_trailer_and_matching_object_count() {
        let bytes = simple_document();
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(b"%PDF-1.7"));
        assert!(text.ends_with("%%EOF\n"));
        assert!(text.contains("/Type /Catalog"));
        assert!(text.contains("/Type /Pages"));
        assert!(text.contains("/Type /Page "));
        assert!(text.contains("startxref"));

        // The xref table must list exactly as many entries as the /Size says.
        let size: usize = text
            .split("/Size ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|value| value.parse().ok())
            .expect("trailer must carry /Size");
        let xref_body = text.split("xref\n0 ").nth(1).expect("xref table");
        let declared: usize = xref_body.lines().next().unwrap().trim().parse().unwrap();
        assert_eq!(size, declared);
        let entries = xref_body
            .lines()
            .skip(1)
            .filter(|line| line.ends_with(" n ") || line.ends_with(" f "))
            .count();
        assert_eq!(entries, size);
    }

    #[test]
    fn the_startxref_offset_points_at_the_xref_keyword() {
        let bytes = simple_document();
        let text = String::from_utf8_lossy(&bytes);
        let offset: usize = text
            .rsplit("startxref\n")
            .next()
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|value| value.parse().ok())
            .expect("startxref must be present");
        assert!(
            bytes[offset..].starts_with(b"xref"),
            "startxref must point at the table"
        );
    }

    #[test]
    fn object_offsets_in_the_xref_land_on_their_objects() {
        let bytes = simple_document();
        let text = String::from_utf8_lossy(&bytes);
        let xref_body = text.split("xref\n0 ").nth(1).unwrap();
        for (index, line) in xref_body.lines().skip(1).enumerate().skip(1) {
            if !line.ends_with(" n ") {
                continue;
            }
            let offset: usize = line[..10].parse().unwrap();
            let expected = format!("{index} 0 obj");
            assert!(
                bytes[offset..].starts_with(expected.as_bytes()),
                "object {index} offset is wrong"
            );
        }
    }

    #[test]
    fn all_five_base_fonts_are_declared_with_winansi() {
        let text = String::from_utf8_lossy(&simple_document()).into_owned();
        for base in [
            "Helvetica",
            "Helvetica-Bold",
            "Helvetica-Oblique",
            "Helvetica-BoldOblique",
            "Courier",
        ] {
            assert!(
                text.contains(&format!("/BaseFont /{base}")),
                "missing {base}"
            );
        }
        assert!(text.contains("/Encoding /WinAnsiEncoding"));
    }

    #[test]
    fn text_measurement_matches_the_published_metrics() {
        // Helvetica: space 278, 'A' 667, at 1000pt those are the raw units.
        assert!((Font::Regular.text_width(" ", 1000.0) - 278.0).abs() < 0.01);
        assert!((Font::Regular.text_width("A", 1000.0) - 667.0).abs() < 0.01);
        assert!(Font::Bold.text_width("Hello", 12.0) > Font::Regular.text_width("Hello", 12.0));
        assert_eq!(Font::Regular.text_width("", 12.0), 0.0);
        // Courier is fixed pitch: every glyph is 600 units.
        assert!((Font::Mono.text_width("abcd", 10.0) - 24.0).abs() < 0.01);
    }

    #[test]
    fn parentheses_and_backslashes_in_text_are_escaped() {
        let mut doc = Document::new((595.0, 842.0));
        let mut content = Content::new();
        content.text(
            Font::Regular,
            10.0,
            10.0,
            10.0,
            (0, 0, 0),
            r"weird (name) \ here",
        );
        doc.add_page(content);
        // Content streams are compressed, so assert on the escape helper directly.
        let escaped = String::from_utf8(escape_string(&encode_winansi(r"a(b)c\d"))).unwrap();
        assert_eq!(escaped, r"a\(b\)c\\d");
        assert!(doc.finish().is_ok());
    }

    #[test]
    fn typographic_characters_degrade_to_winansi() {
        let encoded = encode_winansi("\u{201C}quoted\u{201D} \u{2014} caf\u{e9} \u{2026}");
        let text = String::from_utf8_lossy(&encoded);
        assert!(text.starts_with("\"quoted\""));
        assert!(text.contains("--"));
        assert!(text.contains("..."));
        assert_eq!(
            encoded[encoded.len() - 5],
            0xE9,
            "e-acute keeps its Latin-1 byte"
        );
    }

    #[test]
    fn characters_outside_winansi_become_a_visible_placeholder() {
        assert_eq!(encode_winansi("✅"), b"?");
        assert_eq!(encode_winansi("日本語"), b"???");
    }

    #[test]
    fn numbers_never_use_scientific_notation() {
        assert_eq!(fmt(0.0000001), "0");
        assert_eq!(fmt(12.0), "12");
        assert_eq!(fmt(12.5), "12.5");
        assert_eq!(fmt(841.89), "841.89");
        assert_eq!(fmt(f32::NAN), "0");
        assert!(!fmt(0.00001).contains('e'));
    }

    #[test]
    fn png_images_are_decoded_and_embedded() {
        let png = build_test_png();
        let image = Image::decode(&png).unwrap();
        assert_eq!(image.width, 2);
        assert_eq!(image.height, 2);

        let mut doc = Document::new((595.0, 842.0));
        let name = doc.add_image(&image);
        let mut content = Content::new();
        content.image(&name, 50.0, 50.0, 100.0, 100.0);
        doc.add_page(content);

        let text = String::from_utf8_lossy(&doc.finish().unwrap()).into_owned();
        assert!(text.contains("/Subtype /Image"));
        assert!(text.contains("/ColorSpace /DeviceRGB"));
        assert!(text.contains(&format!("/{name} ")));
    }

    #[test]
    fn unsupported_image_formats_are_refused_rather_than_guessed_at() {
        assert!(Image::decode(b"GIF89a not really").is_err());
        assert!(Image::decode(b"").is_err());
        assert!(Image::decode(b"%PDF-1.7").is_err());
    }

    #[test]
    fn jpeg_dimensions_are_read_from_the_frame_header() {
        // SOI, APP0 segment, SOF0 with height 0x0010 and width 0x0020, 3 components.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00,
            0x10, 0x00, 0x20, 0x03,
        ];
        assert_eq!(jpeg_dimensions(&jpeg), Some((32, 16, 3)));
        assert_eq!(jpeg_dimensions(&[0xFF, 0xD8]), None);
    }

    /// A 2x2 RGB PNG, built with the `png` encoder so the fixture is genuinely valid.
    fn build_test_png() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 2, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
                .unwrap();
        }
        out
    }

    #[test]
    fn transparent_pngs_composite_onto_white_not_black() {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            // Fully transparent black.
            writer.write_image_data(&[0, 0, 0, 0]).unwrap();
        }
        let image = Image::decode(&out).unwrap();
        assert_eq!(image.width, 1);
        assert_eq!(composite_on_white(&[0, 0, 0, 0], 4, 3), vec![255, 255, 255]);
        assert_eq!(composite_on_white(&[0, 0, 0, 255], 4, 3), vec![0, 0, 0]);
    }

    #[test]
    fn an_empty_document_still_serializes() {
        let doc = Document::new((595.0, 842.0));
        let bytes = doc.finish().unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        assert!(String::from_utf8_lossy(&bytes).contains("/Count 0"));
    }
}
