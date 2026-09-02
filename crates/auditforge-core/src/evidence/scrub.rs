//! Stripping metadata from imported images.
//!
//! A screenshot is not just pixels. Phone cameras write GPS coordinates into EXIF; some
//! capture tools write the machine name, the logged-in user, the software serial and a
//! full-resolution embedded thumbnail that survives cropping. Evidence attached to a
//! security report routinely leaves the tester's organisation, so any of that is a leak
//! the author never sees — the image looks identical.
//!
//! This removes metadata containers wholesale rather than trying to decide which fields
//! are sensitive. The pixels are what the evidence is; everything else is discarded.
//!
//! ## Why this runs before hashing
//!
//! `Evidence::sha256` is documented as the digest of the STORED bytes, and
//! [`super::verify`] re-hashes the vault copy and compares. Scrubbing after hashing would
//! break verification for every image. So the scrub happens first, `sha256` covers what
//! is actually in the vault, and the digest of the file as supplied is recorded
//! separately as `source_sha256` — the chain of custody stays complete, and the fact that
//! bytes were removed is disclosed rather than silent.
//!
//! ## What this is not
//!
//! It does not re-encode. Pixels are untouched, so it cannot be accused of altering the
//! image. It also cannot help with what is *in* the picture — a hostname in a title bar
//! is a redaction problem for a human, not a metadata problem.

/// Outcome of a scrub attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scrubbed {
    pub bytes: Vec<u8>,
    /// True when bytes were actually removed.
    pub changed: bool,
    /// Metadata containers that were dropped, for the audit entry.
    pub removed: Vec<String>,
}

impl Scrubbed {
    fn unchanged(bytes: Vec<u8>) -> Self {
        Scrubbed {
            bytes,
            changed: false,
            removed: Vec::new(),
        }
    }
}

/// Strip metadata from an image, by sniffed format. Anything else is returned untouched.
pub fn scrub_image(bytes: &[u8]) -> Scrubbed {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        scrub_jpeg(bytes)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        scrub_png(bytes)
    } else {
        Scrubbed::unchanged(bytes.to_vec())
    }
}

/// JPEG: drop every APPn application segment and every COM comment.
///
/// EXIF lives in APP1, XMP in APP1 with a different signature, Photoshop resource blocks
/// (which carry the IPTC block) in APP13, and colour-management profiles in APP2. APP0 is
/// the JFIF density header — kept, because some decoders expect it and it holds nothing
/// identifying.
fn scrub_jpeg(bytes: &[u8]) -> Scrubbed {
    let mut out = Vec::with_capacity(bytes.len());
    let mut removed = Vec::new();
    let mut i = 0usize;

    // SOI
    if bytes.len() < 2 {
        return Scrubbed::unchanged(bytes.to_vec());
    }
    out.extend_from_slice(&bytes[0..2]);
    i += 2;

    while i + 1 < bytes.len() {
        if bytes[i] != 0xFF {
            // Not a marker where one was expected: stop parsing and copy the remainder
            // verbatim rather than risk corrupting the file.
            out.extend_from_slice(&bytes[i..]);
            return finish(out, removed, bytes);
        }
        let marker = bytes[i + 1];

        // Start of scan: entropy-coded data follows to the end. Copy it all.
        if marker == 0xDA {
            out.extend_from_slice(&bytes[i..]);
            return finish(out, removed, bytes);
        }
        // Standalone markers carry no length payload.
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 || marker == 0xFF {
            out.extend_from_slice(&bytes[i..i + 2]);
            i += 2;
            continue;
        }
        if i + 3 >= bytes.len() {
            out.extend_from_slice(&bytes[i..]);
            return finish(out, removed, bytes);
        }
        let length = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        // A length shorter than its own field means a malformed file; bail out safely.
        if length < 2 || i + 2 + length > bytes.len() {
            out.extend_from_slice(&bytes[i..]);
            return finish(out, removed, bytes);
        }

        let is_appn_to_drop = (0xE1..=0xEF).contains(&marker); // APP1..APP15
        let is_comment = marker == 0xFE;

        if is_appn_to_drop || is_comment {
            removed.push(if is_comment {
                "JPEG COM".to_string()
            } else {
                format!("JPEG APP{}", marker - 0xE0)
            });
        } else {
            out.extend_from_slice(&bytes[i..i + 2 + length]);
        }
        i += 2 + length;
    }

    finish(out, removed, bytes)
}

