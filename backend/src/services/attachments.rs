//! Attachment files live at `{ATTACHMENTS_DIR}/{card_id}/{attachment_id}_{filename}`;
//! `stored_path` holds that path relative to `ATTACHMENTS_DIR` and never leaves the server.

use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use axum::extract::multipart::{Field, MultipartError};
use axum::http::StatusCode;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use super::{activity, is_foreign_key_violation, now_ms};
use crate::config::Config;
use crate::errors::AppError;
use crate::models::{Attachment, Card};
use crate::AppState;

pub const ALLOWED_MIME_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "application/pdf",
    TEXT_PLAIN,
    TEXT_CSV,
    "application/zip",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
];
const TEXT_PLAIN: &str = "text/plain";
const TEXT_CSV: &str = "text/csv";
const OCTET_STREAM: &str = "application/octet-stream";
const CSV_EXTENSION: &str = ".csv";

/// Leading bytes inspected for type detection. `infer` scans up to three
/// 6000-byte windows into a ZIP to tell DOCX/XLSX apart from plain ZIP.
const SNIFF_BYTES: usize = 32 * 1024;
const BYTES_PER_MB: u64 = 1024 * 1024;
/// Leaves room for the `{uuid}_` prefix within the common 255-byte filename limit.
const MAX_FILENAME_BYTES: usize = 200;
const FALLBACK_FILENAME: &str = "file";

/// Types that can carry EXIF and are re-encoded on upload. TIFF is not in
/// `ALLOWED_MIME_TYPES` and needs the `image` crate's `tiff` feature to decode.
const EXIF_MIME_TYPES: &[&str] = &["image/jpeg", "image/tiff"];
/// High enough to be visually lossless for photos.
const JPEG_REENCODE_QUALITY: u8 = 90;
/// Caps decoder allocations (~5000x4000 RGB fits) to protect the RAM budget;
/// larger images fail to decode and are kept as uploaded.
const MAX_IMAGE_DECODE_BYTES: u64 = 64 * 1024 * 1024;
const EXIF_TEMP_SUFFIX: &str = ".exif-tmp";

const EMPTY_FILE_MESSAGE: &str = "file is empty";
const TYPE_NOT_ALLOWED_MESSAGE: &str = "file type is not allowed";
const INVALID_UPLOAD_MESSAGE: &str = "invalid multipart body";

pub struct Download {
    pub filename: String,
    pub content_type: &'static str,
    pub size_bytes: u64,
    pub file: tokio::fs::File,
}

pub fn max_upload_bytes(config: &Config) -> u64 {
    config.max_attachment_size_mb.saturating_mul(BYTES_PER_MB)
}

/// Maps a multipart read failure to a client error; an over-limit body is 413.
pub fn upload_error(err: MultipartError, config: &Config) -> AppError {
    if err.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return too_large(config);
    }
    tracing::debug!(error = %err, "multipart upload rejected");
    AppError::BadRequest(INVALID_UPLOAD_MESSAGE.to_string())
}

fn too_large(config: &Config) -> AppError {
    tracing::info!(
        max_attachment_size_mb = config.max_attachment_size_mb,
        "attachment rejected: exceeds maximum size"
    );
    AppError::PayloadTooLarge
}

/// Keeps only the final path component, drops control characters (including
/// NUL) and leading dots/whitespace, and caps the length.
pub fn sanitize_filename(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base.chars().filter(|c| !c.is_control()).collect();
    let trimmed = cleaned
        .trim_start_matches(|c: char| c == '.' || c.is_whitespace())
        .trim_end();

    let mut name = String::new();
    for c in trimmed.chars() {
        if name.len() + c.len_utf8() > MAX_FILENAME_BYTES {
            break;
        }
        name.push(c);
    }
    if name.is_empty() {
        FALLBACK_FILENAME.to_string()
    } else {
        name
    }
}

