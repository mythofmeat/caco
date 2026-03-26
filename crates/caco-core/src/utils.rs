use std::fs::File;
use std::io::Read;
use std::path::Path;

use md5::{Digest, Md5};
use regex::Regex;
use zip::ZipArchive;

/// Maximum size for a WAD inside a ZIP (256 MB).
const MAX_ZIP_ENTRY_SIZE: u64 = 256 * 1024 * 1024;

/// Parse WAD header and directory. Returns `[(name, offset, size), ...]`.
///
/// Accepts raw bytes. Handles both IWAD and PWAD files.
pub fn parse_wad_directory(wad_data: &[u8]) -> Vec<(String, u32, u32)> {
    if wad_data.len() < 12 {
        return Vec::new();
    }

    let magic = &wad_data[..4];
    if magic != b"IWAD" && magic != b"PWAD" {
        return Vec::new();
    }

    let num_lumps = i32::from_le_bytes(wad_data[4..8].try_into().unwrap()) as usize;
    let dir_offset = i32::from_le_bytes(wad_data[8..12].try_into().unwrap()) as usize;

    let mut entries = Vec::with_capacity(num_lumps);
    for i in 0..num_lumps {
        let entry_offset = dir_offset + i * 16;
        if entry_offset + 16 > wad_data.len() {
            break;
        }

        let lump_offset =
            u32::from_le_bytes(wad_data[entry_offset..entry_offset + 4].try_into().unwrap());
        let lump_size = u32::from_le_bytes(
            wad_data[entry_offset + 4..entry_offset + 8]
                .try_into()
                .unwrap(),
        );
        let name_bytes = &wad_data[entry_offset + 8..entry_offset + 16];
        let name = name_bytes
            .split(|&b| b == 0)
            .next()
            .unwrap_or(b"")
            .iter()
            .map(|&b| (b as char).to_ascii_uppercase())
            .collect::<String>();

        entries.push((name, lump_offset, lump_size));
    }

    entries
}

/// Compute MD5 hex digest of a file.
pub fn compute_md5(path: &Path) -> crate::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Md5::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Extract a 4-digit year from a date string (e.g. "2023-03-01").
pub fn extract_year(date_str: &str) -> Option<i32> {
    if date_str.len() < 4 {
        return None;
    }
    date_str[..4].parse().ok()
}

/// Format bytes as human-readable size (e.g., "12.3 MB").
pub fn format_size(size_bytes: u64) -> String {
    let mut value = size_bytes as f64;
    for unit in &["B", "KB", "MB", "GB"] {
        if value < 1024.0 {
            if *unit == "B" {
                return format!("{} {}", value as u64, unit);
            }
            return format!("{:.1} {}", value, unit);
        }
        value /= 1024.0;
    }
    format!("{:.1} TB", value)
}

/// Truncate text to `max_len`, appending suffix if truncated.
pub fn truncate(text: &str, max_len: usize, suffix: &str) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }
    let end = max_len.saturating_sub(suffix.len());
    // Find a valid char boundary
    let end = text.floor_char_boundary(end);
    format!("{}{}", &text[..end], suffix)
}

/// Sanitize a WAD title for use as a directory name.
///
/// Lowercase, replace non-alphanumeric with hyphens, strip leading/trailing
/// hyphens, collapse runs, and truncate to 64 chars.
pub fn sanitize_dirname(title: &str) -> String {
    let re = Regex::new(r"[^a-z0-9]+").unwrap();
    let name = title.to_lowercase();
    let name = re.replace_all(&name, "-");
    let name = name.trim_matches('-');
    let re2 = Regex::new(r"-{2,}").unwrap();
    let name = re2.replace_all(name, "-");
    let name = &name[..name.len().min(64)];
    name.to_string()
}

/// Load WAD data from a file, handling ZIP-wrapped WADs.
///
/// For `.zip` files or files with non-standard extensions, tries to extract
/// the first `.wad` entry from inside the archive. Falls back to reading
/// raw bytes.
pub fn load_wad_data(wad_path: &Path) -> Option<Vec<u8>> {
    if !wad_path.exists() {
        return None;
    }

    let ext = wad_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    // Try ZIP first for .zip or non-standard extensions
    if (ext == "zip" || !matches!(ext.as_str(), "wad" | "pk3" | "pk7"))
        && let Some(data) = try_read_wad_from_zip(wad_path) {
            return Some(data);
        }

    // Fall back to reading raw bytes
    std::fs::read(wad_path).ok()
}