/// PNG: drop text, timestamp and EXIF chunks, keep everything the image needs.
///
/// An allow-list would be safer still, but PNG is extensible and a private ancillary
/// chunk a decoder needs is possible; dropping the known metadata carriers by name keeps
/// unusual-but-harmless files working.
fn scrub_png(bytes: &[u8]) -> Scrubbed {
    const DROP: [&[u8; 4]; 6] = [b"tEXt", b"iTXt", b"zTXt", b"eXIf", b"tIME", b"dSIG"];

    let mut out = Vec::with_capacity(bytes.len());
    let mut removed = Vec::new();
    out.extend_from_slice(&bytes[0..8]); // signature
    let mut i = 8usize;

    while i + 8 <= bytes.len() {
        let length = u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        let kind = &bytes[i + 4..i + 8];
        let total = 12usize.saturating_add(length); // length + type + data + crc
        if i + total > bytes.len() {
            // Truncated chunk: copy the remainder and stop rather than fabricate one.
            out.extend_from_slice(&bytes[i..]);
            return finish(out, removed, bytes);
        }

        if DROP.iter().any(|d| d.as_slice() == kind) {
            removed.push(format!("PNG {}", String::from_utf8_lossy(kind)));
        } else {
            out.extend_from_slice(&bytes[i..i + total]);
        }

        i += total;
        if kind == b"IEND" {
            break;
        }
    }
    // Anything trailing IEND is not part of the image. Dropping it is deliberate: data
    // appended after IEND is a classic way to smuggle a payload past an image check.
    if i < bytes.len() {
        removed.push("PNG trailing data after IEND".to_string());
    }

    finish(out, removed, bytes)
}