/// Detects the type from content (magic bytes) and returns it only if allowed.
/// Text has no signature: NUL-free UTF-8 is text, reported as CSV for `.csv` names.
/// `truncated` means `head` is a prefix of a longer file.
pub fn detect_mime(head: &[u8], filename: &str, truncated: bool) -> Option<&'static str> {
    if let Some(kind) = infer::get(head) {
        return ALLOWED_MIME_TYPES
            .iter()
            .copied()
            .find(|allowed| *allowed == kind.mime_type());
    }
    if !looks_like_text(head, truncated) {
        return None;
    }
    if filename.to_ascii_lowercase().ends_with(CSV_EXTENSION) {
        Some(TEXT_CSV)
    } else {
        Some(TEXT_PLAIN)
    }
}

fn looks_like_text(head: &[u8], truncated: bool) -> bool {
    if head.is_empty() || head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        // A multi-byte character may be cut off at the end of the sniffed prefix.
        Err(err) => truncated && err.error_len().is_none(),
    }
}

pub async fn get(state: &AppState, attachment_id: &str) -> Result<Attachment, AppError> {
    let attachment = sqlx::query_as!(
        Attachment,
        r#"SELECT id AS "id!", card_id, filename, stored_path, size_bytes, uploaded_by,
                  uploaded_at
           FROM attachments
           WHERE id = $1"#,
        attachment_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(attachment)
}

/// Streams `field` to disk, enforcing the size limit and allowed types, then
/// records the row. The file is removed if any step fails.
/// `filename` must already be sanitized.
pub async fn upload(
    state: &AppState,
    card: &Card,
    user_id: &str,
    filename: &str,
    field: Field<'_>,
) -> Result<Attachment, AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let stored_name = format!("{id}_{filename}");
    let stored_path = format!("{}/{stored_name}", card.id);
    let card_dir = state.config.attachments_dir.join(&card.id);
    tokio::fs::create_dir_all(&card_dir)
        .await
        .with_context(|| format!("failed to create attachment dir {}", card_dir.display()))?;
    let path = card_dir.join(&stored_name);

    let stored = store(
        state,
        card,
        user_id,
        &id,
        filename,
        &stored_path,
        &path,
        field,
    )
    .await;
    let attachment = match stored {
        Ok(attachment) => attachment,
        Err(err) => {
            remove_file(&path, &id).await;
            return Err(err);
        }
    };

    tracing::info!(
        attachment_id = %attachment.id,
        card_id = %card.id,
        %user_id,
        size_bytes = attachment.size_bytes,
        "attachment uploaded"
    );
    activity::record(
        state,
        &card.id,
        &card.board_id,
        user_id,
        activity::ADDED_ATTACHMENT,
        json!({ "attachment_id": attachment.id, "filename": attachment.filename }),
    )
    .await;
    Ok(attachment)
}

#[allow(clippy::too_many_arguments)]
async fn store(
    state: &AppState,
    card: &Card,
    user_id: &str,
    id: &str,
    filename: &str,
    stored_path: &str,
    path: &Path,
    mut field: Field<'_>,
) -> Result<Attachment, AppError> {
    let max_bytes = max_upload_bytes(&state.config);
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .with_context(|| format!("failed to create attachment file {}", path.display()))?;

    let mut size: u64 = 0;
    let mut head: Vec<u8> = Vec::with_capacity(SNIFF_BYTES);
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|err| upload_error(err, &state.config))?
    {
        size += chunk.len() as u64;
        if size > max_bytes {
            return Err(too_large(&state.config));
        }
        let wanted = SNIFF_BYTES.saturating_sub(head.len()).min(chunk.len());
        head.extend_from_slice(&chunk[..wanted]);
        file.write_all(&chunk)
            .await
            .context("failed to write attachment file")?;
    }
    file.flush()
        .await
        .context("failed to flush attachment file")?;

    drop(file);

    if size == 0 {
        return Err(AppError::BadRequest(EMPTY_FILE_MESSAGE.to_string()));
    }
    let truncated = size > head.len() as u64;
    let Some(mime) = detect_mime(&head, filename, truncated) else {
        tracing::info!(card_id = %card.id, %user_id, "attachment rejected: type not allowed");
        return Err(AppError::BadRequest(TYPE_NOT_ALLOWED_MESSAGE.to_string()));
    };
    let size = if EXIF_MIME_TYPES.contains(&mime) {
        strip_exif(path, id).await.unwrap_or(size)
    } else {
        size
    };

    let size_bytes = i64::try_from(size).context("attachment size overflows i64")?;
    let now = now_ms();
    let inserted = sqlx::query_as!(
        Attachment,
        r#"INSERT INTO attachments (id, card_id, filename, stored_path, size_bytes, uploaded_by,
                                   uploaded_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING id AS "id!", card_id AS "card_id!", filename AS "filename!",
                     stored_path AS "stored_path!", size_bytes AS "size_bytes!",
                     uploaded_by AS "uploaded_by!", uploaded_at AS "uploaded_at!""#,
        id,
        card.id,
        filename,
        stored_path,
        size_bytes,
        user_id,
        now
    )
    .fetch_one(&state.db)
    .await;

    match inserted {
        Ok(attachment) => Ok(attachment),
        // The card was deleted after the access check.
        Err(err) if is_foreign_key_violation(&err) => Err(AppError::NotFound),
        Err(err) => Err(err.into()),
    }
}

