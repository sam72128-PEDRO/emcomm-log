//! 2.2 attachment helpers (ATT-*): content sniffing, the "never open programs" rule, and bounded thumbnails.
//! Thumbnails are decoded with size limits and re-encoded as small PNGs, so they never carry the
//! original's EXIF (camera, time, GPS position).

use crate::error::{AppError, AppResult, ErrorCode};
use std::io::Read;
use std::path::Path;

/// Suggested size above which the UI asks before copying (ATT-07). Not a limit.
pub const LARGE_WARNING_BYTES: u64 = 25 * 1024 * 1024;
/// Thumbnail edge, pixels.
pub const THUMB_EDGE: u32 = 320;
/// Images larger than this are not decoded for a thumbnail (decompression bombs, memory on 4 GB machines).
pub const MAX_DECODE_PIXELS: u64 = 40_000_000;
pub const MAX_DECODE_FILE: u64 = 64 * 1024 * 1024;

pub const SOURCES: &[&str] = &["file", "drag_drop", "paste", "camera", "photo_picker", "document_picker", "import"];

/// What the bytes are, from their first bytes (extension only as a fallback for plain text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sniffed {
    Jpeg,
    Png,
    Gif,
    Webp,
    Heic,
    Pdf,
    /// ZIP container (DOCX/XLSX/ODT…).
    Zip,
    /// Windows/macOS/Linux executable, or a script with a #! line.
    Program,
    Other,
}

impl Sniffed {
    pub fn mime(&self, name: &str) -> String {
        match self {
            Self::Jpeg => "image/jpeg".into(),
            Self::Png => "image/png".into(),
            Self::Gif => "image/gif".into(),
            Self::Webp => "image/webp".into(),
            Self::Heic => "image/heic".into(),
            Self::Pdf => "application/pdf".into(),
            Self::Zip => match ext(name).as_str() {
                "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document".into(),
                "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
                "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation".into(),
                "odt" => "application/vnd.oasis.opendocument.text".into(),
                _ => "application/zip".into(),
            },
            Self::Program => "application/octet-stream".into(),
            Self::Other => match ext(name).as_str() {
                "txt" | "log" => "text/plain".into(),
                "csv" => "text/csv".into(),
                "md" => "text/markdown".into(),
                "adi" | "adif" => "text/plain".into(),
                _ => "application/octet-stream".into(),
            },
        }
    }
    pub fn is_image(&self) -> bool {
        matches!(self, Self::Jpeg | Self::Png | Self::Gif | Self::Webp)
    }
}

