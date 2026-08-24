//! Filename sanitizing and path containment.
//!
//! Two separate jobs, deliberately kept apart:
//!
//! * `sanitize_filename` turns an arbitrary, possibly hostile string into a name that is
//!   safe to create inside the evidence vault on any of the target platforms.
//! * `resolve_within` proves that a path really lands inside a root, after symlinks and
//!   `..` have been resolved - the check that actually stops traversal.

use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// Longest stored filename. Comfortably inside the 255-byte limit on ext4 and NTFS even
/// after the vault prefixes a reference id.
const MAX_FILENAME_LEN: usize = 120;
const MAX_EXTENSION_LEN: usize = 16;

/// Names Windows refuses to create, with or without an extension.
const RESERVED_STEMS: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Reduce any user-supplied name to a safe single path component.
///
/// The result never contains a separator, never starts with a dot, never resolves to a
/// parent directory and never collides with a Windows device name.
pub fn sanitize_filename(raw: &str) -> String {
    // Only the last component matters; "../../etc/passwd" becomes "passwd" before any
    // character filtering runs.
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("").trim();

    let mut cleaned = String::with_capacity(last.len());
    let mut previous = '\0';
    for ch in last.chars() {
        let mapped = if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ' ') {
            if ch == ' ' {
                '_'
            } else {
                ch
            }
        } else {
            '_'
        };
        // Collapse runs of underscores and of dots. Runs of dots are not dangerous on
        // their own - only a whole `..` path component is - but a name like `evil..log`
        // reads as an escape attempt to every reviewer who sees it, so remove the doubt.
        if (mapped == '_' || mapped == '.') && mapped == previous {
            continue;
        }
        previous = mapped;
        cleaned.push(mapped);
    }

    // Leading dots would create a hidden file; a name of only dots is meaningless.
    let cleaned = cleaned
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_string();
    if cleaned.is_empty() {
        return "evidence.bin".to_string();
    }

    let (stem, extension) = match cleaned.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= MAX_EXTENSION_LEN
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            (stem.to_string(), Some(ext.to_ascii_lowercase()))
        }
        _ => (cleaned.clone(), None),
    };

    let mut stem = stem;
    if RESERVED_STEMS.contains(&stem.to_ascii_lowercase().as_str()) {
        stem.insert(0, '_');
    }

    let budget = MAX_FILENAME_LEN - extension.as_ref().map_or(0, |ext| ext.len() + 1);
    if stem.len() > budget {
        stem.truncate(budget);
    }
    if stem.is_empty() {
        stem.push_str("evidence");
    }

    match extension {
        Some(ext) => format!("{stem}.{ext}"),
        None => stem,
    }
}

/// Join `relative` onto `root` and prove the result stays inside `root`.
///
/// The lexical check runs first so an absolute path or a `..` component is rejected
/// before it can be handed to the filesystem, then the resolved paths are compared so a
/// symlink planted inside the vault cannot point outward.
pub fn resolve_within(root: &Path, relative: &str) -> Result<PathBuf> {
    let candidate = Path::new(relative);
    if candidate.is_absolute() {
        return Err(Error::validation(
            "evidence paths must be relative to the vault",
        ));
    }
    for component in candidate.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(Error::validation("evidence path escapes the vault"))
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(Error::validation("evidence path must not be absolute"))
            }
        }
    }

    let joined = root.join(candidate);
    let resolved_root = root
        .canonicalize()
        .map_err(|e| Error::Io(format!("evidence vault is unavailable: {e}")))?;

    // The file may not exist yet (we are often resolving a destination), so canonicalize
    // the deepest existing ancestor and re-append the rest.
    let resolved = canonicalize_lenient(&joined)?;
    if !resolved.starts_with(&resolved_root) {
        return Err(Error::validation("evidence path escapes the vault"));
    }
    Ok(joined)
}

/// Canonicalize as much of `path` as exists, keeping the remaining components as-is.
fn canonicalize_lenient(path: &Path) -> Result<PathBuf> {
    if let Ok(resolved) = path.canonicalize() {
        return Ok(resolved);
    }
    let mut remainder: Vec<std::ffi::OsString> = Vec::new();
    let mut current = path.to_path_buf();
    loop {
        match current.parent() {
            Some(parent) => {
                if let Some(name) = current.file_name() {
                    remainder.push(name.to_os_string());
                }
                if let Ok(resolved) = parent.canonicalize() {
                    let mut out = resolved;
                    for name in remainder.iter().rev() {
                        out.push(name);
                    }
                    return Ok(out);
                }
                current = parent.to_path_buf();
            }
            None => return Err(Error::Io("could not resolve path".to_string())),
        }
    }
}