/// Re-encodes the image at `path` as a fresh JPEG, which drops EXIF (GPS, camera
/// serials, ...). Returns the new size, or `None` (original kept) on any failure.
async fn strip_exif(path: &Path, attachment_id: &str) -> Option<u64> {
    let source = path.to_path_buf();
    let reencoded = tokio::task::spawn_blocking(move || reencode_jpeg(&source)).await;
    let bytes = match reencoded {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(err)) => {
            tracing::error!(error = ?err, %attachment_id, "exif strip failed, keeping original");
            return None;
        }
        Err(err) => {
            tracing::error!(error = ?err, %attachment_id, "exif strip task failed, keeping original");
            return None;
        }
    };

    let mut temp_name = path.as_os_str().to_owned();
    temp_name.push(EXIF_TEMP_SUFFIX);
    let temp_path = PathBuf::from(temp_name);
    let replaced = async {
        tokio::fs::write(&temp_path, &bytes).await?;
        tokio::fs::rename(&temp_path, path).await
    }
    .await;
    if let Err(err) = replaced {
        tracing::error!(error = ?err, %attachment_id, "failed to replace image with exif-stripped copy, keeping original");
        remove_file(&temp_path, attachment_id).await;
        return None;
    }

    tracing::info!(%attachment_id, size_bytes = bytes.len(), "exif stripped from image attachment");
    Some(bytes.len() as u64)
}

/// CPU-heavy: call from `spawn_blocking`.
fn reencode_jpeg(path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut reader = ImageReader::open(path)
        .context("failed to open image")?
        .with_guessed_format()
        .context("failed to read image header")?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_IMAGE_DECODE_BYTES);
    reader.limits(limits);

    let mut decoder = reader.into_decoder().context("unsupported image")?;
    // EXIF orientation is lost with the metadata, so bake it into the pixels.
    let orientation = decoder
        .orientation()
        .context("failed to read orientation")?;
    let mut image = DynamicImage::from_decoder(decoder).context("failed to decode image")?;
    image.apply_orientation(orientation);
    let image = match image {
        DynamicImage::ImageLuma8(_) | DynamicImage::ImageRgb8(_) => image,
        other => DynamicImage::ImageRgb8(other.to_rgb8()),
    };

    let mut bytes = Vec::new();
    image
        .write_with_encoder(JpegEncoder::new_with_quality(
            &mut bytes,
            JPEG_REENCODE_QUALITY,
        ))
        .context("failed to encode jpeg")?;
    Ok(bytes)
}