fn ext(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn sniff_bytes(b: &[u8]) -> Sniffed {
    let starts = |p: &[u8]| b.len() >= p.len() && &b[..p.len()] == p;
    if starts(&[0xFF, 0xD8, 0xFF]) {
        Sniffed::Jpeg
    } else if starts(b"\x89PNG\r\n\x1a\n") {
        Sniffed::Png
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Sniffed::Gif
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Sniffed::Webp
    } else if b.len() >= 12 && &b[4..8] == b"ftyp" && matches!(&b[8..12], b"heic" | b"heix" | b"mif1" | b"msf1" | b"hevc") {
        Sniffed::Heic
    } else if starts(b"%PDF-") {
        Sniffed::Pdf
    } else if starts(b"PK\x03\x04") {
        Sniffed::Zip
    } else if starts(b"MZ")
        || starts(b"\x7fELF")
        || starts(&[0xCF, 0xFA, 0xED, 0xFE])
        || starts(&[0xCE, 0xFA, 0xED, 0xFE])
        || starts(&[0xCA, 0xFE, 0xBA, 0xBE])
        || starts(b"#!")
    {
        Sniffed::Program
    } else {
        Sniffed::Other
    }
}

pub fn sniff_file(p: &Path) -> AppResult<Sniffed> {
    let mut f = std::fs::File::open(p)?;
    let mut head = [0u8; 16];
    let n = f.read(&mut head)?;
    Ok(sniff_bytes(&head[..n]))
}

/// Names the OS would run or install when "opened" (ATT-06). Such attachments can be saved elsewhere but
/// EMCOMM LOG never opens them.
const RISKY_EXT: &[&str] = &[
    "exe", "com", "bat", "cmd", "scr", "msi", "msix", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "cpl", "jar", "lnk",
    "reg", "app", "pkg", "dmg", "command", "tool", "sh", "bash", "zsh", "csh", "run", "bin", "appimage", "deb", "rpm", "desktop", "py",
    "pl", "rb", "apk", "url", "webloc", "inf", "dll", "so", "dylib",
];

pub fn is_risky(name: &str, sniffed: Sniffed) -> bool {
    sniffed == Sniffed::Program || RISKY_EXT.contains(&ext(name).as_str())
}

/// A small PNG thumbnail of an image file, or None when it is not a decodable image or too large.
pub fn thumbnail_png(p: &Path) -> AppResult<Option<Vec<u8>>> {
    let s = sniff_file(p)?;
    if !s.is_image() {
        return Ok(None);
    }
    if std::fs::metadata(p)?.len() > MAX_DECODE_FILE {
        return Ok(None);
    }
    let mut reader = image::ImageReader::open(p)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let (w, h) = match reader.into_dimensions() {
        Ok(d) => d,
        Err(_) => return Ok(None),
    };
    if (w as u64) * (h as u64) > MAX_DECODE_PIXELS {
        return Ok(None);
    }
    let mut reader = image::ImageReader::open(p)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let img = match reader.decode() {
        Ok(i) => i,
        Err(_) => return Ok(None), // malformed image: show the file icon, never fail the entry
    };
    let thumb = img.thumbnail(THUMB_EDGE, THUMB_EDGE);
    let mut out = std::io::Cursor::new(Vec::new());
    thumb
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| AppError::new(ErrorCode::Io, format!("Could not make a thumbnail: {e}")))?;
    Ok(Some(out.into_inner()))
}

/// Clean a display name for an attachment (no path parts, no controls), keeping Unicode.
pub fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let s: String = base.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string();
    if s.is_empty() || s == "." || s == ".." { "attachment".into() } else { s.chars().take(200).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffing_and_risk() {
        assert_eq!(sniff_bytes(b"\xFF\xD8\xFF\xE0xx"), Sniffed::Jpeg);
        assert_eq!(sniff_bytes(b"%PDF-1.7"), Sniffed::Pdf);
        assert_eq!(sniff_bytes(b"MZ\x90\x00"), Sniffed::Program);
        assert_eq!(sniff_bytes(b"#!/bin/sh\n"), Sniffed::Program);
        assert!(is_risky("photo.jpg", Sniffed::Program), "content wins over a harmless name");
        assert!(is_risky("run.BAT", Sniffed::Other));
        assert!(!is_risky("report.pdf", Sniffed::Pdf));
        assert_eq!(Sniffed::Zip.mime("Plan.DOCX"), "application/vnd.openxmlformats-officedocument.wordprocessingml.document");
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name("C:\\Users\\me\\Fotos\\Mañana.jpg"), "Mañana.jpg");
        assert_eq!(clean_name("\u{0007}"), "attachment");
    }

    #[test]
    fn thumbnails_are_small_bounded_and_tolerate_bad_files() {
        let d = tempfile::tempdir().unwrap();
        let img = image::RgbImage::from_fn(1200, 800, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]));
        let p = d.path().join("big.png");
        img.save(&p).unwrap();
        let t = thumbnail_png(&p).unwrap().expect("thumbnail");
        let back = image::load_from_memory(&t).unwrap();
        assert!(back.width() <= THUMB_EDGE && back.height() <= THUMB_EDGE);
        let bad = d.path().join("broken.jpg");
        std::fs::write(&bad, b"\xFF\xD8\xFF\xE0 not really a jpeg").unwrap();
        assert!(thumbnail_png(&bad).unwrap().is_none(), "malformed image -> icon, not an error");
        let pdf = d.path().join("a.pdf");
        std::fs::write(&pdf, b"%PDF-1.4 ...").unwrap();
        assert!(thumbnail_png(&pdf).unwrap().is_none());
    }
}
