//! The archive's own listing of itself: `ls-laR.gz`, which `ls -laR` of the whole tree
//! writes daily and every mirror but one carries.
//!
//! ```text
//! ./levels/doom2/Ports/megawads:
//! total 9422396
//! -rw-rw-r-- 2 idgmaint idgames     5165 Nov  1 09:44 utakata.txt
//! -rw-rw-r-- 2 idgmaint idgames 11671592 Nov  1 09:44 utakata.zip
//! -rw-rw-r-- 1 idgmaint idgames 24388254 Aug  9  2015 sunlust.zip
//! ```
//!
//! This is the part of the index that needs no site but a mirror, so everything the rest
//! of it knows hangs off what is read here. There is no network in this file.

use std::collections::HashMap;
use std::io::Read;

use caco_core::mirrors::is_bare_file_name;
use chrono::{Datelike, Duration, NaiveDate};
use flate2::read::GzDecoder;

/// Where the listing is, under the root of a mirror.
pub const LISTING_PATH: &str = "ls-laR.gz";

/// More than this is not the listing, whatever a server says. It is 2.8 MB today, and
/// has taken thirty years to get there.
const MAX_LISTING_BYTES: u64 = 256 << 20;

/// One upload: a zip, and the text file beside it if it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedUpload {
    /// The directory, spelled as the API spells it: `levels/doom2/Ports/megawads/`.
    pub dir: String,
    pub filename: String,
    pub size: i64,
    /// When the file last changed on the archive, as `YYYY-MM-DD`.
    pub date: String,
    pub text: Option<TextStamp>,
}

impl ListedUpload {
    /// Its path under the archive's root.
    pub fn path(&self) -> String {
        format!("{}{}", self.dir, self.filename)
    }
}

/// A text file beside an upload, and how to tell whether it has changed since it was
/// last read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextStamp {
    /// Its own spelling, which on a mirror's filesystem is not always the zip's.
    pub name: String,
    pub size: i64,
    pub date: String,
}

/// The listing's text, out of its gzip wrapper. A truncated download is an error here
/// rather than a listing with most of the archive missing from it, which the index would
/// take for most of the archive having been deleted.
pub fn decompress(bytes: &[u8]) -> std::io::Result<String> {
    let mut text = Vec::new();
    GzDecoder::new(bytes)
        .take(MAX_LISTING_BYTES)
        .read_to_end(&mut text)?;
    // It is ASCII today. A name that ever was not would not fetch anyway, and should
    // not cost the other 21,000.
    Ok(String::from_utf8_lossy(&text).into_owned())
}

/// Every upload in a listing made on `generated`, in the order it lists them.
///
/// `ls` gives a time rather than a year for anything from the last six months, so a date
/// only means something relative to when the listing was made.
pub fn parse_listing(text: &str, generated: NaiveDate) -> Vec<ListedUpload> {
    let mut uploads = Vec::new();
    let mut dir: Option<String> = None;
    let mut files: Vec<Listed<'_>> = Vec::new();

    for line in text.lines() {
        if let Some(next) = directory_header(line) {
            collect_uploads(dir.as_deref(), &files, generated, &mut uploads);
            files.clear();
            dir = Some(next);
        } else if let Some(file) = parse_line(line) {
            files.push(file);
        }
    }
    collect_uploads(dir.as_deref(), &files, generated, &mut uploads);
    uploads
}

/// The directory a line introduces, spelled the way the API does (`levels/doom2/`), with
/// `""` for the archive's root.
///
/// A listing comes from somewhere else and goes on to name paths, on a copy of the archive
/// on this machine among other places, so `./../../home` stops here rather than being read.
fn directory_header(line: &str) -> Option<String> {
    let dir = line.strip_suffix(':')?;
    if dir == "." {
        return Some(String::new());
    }
    let dir = dir.strip_prefix("./")?;
    dir.split('/')
        .all(is_bare_file_name)
        .then(|| format!("{dir}/"))
}

/// One regular file in a directory.
struct Listed<'a> {
    name: &'a str,
    size: i64,
    month: u32,
    day: u32,
    /// A year, or a time of day.
    when: &'a str,
}

/// A regular file's line, which is not `total`, a directory or a symlink.
fn parse_line(line: &str) -> Option<Listed<'_>> {
    let mut rest = line;
    let mut field = || {
        let trimmed = rest.trim_start();
        let end = trimmed.find(char::is_whitespace)?;
        let (value, tail) = trimmed.split_at(end);
        rest = tail;
        Some(value)
    };

    let mode = field()?;
    let _links = field()?;
    let _owner = field()?;
    let _group = field()?;
    let size = field()?.parse().ok()?;
    let month = month_number(field()?)?;
    let day = field()?.parse().ok()?;
    let when = field()?;
    if !mode.starts_with('-') {
        return None;
    }
    // One space, and everything after it is the name, spaces and all.
    let name = rest
        .strip_prefix(' ')
        .filter(|name| is_bare_file_name(name))?;
    Some(Listed {
        name,
        size,
        month,
        day,
        when,
    })
}

