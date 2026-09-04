//! Image attachment loading for multimodal user turns.
//!
//! Converts local image files into `umf::chatml::ImageAttachment` sidecar
//! entries (MIME + base64 data + filename). MIME type is sniffed from the
//! file extension — no new dependencies — and image bytes are base64-encoded
//! for the OpenAI-compatible `data:` URL wire form.

use std::path::Path;

/// Supported image extensions and their MIME types (v1 scope).
const IMAGE_MIME_BY_EXT: &[(&str, &str)] = &[
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("png", "image/png"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
];

/// Sniff the MIME type of an image file from its extension.
///
/// Returns `None` for unsupported or missing extensions — callers surface a
/// clear validation error instead of guessing.
pub fn sniff_image_mime(path: &Path) -> Option<&'static str> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())?;
    IMAGE_MIME_BY_EXT
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, mime)| *mime)
}

/// Load image files into multimodal sidecar attachments.
///
/// Each path is read fully and base64-encoded. Fails with a descriptive
/// error on unsupported extensions or unreadable files so the run aborts
/// before any model call instead of silently degrading to text-only.
pub fn load_image_attachments(
    paths: &[std::path::PathBuf],
) -> Result<Vec<umf::chatml::ImageAttachment>, String> {
    use base64::Engine as _;

    let mut attachments = Vec::with_capacity(paths.len());
    for path in paths {
        let mime = sniff_image_mime(path).ok_or_else(|| {
            format!(
                "Unsupported image type for --attach '{}': expected one of {}",
                path.display(),
                IMAGE_MIME_BY_EXT
                    .iter()
                    .map(|(e, _)| format!(".{}", e))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

        let bytes = std::fs::read(path)
            .map_err(|e| format!("Failed to read attachment '{}': {}", path.display(), e))?;

        attachments.push(
            umf::chatml::ImageAttachment::new(
                mime,
                base64::engine::general_purpose::STANDARD.encode(&bytes),
            )
            .with_filename(
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("attachment"),
            ),
        );
    }
    Ok(attachments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use std::io::Write;

    #[test]
    fn test_sniff_image_mime_supported_extensions() {
        assert_eq!(sniff_image_mime(Path::new("a.jpg")), Some("image/jpeg"));
        assert_eq!(sniff_image_mime(Path::new("a.JPEG")), Some("image/jpeg"));
        assert_eq!(sniff_image_mime(Path::new("b.png")), Some("image/png"));
        assert_eq!(sniff_image_mime(Path::new("c.gif")), Some("image/gif"));
        assert_eq!(sniff_image_mime(Path::new("d.webp")), Some("image/webp"));
    }

    #[test]
    fn test_sniff_image_mime_rejects_unsupported() {
        assert_eq!(sniff_image_mime(Path::new("x.pdf")), None);
        assert_eq!(sniff_image_mime(Path::new("x.txt")), None);
        assert_eq!(sniff_image_mime(Path::new("noext")), None);
    }

    #[test]
    fn test_load_image_attachments_encodes_base64() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tiny.png");
        let mut f = std::fs::File::create(&file).unwrap();
        f.write_all(b"\x89PNG fake bytes").unwrap();

        let attachments = load_image_attachments(&[file.clone()]).unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].mime, "image/png");
        assert_eq!(attachments[0].filename.as_deref(), Some("tiny.png"));
        // "…PNG fake bytes" base64 — verify round-trip through data URL.
        let data_url = attachments[0].to_data_url();
        assert!(data_url.starts_with("data:image/png;base64,"));
        let encoded = data_url.trim_start_matches("data:image/png;base64,");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(decoded, b"\x89PNG fake bytes");
    }

    #[test]
    fn test_load_image_attachments_rejects_bad_extension_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("doc.pdf");
        std::fs::write(&bad, b"%PDF").unwrap();

        let err = load_image_attachments(&[bad.clone()]).unwrap_err();
        assert!(err.contains("Unsupported image type"), "got: {}", err);
        assert!(err.contains("doc.pdf"));

        let missing = dir.path().join("gone.png");
        let err = load_image_attachments(&[missing.clone()]).unwrap_err();
        assert!(err.contains("Failed to read attachment"), "got: {}", err);
        assert!(err.contains("gone.png"));
    }
}

/// Split raw trailing task values into `(task words, attachment paths)`.
///
/// The `task` argument is clap `trailing_var_arg`, so flags typed *after* the
/// task text arrive inside the task values. This recognizes repeatable
/// `--attach <path>` pairs (and `--attach=<path>`) anywhere in those values
/// and returns everything else as task text, preserving word order.
pub fn extract_attach_flags(
    values: Vec<String>,
) -> (Vec<String>, Vec<std::path::PathBuf>) {
    let mut task = Vec::new();
    let mut attachments = Vec::new();
    let mut iter = values.into_iter();
    while let Some(value) = iter.next() {
        if value == "--attach" {
            if let Some(path) = iter.next() {
                attachments.push(std::path::PathBuf::from(path));
            }
        } else if let Some(path) = value.strip_prefix("--attach=") {
            attachments.push(std::path::PathBuf::from(path));
        } else {
            task.push(value);
        }
    }
    (task, attachments)
}

#[cfg(test)]
mod attach_flag_tests {
    use super::*;

    #[test]
    fn test_extract_attach_flags_repeatable_pairs() {
        let (task, attachments) = extract_attach_flags(vec![
            "describe".into(),
            "this".into(),
            "--attach".into(),
            "a.jpg".into(),
            "--attach".into(),
            "b.png".into(),
        ]);
        assert_eq!(task, vec!["describe", "this"]);
        assert_eq!(
            attachments,
            vec![
                std::path::PathBuf::from("a.jpg"),
                std::path::PathBuf::from("b.png"),
            ]
        );
    }

    #[test]
    fn test_extract_attach_flags_equals_form_and_no_flags() {
        let (task, attachments) =
            extract_attach_flags(vec!["run".into(), "--attach=c.gif".into(), "check".into()]);
        assert_eq!(task, vec!["run", "check"]);
        assert_eq!(attachments, vec![std::path::PathBuf::from("c.gif")]);

        // No flags → task untouched, empty attachments.
        let (task, attachments) = extract_attach_flags(vec!["plain".into(), "text".into()]);
        assert_eq!(task, vec!["plain", "text"]);
        assert!(attachments.is_empty());
    }

    #[test]
    fn test_extract_attach_flags_dangling_flag_is_dropped() {
        // `--attach` with no following value: flag dropped, not treated as text.
        let (task, attachments) = extract_attach_flags(vec!["hi".into(), "--attach".into()]);
        assert_eq!(task, vec!["hi"]);
        assert!(attachments.is_empty());
    }
}