/// Opens the file and re-detects its type from content for the response headers.
pub async fn open_for_download(
    state: &AppState,
    attachment: &Attachment,
) -> Result<Download, AppError> {
    let path = resolve(&state.config.attachments_dir, &attachment.stored_path)?;
    let mut file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            tracing::error!(attachment_id = %attachment.id, "attachment file missing on disk");
            return Err(AppError::NotFound);
        }
        Err(err) => {
            return Err(anyhow::Error::new(err)
                .context("failed to open attachment")
                .into())
        }
    };
    let size_bytes = file
        .metadata()
        .await
        .context("failed to stat attachment")?
        .len();

    let mut head = Vec::with_capacity(SNIFF_BYTES);
    (&mut file)
        .take(SNIFF_BYTES as u64)
        .read_to_end(&mut head)
        .await
        .context("failed to read attachment")?;
    file.rewind().await.context("failed to rewind attachment")?;

    let truncated = size_bytes > head.len() as u64;
    let content_type = detect_mime(&head, &attachment.filename, truncated).unwrap_or(OCTET_STREAM);
    Ok(Download {
        filename: attachment.filename.clone(),
        content_type,
        size_bytes,
        file,
    })
}

/// Deletes the row, then the file (best-effort), and logs `removed_attachment`.
pub async fn delete(
    state: &AppState,
    user_id: &str,
    card: &Card,
    attachment: &Attachment,
) -> Result<(), AppError> {
    let deleted = sqlx::query!("DELETE FROM attachments WHERE id = $1", attachment.id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tracing::info!(attachment_id = %attachment.id, card_id = %card.id, %user_id, "attachment deleted");

    match resolve(&state.config.attachments_dir, &attachment.stored_path) {
        Ok(path) => remove_file(&path, &attachment.id).await,
        Err(_) => {
            tracing::error!(attachment_id = %attachment.id, "attachment has an invalid stored path")
        }
    }
    activity::record(
        state,
        &card.id,
        &card.board_id,
        user_id,
        activity::REMOVED_ATTACHMENT,
        json!({ "attachment_id": attachment.id, "filename": attachment.filename }),
    )
    .await;
    Ok(())
}

/// Joins a stored relative path onto the attachments dir, refusing anything
/// that could escape it.
fn resolve(attachments_dir: &Path, stored_path: &str) -> Result<PathBuf, AppError> {
    let relative = Path::new(stored_path);
    if stored_path.is_empty()
        || !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(anyhow::anyhow!("attachment stored_path is not a plain relative path").into());
    }
    Ok(attachments_dir.join(relative))
}