fn month_number(name: &str) -> Option<u32> {
    let position = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|month| *month == name)?;
    Some(position as u32 + 1)
}

/// The zips in one directory, each with the text file beside it. A directory something
/// is only passing through contributes nothing.
fn collect_uploads(
    dir: Option<&str>,
    files: &[Listed<'_>],
    generated: NaiveDate,
    uploads: &mut Vec<ListedUpload>,
) {
    let Some(dir) = dir.filter(|dir| !is_transit(dir)) else {
        return;
    };
    let by_name: HashMap<String, &Listed<'_>> = files
        .iter()
        .map(|file| (file.name.to_ascii_lowercase(), file))
        .collect();

    for file in files {
        let lower = file.name.to_ascii_lowercase();
        let Some(stem) = lower.strip_suffix(".zip") else {
            continue;
        };
        let Some(date) = file.date(generated) else {
            continue;
        };
        let text = by_name.get(&format!("{stem}.txt")).and_then(|text| {
            Some(TextStamp {
                name: text.name.to_string(),
                size: text.size,
                date: text.date(generated)?,
            })
        });
        uploads.push(ListedUpload {
            dir: dir.to_string(),
            filename: file.name.to_string(),
            size: file.size,
            date,
            text,
        });
    }
}

impl Listed<'_> {
    fn date(&self, generated: NaiveDate) -> Option<String> {
        let date = resolve_date(self.month, self.day, self.when, generated)?;
        Some(date.format("%Y-%m-%d").to_string())
    }
}

/// A date from `ls`, which has either a year or a time of day. A time means the file is
/// from the six months before the listing, so it is the latest such date not after the
/// listing was made, with a day's grace for a server in another time zone from whatever
/// said when the listing was made.
fn resolve_date(month: u32, day: u32, when: &str, generated: NaiveDate) -> Option<NaiveDate> {
    if !when.contains(':') {
        return NaiveDate::from_ymd_opt(when.parse().ok()?, month, day);
    }
    let latest = generated + Duration::days(1);
    [latest.year(), latest.year() - 1]
        .into_iter()
        .filter_map(|year| NaiveDate::from_ymd_opt(year, month, day))
        .find(|date| *date <= latest)
}