/// Try to extract the first .wad entry from a ZIP archive.
fn try_read_wad_from_zip(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().to_lowercase();
        let size = entry.size();

        if name.ends_with(".wad") {
            if size > MAX_ZIP_ENTRY_SIZE {
                return None;
            }
            let mut buf = Vec::with_capacity(size as usize);
            if entry.read_to_end(&mut buf).is_ok() {
                return Some(buf);
            }
            return None;
        }
    }

    None
}

/// Render a rating as filled/empty star characters (e.g., "★★★☆☆").
pub fn format_rating(rating: Option<i32>, max_stars: usize) -> String {
    match rating {
        Some(r) if r > 0 => {
            let filled = r as usize;
            let empty = max_stars.saturating_sub(filled);
            "\u{2605}".repeat(filled) + &"\u{2606}".repeat(empty)
        }
        _ => String::new(),
    }
}

/// Format "Author (Year)" with graceful fallbacks.
pub fn format_author_year(author: Option<&str>, year: Option<i32>) -> String {
    match (author.filter(|a| !a.is_empty()), year) {
        (Some(a), Some(y)) => format!("{a} ({y})"),
        (Some(a), None) => a.to_string(),
        (None, Some(y)) => format!("({y})"),
        (None, None) => "Unknown author".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_year() {
        assert_eq!(extract_year("2023-03-01"), Some(2023));
        assert_eq!(extract_year("1994"), Some(1994));
        assert_eq!(extract_year("abc"), None);
        assert_eq!(extract_year(""), None);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1_048_576), "1.0 MB");
        assert_eq!(format_size(1_073_741_824), "1.0 GB");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10, "..."), "hello");
        assert_eq!(truncate("hello world", 8, "..."), "hello...");
    }

    #[test]
    fn test_sanitize_dirname() {
        assert_eq!(sanitize_dirname("Scythe 2"), "scythe-2");
        assert_eq!(sanitize_dirname("  Hello World!!  "), "hello-world");
        assert_eq!(sanitize_dirname("A---B"), "a-b");
    }

    #[test]
    fn test_format_rating() {
        assert_eq!(format_rating(Some(3), 5), "★★★☆☆");
        assert_eq!(format_rating(Some(5), 5), "★★★★★");
        assert_eq!(format_rating(None, 5), "");
        assert_eq!(format_rating(Some(0), 5), "");
    }

    #[test]
    fn test_format_author_year() {
        assert_eq!(format_author_year(Some("Romero"), Some(1994)), "Romero (1994)");
        assert_eq!(format_author_year(Some("Romero"), None), "Romero");
        assert_eq!(format_author_year(None, Some(1994)), "(1994)");
        assert_eq!(format_author_year(None, None), "Unknown author");
    }

    #[test]
    fn test_parse_wad_directory_invalid() {
        assert!(parse_wad_directory(b"").is_empty());
        assert!(parse_wad_directory(b"SHORT").is_empty());
        assert!(parse_wad_directory(b"NOTAWADFILE!").is_empty());
    }

    #[test]
    fn test_parse_wad_directory_minimal() {
        // Build a minimal valid WAD: header + 1 lump directory entry
        let mut wad = Vec::new();
        wad.extend_from_slice(b"PWAD"); // magic
        wad.extend_from_slice(&1_i32.to_le_bytes()); // num_lumps
        wad.extend_from_slice(&12_i32.to_le_bytes()); // dir_offset (right after header)
        // Directory entry: offset=0, size=0, name="TESTLUMP"
        wad.extend_from_slice(&0_u32.to_le_bytes());
        wad.extend_from_slice(&42_u32.to_le_bytes());
        wad.extend_from_slice(b"TESTLUMP");

        let entries = parse_wad_directory(&wad);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "TESTLUMP");
        assert_eq!(entries[0].1, 0);
        assert_eq!(entries[0].2, 42);
    }
}