async fn remove_file(path: &Path, attachment_id: &str) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => tracing::error!(
            error = ?err,
            %attachment_id,
            path = %path.display(),
            "failed to remove attachment file"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn sanitize_strips_paths_control_chars_and_leading_dots() {
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("C:\\Users\\me\\notes.txt"), "notes.txt");
        assert_eq!(sanitize_filename("a\0b\n.txt"), "ab.txt");
        assert_eq!(sanitize_filename("...hidden"), "hidden");
        assert_eq!(sanitize_filename(" . .env "), "env");
        assert_eq!(sanitize_filename(""), FALLBACK_FILENAME);
        assert_eq!(sanitize_filename("dir/"), FALLBACK_FILENAME);
        assert_eq!(sanitize_filename(".."), FALLBACK_FILENAME);
    }

    #[test]
    fn sanitize_caps_length_on_a_char_boundary() {
        let long = "é".repeat(MAX_FILENAME_BYTES);
        let name = sanitize_filename(&long);
        assert_eq!(name.len(), MAX_FILENAME_BYTES);
        assert!(name.chars().all(|c| c == 'é'));
    }

    #[test]
    fn detect_uses_content_not_name() {
        assert_eq!(
            detect_mime(PNG_MAGIC, "photo.txt", false),
            Some("image/png")
        );
        assert_eq!(
            detect_mime(b"%PDF-1.7\n", "a.png", false),
            Some("application/pdf")
        );
        assert_eq!(
            detect_mime(b"PK\x03\x04\x14\0\0\0", "a.zip", false),
            Some("application/zip")
        );
        assert_eq!(
            detect_mime(b"hello\nworld", "notes", false),
            Some(TEXT_PLAIN)
        );
        assert_eq!(
            detect_mime(b"a,b\n1,2\n", "DATA.CSV", false),
            Some(TEXT_CSV)
        );
    }

    #[test]
    fn detect_rejects_disallowed_or_binary_content() {
        assert_eq!(detect_mime(b"MZ\x90\0\x03\0\0\0", "a.pdf", false), None);
        assert_eq!(detect_mime(b"\x7fELF\x02\x01\x01\0", "a.txt", false), None);
        assert_eq!(
            detect_mime(b"<html><script>x</script></html>", "a.txt", false),
            None
        );
        assert_eq!(detect_mime(b"abc\0def", "a.txt", false), None);
        assert_eq!(detect_mime(b"\xff\xfe\xfd", "a.txt", false), None);
        assert_eq!(detect_mime(b"", "a.txt", false), None);
    }

    #[test]
    fn detect_tolerates_utf8_cut_at_sniff_boundary_only() {
        let cut = &"é".as_bytes()[..1];
        let head = [b"ok ".as_slice(), cut].concat();
        assert_eq!(detect_mime(&head, "a.txt", true), Some(TEXT_PLAIN));
        assert_eq!(detect_mime(&head, "a.txt", false), None);
    }

    const EXIF_MARKER: &[u8] = b"SECRET-GPS-49.0N-2.3E";

    /// A 4x2 JPEG with an APP1 EXIF segment (minimal TIFF header + marker bytes).
    fn jpeg_with_exif() -> Vec<u8> {
        let image =
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 2, image::Rgb([200, 10, 10])));
        let mut plain = Vec::new();
        image
            .write_with_encoder(JpegEncoder::new_with_quality(
                &mut plain,
                JPEG_REENCODE_QUALITY,
            ))
            .unwrap();

        let tiff_header: &[u8] = b"II*\0\x08\0\0\0\0\0\0\0\0\0";
        let body = [b"Exif\0\0".as_slice(), tiff_header, EXIF_MARKER].concat();
        let length = u16::try_from(body.len() + 2).unwrap().to_be_bytes();
        let soi = &plain[..2];
        [soi, &[0xFF, 0xE1], &length, &body, &plain[2..]].concat()
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    async fn temp_file(contents: &[u8]) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("zeroboard-exif-{}.jpg", uuid::Uuid::new_v4()));
        tokio::fs::write(&path, contents).await.unwrap();
        path
    }

    #[tokio::test]
    async fn strip_exif_reencodes_jpeg_without_metadata() {
        let original = jpeg_with_exif();
        assert!(contains(&original, EXIF_MARKER));
        assert_eq!(
            detect_mime(&original, "photo.jpg", false),
            Some("image/jpeg")
        );
        let path = temp_file(&original).await;

        let new_size = strip_exif(&path, "a1").await.expect("strip should succeed");

        let stripped = tokio::fs::read(&path).await.unwrap();
        assert_eq!(new_size, stripped.len() as u64);
        assert!(!contains(&stripped, b"Exif"));
        assert!(!contains(&stripped, EXIF_MARKER));
        let decoded = image::load_from_memory(&stripped).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (4, 2));
        let mut temp_name = path.as_os_str().to_owned();
        temp_name.push(EXIF_TEMP_SUFFIX);
        assert!(!PathBuf::from(temp_name).exists());
        tokio::fs::remove_file(&path).await.unwrap();
    }

    #[tokio::test]
    async fn strip_exif_keeps_original_when_decoding_fails() {
        let corrupt = b"\xFF\xD8\xFF\xE0 not really a jpeg".to_vec();
        assert_eq!(
            detect_mime(&corrupt, "photo.jpg", false),
            Some("image/jpeg")
        );
        let path = temp_file(&corrupt).await;

        assert_eq!(strip_exif(&path, "a1").await, None);
        assert_eq!(tokio::fs::read(&path).await.unwrap(), corrupt);
        tokio::fs::remove_file(&path).await.unwrap();
    }

    #[test]
    fn resolve_refuses_escaping_paths() {
        let dir = Path::new("/data/attachments");
        assert_eq!(
            resolve(dir, "card/file.txt").unwrap(),
            dir.join("card/file.txt")
        );
        assert!(resolve(dir, "../secret").is_err());
        assert!(resolve(dir, "/etc/passwd").is_err());
        assert!(resolve(dir, "").is_err());
    }
}
