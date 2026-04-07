use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use chrono::{TimeZone, Utc};
use md5::{Digest, Md5};
use regex::Regex;
use zip::ZipArchive;

static NON_ALNUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());
static MULTI_HYPHEN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-{2,}").unwrap());
static ZIP_MAP_DOOM1_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^E[1-9]M[0-9]$").unwrap());
static ZIP_MAP_DOOM2_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^MAP[0-9][0-9]$").unwrap());

/// Maximum size for a WAD inside a ZIP (1 GiB).
const MAX_ZIP_ENTRY_SIZE: u64 = 1024 * 1024 * 1024;

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

/// Convert a [`std::time::SystemTime`] to an RFC 3339 string.
///
/// Returns an empty string if the conversion fails.
pub fn system_time_to_rfc3339(time: std::time::SystemTime) -> String {
    time.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| Utc.timestamp_opt(d.as_secs() as i64, 0).single())
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

/// Sanitize text for use as a filename or directory component.
///
/// Lowercase, replace non-alphanumeric with hyphens, strip leading/trailing
/// hyphens, collapse runs, and truncate to `max_len` chars.
pub fn sanitize_name(text: &str, max_len: usize) -> String {
    let name = text.to_lowercase();
    let name = NON_ALNUM_RE.replace_all(&name, "-");
    let name = name.trim_matches('-');
    let name = MULTI_HYPHEN_RE.replace_all(name, "-");
    let name = &name[..name.len().min(max_len)];
    name.to_string()
}