/// Whether files in this directory are on their way somewhere. `newstuff/` holds a second
/// name for what has just gone into `levels/`, and `incoming/` what nobody has looked at
/// yet. The API has nothing in either, so an upload here would be a duplicate of what it
/// knows, or something it will not admit to.
fn is_transit(dir: &str) -> bool {
    ["newstuff/", "incoming/", "lmps/newstuff/", "lmps/incoming/"]
        .iter()
        .any(|transit| dir.starts_with(transit))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED: fn() -> NaiveDate = || NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();

    /// Lifted from the real listing, down to the column alignment.
    const LISTING: &str = "\
.:
total 1160
-rw-r--r--  2 fpv      idgames   1699 Jul 29 13:45 README
drwxrwsr-x  2 idgmaint idgames  36864 Nov  1 10:21 combos
-rw-rw-r--  1 idgmaint idgames 445363 Nov  3 01:04 fullsort.gz

./levels/doom2/Ports/megawads:
total 9422396
-rw-rw-r-- 2 idgmaint idgames     5165 Nov  1 09:44 utakata.txt
-rw-rw-r-- 2 idgmaint idgames 11671592 Nov  1 09:44 utakata.zip
-rw-rw-r-- 1 idgmaint idgames     4520 Mar 23  2006 0cb.txt
-rw-rw-r-- 1 idgmaint idgames  1212101 Mar 23  2006 0cb.zip

./newstuff:
total 48
-rw-rw-r-- 2 idgmaint idgames 11671592 Nov  1 09:44 utakata.zip
-rw-rw-r-- 2 idgmaint idgames     5165 Nov  1 09:44 utakata.txt

./incoming/holding/shunt:
total 8
-rw------- 1 idgmaint idgames 3956465 Nov  2 12:59 invasive.zip
";

    fn upload<'a>(uploads: &'a [ListedUpload], name: &str) -> &'a ListedUpload {
        uploads
            .iter()
            .find(|upload| upload.filename == name)
            .unwrap_or_else(|| panic!("{name} not in {uploads:?}"))
    }

    #[test]
    fn test_a_listing_gives_each_zip_with_its_text_file() {
        let uploads = parse_listing(LISTING, GENERATED());

        assert_eq!(
            upload(&uploads, "0cb.zip"),
            &ListedUpload {
                dir: "levels/doom2/Ports/megawads/".to_string(),
                filename: "0cb.zip".to_string(),
                size: 1212101,
                date: "2006-03-23".to_string(),
                text: Some(TextStamp {
                    name: "0cb.txt".to_string(),
                    size: 4520,
                    date: "2006-03-23".to_string(),
                }),
            }
        );
        assert_eq!(
            upload(&uploads, "0cb.zip").path(),
            "levels/doom2/Ports/megawads/0cb.zip"
        );
    }

    /// Hard links: the same upload is in `newstuff/` and in its real home for a while.
    #[test]
    fn test_what_is_passing_through_is_not_an_upload_of_its_own() {
        let uploads = parse_listing(LISTING, GENERATED());
        let paths: Vec<String> = uploads.iter().map(ListedUpload::path).collect();
        assert_eq!(
            paths,
            [
                "levels/doom2/Ports/megawads/utakata.zip",
                "levels/doom2/Ports/megawads/0cb.zip",
            ]
        );
    }

    #[test]
    fn test_a_time_instead_of_a_year_means_the_last_six_months() {
        let date = |month, day, when| resolve_date(month, day, when, GENERATED());
        let ymd = |y, m, d| NaiveDate::from_ymd_opt(y, m, d);

        assert_eq!(date(11, 1, "09:44"), ymd(2026, 11, 1));
        assert_eq!(date(6, 21, "07:01"), ymd(2026, 6, 21));
        // Later in the year than the listing, so last year.
        assert_eq!(date(12, 27, "19:37"), ymd(2025, 12, 27));
        // A year is a year, wherever it falls.
        assert_eq!(date(12, 27, "2023"), ymd(2023, 12, 27));
    }

    /// `Last-Modified` is UTC, and the server writes its own local time.
    #[test]
    fn test_a_file_from_a_day_ahead_of_the_listing_is_this_year() {
        let date = resolve_date(11, 4, "01:04", GENERATED());
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 11, 4));
    }

    #[test]
    fn test_a_leap_day_is_found_in_a_year_that_has_one() {
        let generated = NaiveDate::from_ymd_opt(2028, 3, 10).unwrap();
        assert_eq!(
            resolve_date(2, 29, "12:00", generated),
            NaiveDate::from_ymd_opt(2028, 2, 29)
        );
        // Not a date this listing could mean, and not a reason to lose the rest of it.
        let generated = NaiveDate::from_ymd_opt(2027, 3, 10).unwrap();
        assert_eq!(resolve_date(2, 29, "12:00", generated), None);
    }

    #[test]
    fn test_a_zip_with_no_text_file_is_still_an_upload() {
        let uploads = parse_listing(
            "./combos:\n-rw-rw-r-- 1 a b 10 Mar 23  2006 extrme1.zip\n",
            GENERATED(),
        );
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].text, None);
    }

    #[test]
    fn test_a_name_keeps_its_spelling_and_its_spaces() {
        let uploads = parse_listing(
            "./idstuff/et:\n\
             -rw-r--r-- 1 a b 10 Mar 23  2006 ET 2.60 Final.ZIP\n\
             -rw-r--r-- 1 a b 20 Mar 24  2006 et 2.60 final.txt\n",
            GENERATED(),
        );
        assert_eq!(uploads[0].filename, "ET 2.60 Final.ZIP");
        let text = uploads[0].text.as_ref().unwrap();
        assert_eq!((text.name.as_str(), text.size), ("et 2.60 final.txt", 20));
    }

    /// The real listing has five of each, and neither is something to fetch.
    #[test]
    fn test_links_and_directories_are_not_uploads() {
        let uploads = parse_listing(
            "./themes:\n\
             lrwxrwxrwx 1 a b 10 Mar 23  2006 link.zip -> ../other.zip\n\
             drwxr-xr-x 2 a b 10 Mar 23  2006 dir.zip\n\
             -rw-r--r-- 1 a b 10 Mar 23  2006 real.zip\n",
            GENERATED(),
        );
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].filename, "real.zip");
    }

    /// Names from a mirror become paths, and one of the places they get joined to is a
    /// folder on this machine.
    #[test]
    fn test_a_listing_cannot_name_a_path_outside_the_archive() {
        let uploads = parse_listing(
            "./levels/../../../home:\n\
             -rw-r--r-- 1 a b 10 Mar 23  2006 climbed.zip\n\
             ./levels:\n\
             -rw-r--r-- 1 a b 10 Mar 23  2006 ../../climbed.zip\n\
             -rw-r--r-- 1 a b 10 Mar 23  2006 fine.zip\n",
            GENERATED(),
        );
        let paths: Vec<String> = uploads.iter().map(ListedUpload::path).collect();
        assert_eq!(paths, ["levels/fine.zip"]);
    }

    /// What a mirror's error page, or anything else that is not a listing, comes to.
    #[test]
    fn test_anything_else_is_no_uploads() {
        let page = "<html>\n<body>./levels: 404</body>\n</html>\n";
        assert_eq!(parse_listing(page, GENERATED()), []);
        assert_eq!(parse_listing("", GENERATED()), []);
    }

    #[test]
    fn test_a_truncated_listing_is_an_error_rather_than_a_short_one() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, LISTING.repeat(100).as_bytes()).unwrap();
        let whole = encoder.finish().unwrap();

        assert_eq!(decompress(&whole).unwrap(), LISTING.repeat(100));
        assert!(decompress(&whole[..whole.len() / 2]).is_err());
    }

    #[test]
    fn test_something_that_is_not_gzip_is_an_error() {
        assert!(decompress(b"<html>Just a moment...</html>").is_err());
    }
}