fn finish(out: Vec<u8>, removed: Vec<String>, original: &[u8]) -> Scrubbed {
    let changed = out.as_slice() != original;
    Scrubbed {
        bytes: out,
        changed,
        removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal but structurally valid PNG: signature, IHDR, IDAT, IEND.
    fn png(extra: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut c = Vec::new();
            c.extend_from_slice(&(data.len() as u32).to_be_bytes());
            c.extend_from_slice(kind);
            c.extend_from_slice(data);
            c.extend_from_slice(&[0, 0, 0, 0]); // CRC, not validated here
            c
        }
        let mut v = Vec::from(b"\x89PNG\r\n\x1a\n".as_slice());
        v.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
        for (kind, data) in extra {
            v.extend(chunk(kind, data));
        }
        v.extend(chunk(b"IDAT", &[1, 2, 3]));
        v.extend(chunk(b"IEND", &[]));
        v
    }

    fn jpeg(segments: &[(u8, &[u8])]) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8]; // SOI
        for (marker, data) in segments {
            v.push(0xFF);
            v.push(*marker);
            let len = (data.len() + 2) as u16;
            v.extend_from_slice(&len.to_be_bytes());
            v.extend_from_slice(data);
        }
        v.push(0xFF);
        v.push(0xDA); // SOS
        v.extend_from_slice(b"scan-data-here");
        v
    }

    #[test]
    fn png_exif_and_text_chunks_are_removed() {
        let dirty = png(&[
            (b"eXIf", b"GPS 45.5017 -73.5673"),
            (b"tEXt", b"Author\0spencer@example.com"),
            (b"iTXt", b"Software\0\0\0\0CaptureTool 4.2"),
        ]);
        let out = scrub_image(&dirty);
        assert!(out.changed);
        assert!(!out.bytes.windows(4).any(|w| w == b"eXIf"));
        assert!(!out.bytes.windows(4).any(|w| w == b"tEXt"));
        assert!(!out.bytes.windows(4).any(|w| w == b"iTXt"));
        // The GPS string itself must be gone, not merely its chunk header.
        assert!(!contains(&out.bytes, b"45.5017"));
        assert!(!contains(&out.bytes, b"spencer@example.com"));
        assert_eq!(out.removed.len(), 3);
    }

    #[test]
    fn png_image_data_survives_untouched() {
        let dirty = png(&[(b"tEXt", b"Comment\0secret")]);
        let out = scrub_image(&dirty);
        assert!(out.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(out.bytes.windows(4).any(|w| w == b"IHDR"));
        assert!(out.bytes.windows(4).any(|w| w == b"IDAT"));
        assert!(out.bytes.windows(4).any(|w| w == b"IEND"));
    }

    #[test]
    fn a_clean_png_is_returned_byte_identical() {
        let clean = png(&[]);
        let out = scrub_image(&clean);
        assert!(!out.changed, "a clean file must not be rewritten");
        assert_eq!(out.bytes, clean);
        assert!(out.removed.is_empty());
    }

    #[test]
    fn data_appended_after_iend_is_dropped() {
        // Appending a payload after IEND is a standard way to smuggle bytes past a check
        // that only looks at the header.
        let mut dirty = png(&[]);
        dirty.extend_from_slice(b"PK\x03\x04smuggled-zip-payload");
        let out = scrub_image(&dirty);
        assert!(out.changed);
        assert!(!contains(&out.bytes, b"smuggled-zip-payload"));
        assert!(out.removed.iter().any(|r| r.contains("trailing data")));
    }

    #[test]
    fn jpeg_exif_app1_is_removed_and_the_scan_survives() {
        let dirty = jpeg(&[
            (0xE0, b"JFIF\0\x01\x02\0\0\x01\0\x01\0\0"),
            (0xE1, b"Exif\0\0GPS 45.5017 -73.5673 iPhone"),
            (0xED, b"Photoshop 3.0\0IPTC block"),
            (0xFE, b"comment: internal build machine WS-042"),
            (0xDB, b"quantization-table"),
        ]);
        let out = scrub_image(&dirty);
        assert!(out.changed);
        assert!(!contains(&out.bytes, b"45.5017"));
        assert!(!contains(&out.bytes, b"iPhone"));
        assert!(!contains(&out.bytes, b"IPTC block"));
        assert!(!contains(&out.bytes, b"WS-042"));
        // JFIF density header and the quantization table are structural — they stay.
        assert!(contains(&out.bytes, b"JFIF"));
        assert!(contains(&out.bytes, b"quantization-table"));
        // Everything from SOS onward is the actual picture.
        assert!(contains(&out.bytes, b"scan-data-here"));
        assert!(out.bytes.starts_with(&[0xFF, 0xD8]));
    }

    #[test]
    fn a_clean_jpeg_is_returned_byte_identical() {
        let clean = jpeg(&[(0xDB, b"quantization-table")]);
        let out = scrub_image(&clean);
        assert!(!out.changed);
        assert_eq!(out.bytes, clean);
    }

    #[test]
    fn a_non_image_is_never_touched() {
        let text = b"nmap -sV 10.10.10.3\nPORT STATE SERVICE".to_vec();
        let out = scrub_image(&text);
        assert!(!out.changed);
        assert_eq!(out.bytes, text);
    }

    #[test]
    fn malformed_images_are_returned_rather_than_rejected_or_corrupted() {
        // Import must not fail on a truncated file; the policy check and the size limit
        // are where files get refused, not here.
        for bad in [
            b"\x89PNG\r\n\x1a\n".to_vec(),                  // signature only
            b"\x89PNG\r\n\x1a\n\x00\x00\xff\xffIHDR".to_vec(), // length overruns the buffer
            vec![0xFF, 0xD8, 0xFF],                          // JPEG SOI then nothing
            vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x01],        // APP1 with an impossible length
        ] {
            let out = scrub_image(&bad);
            assert!(!out.bytes.is_empty(), "scrub must not empty a malformed file");
        }
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }
}