/// Sanitize a WAD title for use as a directory name.
///
/// Shorthand for [`sanitize_name`] with a 64-char limit.
pub fn sanitize_dirname(title: &str) -> String {
    sanitize_name(title, 64)
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

/// Try to extract a .wad entry from a ZIP archive.
///
/// For multi-WAD ZIPs, picks the WAD containing map lumps (ExMy or MAPxx).
/// Falls back to the first WAD if none contain maps.
fn try_read_wad_from_zip(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;

    // Collect indices of .wad entries
    let wad_indices: Vec<usize> = (0..archive.len())
        .filter(|&i| {
            archive
                .by_index(i)
                .map(|e| e.name().to_lowercase().ends_with(".wad"))
                .unwrap_or(false)
        })
        .collect();

    if wad_indices.is_empty() {
        return None;
    }

    // Single WAD: just return it
    if wad_indices.len() == 1 {
        let mut entry = archive.by_index(wad_indices[0]).ok()?;
        if entry.size() > MAX_ZIP_ENTRY_SIZE {
            return None;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).ok()?;
        return Some(buf);
    }

    // Multiple WADs: pick the one with map lumps
    let mut first_wad: Option<Vec<u8>> = None;

    for &idx in &wad_indices {
        let mut entry = match archive.by_index(idx) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.size() > MAX_ZIP_ENTRY_SIZE {
            continue;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        if entry.read_to_end(&mut buf).is_err() {
            continue;
        }
        let directory = parse_wad_directory(&buf);
        let has_maps = directory.iter().any(|(name, _, _)| {
            ZIP_MAP_DOOM1_RE.is_match(name) || ZIP_MAP_DOOM2_RE.is_match(name)
        });
        if has_maps {
            return Some(buf);
        }
        if first_wad.is_none() {
            first_wad = Some(buf);
        }
    }

    // Fallback: first WAD if none had maps
    first_wad
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

/// Normalize tags from a comma-separated string to a clean list.
///
/// Strips whitespace, lowercases, and removes empty entries.
pub fn normalize_tags(tags: Option<&str>) -> Option<Vec<String>> {
    let tags = tags?;
    let parts: Vec<String> = tags
        .split(',')
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts)
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

    #[test]
    fn test_parse_wad_directory_iwad_magic() {
        let mut wad = Vec::new();
        wad.extend_from_slice(b"IWAD");
        wad.extend_from_slice(&1_i32.to_le_bytes());
        wad.extend_from_slice(&12_i32.to_le_bytes());
        wad.extend_from_slice(&0_u32.to_le_bytes());
        wad.extend_from_slice(&0_u32.to_le_bytes());
        wad.extend_from_slice(b"MAP01\0\0\0");

        let entries = parse_wad_directory(&wad);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "MAP01");
    }

    #[test]
    fn test_parse_wad_directory_multiple_lumps() {
        let mut wad = Vec::new();
        wad.extend_from_slice(b"PWAD");
        wad.extend_from_slice(&3_i32.to_le_bytes());
        wad.extend_from_slice(&12_i32.to_le_bytes());
        // 3 directory entries
        for name in &[b"MAP01\0\0\0", b"THINGS\0\0", b"LINEDEFS"] {
            wad.extend_from_slice(&0_u32.to_le_bytes());
            wad.extend_from_slice(&0_u32.to_le_bytes());
            wad.extend_from_slice(*name);
        }

        let entries = parse_wad_directory(&wad);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0, "MAP01");
        assert_eq!(entries[1].0, "THINGS");
        assert_eq!(entries[2].0, "LINEDEFS");
    }

    #[test]
    fn test_parse_wad_directory_null_terminated_name() {
        let mut wad = Vec::new();
        wad.extend_from_slice(b"PWAD");
        wad.extend_from_slice(&1_i32.to_le_bytes());
        wad.extend_from_slice(&12_i32.to_le_bytes());
        wad.extend_from_slice(&0_u32.to_le_bytes());
        wad.extend_from_slice(&0_u32.to_le_bytes());
        wad.extend_from_slice(b"E1M1\0\0\0\0"); // Null-padded to 8 bytes

        let entries = parse_wad_directory(&wad);
        assert_eq!(entries[0].0, "E1M1");
    }

    #[test]
    fn test_parse_wad_directory_truncated() {
        // WAD header claims 5 lumps but data is truncated after 1
        let mut wad = Vec::new();
        wad.extend_from_slice(b"PWAD");
        wad.extend_from_slice(&5_i32.to_le_bytes());
        wad.extend_from_slice(&12_i32.to_le_bytes());
        // Only 1 directory entry (not 5)
        wad.extend_from_slice(&0_u32.to_le_bytes());
        wad.extend_from_slice(&0_u32.to_le_bytes());
        wad.extend_from_slice(b"MAP01\0\0\0");

        let entries = parse_wad_directory(&wad);
        assert_eq!(entries.len(), 1); // Gracefully handles truncation
    }

    #[test]
    fn test_parse_wad_directory_lowercase_uppercased() {
        let mut wad = Vec::new();
        wad.extend_from_slice(b"PWAD");
        wad.extend_from_slice(&1_i32.to_le_bytes());
        wad.extend_from_slice(&12_i32.to_le_bytes());
        wad.extend_from_slice(&0_u32.to_le_bytes());
        wad.extend_from_slice(&0_u32.to_le_bytes());
        wad.extend_from_slice(b"map01\0\0\0"); // lowercase

        let entries = parse_wad_directory(&wad);
        assert_eq!(entries[0].0, "MAP01"); // Uppercased
    }

    #[test]
    fn test_extract_year_iso_date() {
        assert_eq!(extract_year("2023-03-01"), Some(2023));
    }

    #[test]
    fn test_extract_year_iso_datetime() {
        assert_eq!(extract_year("2023-03-01T12:00:00"), Some(2023));
    }

    #[test]
    fn test_extract_year_year_only() {
        assert_eq!(extract_year("1994"), Some(1994));
    }

    #[test]
    fn test_extract_year_short_string() {
        assert_eq!(extract_year("99"), None);
    }

    #[test]
    fn test_extract_year_non_numeric() {
        assert_eq!(extract_year("abcd"), None);
    }

    #[test]
    fn test_format_size_large() {
        assert_eq!(format_size(1_099_511_627_776), "1.0 TB");
    }

    #[test]
    fn test_format_size_fractional() {
        assert_eq!(format_size(1536), "1.5 KB");
    }

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate("hello", 5, "..."), "hello");
    }

    #[test]
    fn test_truncate_empty() {
        assert_eq!(truncate("", 10, "..."), "");
    }

    #[test]
    fn test_truncate_custom_suffix() {
        // "…" is 3 bytes in UTF-8, so end = 8 - 3 = 5 chars -> "hello"
        assert_eq!(truncate("hello world", 8, "…"), "hello…");
    }

    #[test]
    fn test_sanitize_dirname_numbers_preserved() {
        assert_eq!(sanitize_dirname("Doom2 Map01"), "doom2-map01");
    }

    #[test]
    fn test_sanitize_dirname_empty() {
        assert_eq!(sanitize_dirname(""), "");
    }

    #[test]
    fn test_sanitize_dirname_all_special() {
        assert_eq!(sanitize_dirname("!!!@@@###"), "");
    }

    #[test]
    fn test_sanitize_dirname_truncation() {
        let long_title = "a".repeat(100);
        let result = sanitize_dirname(&long_title);
        assert!(result.len() <= 64);
    }

    #[test]
    fn test_format_rating_custom_max() {
        assert_eq!(format_rating(Some(2), 3), "★★☆");
    }

    #[test]
    fn test_format_rating_exceeds_max() {
        // Rating exceeding max should still work
        assert_eq!(format_rating(Some(5), 3), "★★★★★");
    }

    #[test]
    fn test_format_author_year_empty_author() {
        // Empty string treated same as None
        assert_eq!(format_author_year(Some(""), Some(1994)), "(1994)");
    }

    #[test]
    fn test_compute_md5_known() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "hello").unwrap();
        let hash = compute_md5(&path).unwrap();
        assert_eq!(hash, "5d41402abc4b2a76b9719d911017c592");
    }

    #[test]
    fn test_compute_md5_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        std::fs::write(&path, "").unwrap();
        let hash = compute_md5(&path).unwrap();
        assert_eq!(hash, "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn test_load_wad_data_direct() {
        let dir = tempfile::tempdir().unwrap();
        let wad_path = dir.path().join("test.wad");
        let mut wad_data = Vec::new();
        wad_data.extend_from_slice(b"PWAD");
        wad_data.extend_from_slice(&0_i32.to_le_bytes());
        wad_data.extend_from_slice(&12_i32.to_le_bytes());
        std::fs::write(&wad_path, &wad_data).unwrap();

        let data = load_wad_data(&wad_path).unwrap();
        assert_eq!(&data[..4], b"PWAD");
    }

    #[test]
    fn test_load_wad_data_nonexistent() {
        assert!(load_wad_data(Path::new("/nonexistent/test.wad")).is_none());
    }

    #[test]
    fn test_load_wad_data_zip() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");

        // Build a WAD and wrap in ZIP
        let mut wad_data = Vec::new();
        wad_data.extend_from_slice(b"PWAD");
        wad_data.extend_from_slice(&0_i32.to_le_bytes());
        wad_data.extend_from_slice(&12_i32.to_le_bytes());

        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip_writer = zip::ZipWriter::new(file);
        zip_writer
            .start_file("inner.wad", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip_writer.write_all(&wad_data).unwrap();
        zip_writer.finish().unwrap();

        let data = load_wad_data(&zip_path).unwrap();
        assert_eq!(&data[..4], b"PWAD");
    }

    #[test]
    fn test_normalize_tags_comma_separated() {
        let result = normalize_tags(Some("cacoward, megawad, doom")).unwrap();
        assert_eq!(result, vec!["cacoward", "megawad", "doom"]);
    }

    #[test]
    fn test_normalize_tags_whitespace() {
        let result = normalize_tags(Some("  tag1 , TAG2 , ")).unwrap();
        assert_eq!(result, vec!["tag1", "tag2"]);
    }

    #[test]
    fn test_normalize_tags_none() {
        assert!(normalize_tags(None).is_none());
    }

    #[test]
    fn test_normalize_tags_empty() {
        assert!(normalize_tags(Some("")).is_none());
        assert!(normalize_tags(Some(" , , ")).is_none());
    }

    #[test]
    fn test_sanitize_name_custom_length() {
        let result = sanitize_name("A Very Long Name Here", 10);
        assert!(result.len() <= 10);
        assert_eq!(result, "a-very-lon");
    }
}