/// Validate a path the user picked for import.
///
/// Rejects anything that is not a regular file. Directories, devices, sockets and FIFOs
/// are refused rather than read: on Linux, reading `/dev/zero` as "evidence" would
/// otherwise fill the vault.
pub fn validate_import_source(path: &str, max_bytes: u64) -> Result<PathBuf> {
    let source = PathBuf::from(path);
    if !source.is_absolute() {
        return Err(Error::validation(
            "evidence must be imported from an absolute path",
        ));
    }
    let metadata =
        std::fs::metadata(&source).map_err(|_| Error::validation("that file could not be read"))?;
    if !metadata.is_file() {
        return Err(Error::validation(
            "only regular files can be imported as evidence",
        ));
    }
    if metadata.len() == 0 {
        return Err(Error::validation("that file is empty"));
    }
    if metadata.len() > max_bytes {
        return Err(Error::validation(format!(
            "file is larger than the {} MiB evidence limit",
            max_bytes / (1024 * 1024)
        )));
    }
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_attempts_are_reduced_to_a_plain_name() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(
            sanitize_filename("..\\..\\Windows\\System32\\config\\SAM"),
            "SAM"
        );
        assert_eq!(sanitize_filename("/etc/shadow"), "shadow");
        assert_eq!(sanitize_filename(".."), "evidence.bin");
        assert_eq!(sanitize_filename("."), "evidence.bin");
        assert_eq!(sanitize_filename(""), "evidence.bin");
    }

    #[test]
    fn hostile_characters_are_neutralized() {
        assert_eq!(sanitize_filename("scan;rm -rf.txt"), "scan_rm_-rf.txt");
        // Everything before the last separator is discarded, including the payload.
        assert_eq!(sanitize_filename("scan;rm -rf /.txt"), "txt");
        assert_eq!(sanitize_filename("nmap output.xml"), "nmap_output.xml");
        assert_eq!(sanitize_filename("shell$(id).log"), "shell_id_.log");
        assert!(!sanitize_filename("a\u{0000}b.txt").contains('\u{0000}'));
        assert!(!sanitize_filename("evil\n\rname.log").contains('\n'));
    }

    #[test]
    fn hidden_files_and_windows_device_names_are_defused() {
        assert_eq!(sanitize_filename(".bashrc"), "bashrc");
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("..evil..log"), "evil.log");
        assert_eq!(sanitize_filename("nul.txt"), "_nul.txt");
        assert_eq!(sanitize_filename("com1.log"), "_com1.log");
    }

    #[test]
    fn long_names_are_truncated_but_keep_their_extension() {
        let name = format!("{}.pcap", "a".repeat(400));
        let safe = sanitize_filename(&name);
        assert!(safe.len() <= MAX_FILENAME_LEN, "got {} chars", safe.len());
        assert!(safe.ends_with(".pcap"));
    }

    #[test]
    fn sanitized_names_are_always_a_single_component() {
        for hostile in [
            "../../../../root/.ssh/id_rsa",
            "C:\\Users\\victim\\file.txt",
            "sub/dir/file.log",
            "\u{202e}gnp.exe",
        ] {
            let safe = sanitize_filename(hostile);
            assert!(!safe.contains('/'), "{safe}");
            assert!(!safe.contains('\\'), "{safe}");
            assert_eq!(Path::new(&safe).components().count(), 1, "{safe}");
        }
    }

    #[test]
    fn resolve_within_accepts_children_and_rejects_escapes() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("2026")).unwrap();

        assert!(resolve_within(root.path(), "2026/EVD-0001_scan.txt").is_ok());
        assert!(resolve_within(root.path(), "../outside.txt").is_err());
        assert!(resolve_within(root.path(), "2026/../../outside.txt").is_err());
        assert!(resolve_within(root.path(), "/etc/passwd").is_err());
    }

    #[test]
    fn import_source_must_be_an_existing_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("evidence.log");
        std::fs::write(&file, b"hello").unwrap();

        assert!(validate_import_source(file.to_str().unwrap(), 1024).is_ok());
        assert!(validate_import_source(dir.path().to_str().unwrap(), 1024).is_err());
        assert!(validate_import_source("relative/path.txt", 1024).is_err());
        assert!(
            validate_import_source(dir.path().join("missing.txt").to_str().unwrap(), 1024).is_err()
        );
    }

    #[test]
    fn oversized_and_empty_imports_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.bin");
        std::fs::write(&big, vec![0u8; 2048]).unwrap();
        assert!(validate_import_source(big.to_str().unwrap(), 1024).is_err());

        let empty = dir.path().join("empty.bin");
        std::fs::write(&empty, b"").unwrap();
        assert!(validate_import_source(empty.to_str().unwrap(), 1024).is_err());
    }
}
