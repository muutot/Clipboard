use std::{fs, io::Read, path::Path, time::SystemTime};

pub const RESOURCE_METADATA_SCHEMA_VERSION: u8 = 2;

pub fn extension_for_path(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            extension
                .chars()
                .filter(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '_')
                })
                .take(32)
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|extension| !extension.is_empty())
}

pub fn mime_type_for_path(path: &Path) -> String {
    let extension = extension_for_path(path);
    let mut header = [0_u8; 512];
    let bytes_read = fs::File::open(path)
        .and_then(|mut file| file.read(&mut header))
        .unwrap_or(0);
    mime_type_from_bytes(&header[..bytes_read], extension.as_deref()).to_owned()
}

pub fn mime_type_from_bytes(bytes: &[u8], extension: Option<&str>) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return "image/jpeg";
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return "image/gif";
    }
    if bytes.starts_with(b"BM") {
        return "image/bmp";
    }
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return "image/tiff";
    }
    if bytes.starts_with(b"\0\0\x01\0") {
        return "image/x-icon";
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return "image/webp";
    }
    if bytes.starts_with(b"%PDF-") {
        return "application/pdf";
    }
    if bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
    {
        return "application/zip";
    }
    if bytes.starts_with(b"\x1f\x8b") {
        return "application/gzip";
    }
    if bytes.starts_with(b"7z\xbc\xaf'\x1c") {
        return "application/x-7z-compressed";
    }
    if bytes.starts_with(b"Rar!\x1a\x07") {
        return "application/vnd.rar";
    }
    if bytes.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") {
        return "application/x-ole-storage";
    }
    if bytes.starts_with(b"\x7fELF") {
        return "application/x-elf";
    }
    if bytes.starts_with(b"MZ") {
        return "application/vnd.microsoft.portable-executable";
    }
    if bytes.starts_with(b"SQLite format 3\0") {
        return "application/vnd.sqlite3";
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return match &bytes[8..12] {
            b"qt  " => "video/quicktime",
            b"M4A " | b"M4B " => "audio/mp4",
            b"heic" | b"heix" | b"hevc" | b"mif1" => "image/heif",
            b"avif" => "image/avif",
            _ => "video/mp4",
        };
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        return "audio/wav";
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"AVI " {
        return "video/x-msvideo";
    }
    if bytes.starts_with(b"fLaC") {
        return "audio/flac";
    }
    if bytes.starts_with(b"OggS") {
        return "application/ogg";
    }
    if bytes.starts_with(b"ID3")
        || (bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0)
    {
        return "audio/mpeg";
    }
    if bytes.starts_with(b"\x1aE\xdf\xa3") {
        return match extension {
            Some("webm") => "video/webm",
            _ => "video/x-matroska",
        };
    }

    let trimmed = bytes
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .collect::<Vec<_>>();
    let lower_prefix = trimmed
        .iter()
        .take(256)
        .map(u8::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if lower_prefix.starts_with(b"<svg")
        || (lower_prefix.starts_with(b"<?xml")
            && lower_prefix.windows(4).any(|window| window == b"<svg"))
    {
        return "image/svg+xml";
    }

    extension
        .and_then(mime_type_from_extension)
        .unwrap_or("application/octet-stream")
}

pub fn created_at_ms(metadata: &fs::Metadata) -> Option<i64> {
    metadata.created().ok().and_then(system_time_ms)
}

pub fn modified_at_ms(metadata: &fs::Metadata) -> Option<i64> {
    metadata.modified().ok().and_then(system_time_ms)
}

pub fn accessed_at_ms(metadata: &fs::Metadata) -> Option<i64> {
    metadata.accessed().ok().and_then(system_time_ms)
}

fn system_time_ms(value: SystemTime) -> Option<i64> {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
}

fn mime_type_from_extension(extension: &str) -> Option<&'static str> {
    Some(match extension.to_ascii_lowercase().as_str() {
        "txt" | "log" | "ini" | "cfg" | "conf" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "xml" => "application/xml",
        "json" | "map" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "js" | "mjs" | "cjs" => "text/javascript",
        "ts" | "tsx" => "text/typescript",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" | "jpe" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "heic" | "heif" => "image/heif",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "gz" | "gzip" => "application/gzip",
        "tar" => "application/x-tar",
        "bz2" => "application/x-bzip2",
        "xz" => "application/x-xz",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "rtf" => "application/rtf",
        "exe" | "dll" => "application/vnd.microsoft.portable-executable",
        "msi" => "application/x-msi",
        "wasm" => "application/wasm",
        "sqlite" | "sqlite3" | "db" => "application/vnd.sqlite3",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" | "oga" => "audio/ogg",
        "m4a" | "aac" => "audio/mp4",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_signature_wins_over_extension() {
        assert_eq!(
            mime_type_from_bytes(b"\x89PNG\r\n\x1a\nrest", Some("txt")),
            "image/png"
        );
        assert_eq!(
            mime_type_from_bytes(b"%PDF-1.7", Some("png")),
            "application/pdf"
        );
    }

    #[test]
    fn extension_is_used_only_when_signature_is_unknown() {
        assert_eq!(
            mime_type_from_bytes(b"plain content", Some("json")),
            "application/json"
        );
        assert_eq!(
            mime_type_from_bytes(b"plain content", Some("unknown")),
            "application/octet-stream"
        );
    }

    #[test]
    fn distinguishes_common_iso_base_media_brands() {
        let mut header = [0_u8; 12];
        header[4..8].copy_from_slice(b"ftyp");
        header[8..12].copy_from_slice(b"heic");
        assert_eq!(mime_type_from_bytes(&header, Some("mp4")), "image/heif");

        header[8..12].copy_from_slice(b"qt  ");
        assert_eq!(
            mime_type_from_bytes(&header, Some("mp4")),
            "video/quicktime"
        );
    }

    #[test]
    fn extracts_normalized_safe_extension() {
        assert_eq!(
            extension_for_path(Path::new("Archive.TAR.GZ")),
            Some("gz".to_owned())
        );
        assert_eq!(extension_for_path(Path::new("README")), None);
    }
}
