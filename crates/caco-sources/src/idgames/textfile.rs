//! The text file beside each upload, which says what the upload is.
//!
//! Each follows the archive's upload template, more or less, which is the trouble. It
//! has been a form people fill in by hand since 1994: in whatever their editor saved,
//! with the labels reworded, and with a sentence running on past the end of its field.
//! So this reads as much as it can and is never wrong loudly: a file it can make nothing
//! of still has its text kept as a description, since that is what a search looks in.
//!
//! The reading is checked against a sample of the real thing across every era of the
//! archive, in `tests/idgames_live.rs`. There is no network in this file.

use std::sync::LazyLock;

use regex::Regex;

/// What a description is cut to. The text files with stories in them run to tens of
/// kilobytes, and the index is meant to stay in megabytes.
const MAX_DESCRIPTION_BYTES: usize = 4000;

/// As much of a title or an author as can be anything but a mistake in the file.
const MAX_FIELD_BYTES: usize = 300;

/// What can be read out of a text file. Empty for what it does not say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextFile {
    pub title: String,
    pub author: String,
    pub email: String,
    pub description: String,
    pub credits: String,
    pub base: String,
    pub buildtime: String,
    pub editors: String,
    pub bugs: String,
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// A text file's bytes as text, whatever it was saved as.
///
/// UTF-8 is unmistakable, and so is a UTF-16 byte order mark. That leaves about one file
/// in twenty as a single-byte encoding: CP437 from DOS, or Windows-1252, which is Latin-1
/// with punctuation where Latin-1 has control characters.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(text) = decode_utf16(bytes) {
        return text;
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) if looks_like_cp437(bytes) => bytes.iter().map(|&b| cp437(b)).collect(),
        Err(_) => bytes.iter().map(|&b| windows_1252(b)).collect(),
    }
}

fn decode_utf16(bytes: &[u8]) -> Option<String> {
    let units: Vec<u16> = match bytes {
        [0xff, 0xfe, body @ ..] => body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect(),
        [0xfe, 0xff, body @ ..] => body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect(),
        _ => return None,
    };
    Some(String::from_utf16_lossy(&units))
}

/// Whether a file's high bytes read better as CP437 than as Windows-1252.
///
/// The two disagree about every high byte, so it comes down to which reading gives what
/// people actually write. Runs of bytes from CP437's box-drawing range are a ruled line
/// in one reading and `ÄÄÄÄÄÄ` in the other. An accented letter in a word is in `0x80..`
/// on DOS and `0xC0..` on Windows, where the other reading has a control character or a
/// box corner in the middle of a name.
fn looks_like_cp437(bytes: &[u8]) -> bool {
    let is_letter = |byte: Option<&u8>| byte.is_some_and(u8::is_ascii_alphabetic);
    let is_box = |byte: Option<&u8>| byte.is_some_and(|b| (0xb0..=0xdf).contains(b));

    let (mut dos, mut windows) = (0, 0);
    for (i, byte) in bytes.iter().enumerate() {
        let before = i.checked_sub(1).and_then(|j| bytes.get(j));
        let after = bytes.get(i + 1);
        let by_a_letter = is_letter(before) || is_letter(after);
        match byte {
            // A rule, in a run: `ÚÄÄÄÄÄÄ¿`.
            0xb0..=0xdf if is_box(before) || is_box(after) => dos += 2,
            // A capital in a word (`ESPAÑOL`), or the `°` of `13°` and the `½` of `2½`.
            0xb0..=0xdf if by_a_letter => windows += 1,
            // A lower-case accent in Latin-1, and Greek in CP437.
            0xe0..=0xff if by_a_letter => windows += 1,
            // An apostrophe in `Kenshiro’s`, which in CP437 is an `Æ` nobody wrote.
            0x92 => windows += 1,
            // Curly quotes and dashes have a space on one side. Between two letters
            // these are `ö`, `ò` and `û`.
            0x91 | 0x93..=0x97 if !(is_letter(before) && is_letter(after)) => windows += 1,
            // `ë` in `Raphaël`, `¿` before `Bugs?`, which are control characters and
            // stray symbols on Windows.
            0x80..=0xa8 if by_a_letter => dos += 1,
            _ => {}
        }
    }
    dos > windows
}

/// CP437's high half.
const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»\
                          ░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀\
                          αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

fn cp437(byte: u8) -> char {
    if byte < 0x80 {
        return byte as char;
    }
    CP437_HIGH
        .chars()
        .nth(usize::from(byte) - 0x80)
        .unwrap_or('\u{fffd}')
}

/// Windows-1252's `0x80..0xa0`, where it differs from Latin-1. `\0` for the five it left
/// unassigned, which the text cleaning removes.
const WINDOWS_1252_C1: [char; 32] = [
    '€', '\0', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\0', 'Ž', '\0', '\0', '‘',
    '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\0', 'ž', 'Ÿ',
];

fn windows_1252(byte: u8) -> char {
    match byte {
        0x80..=0x9f => WINDOWS_1252_C1[usize::from(byte) - 0x80],
        // Latin-1, which Unicode's first 256 code points are.
        _ => byte as char,
    }
}

// ---------------------------------------------------------------------------
// Reading the template
// ---------------------------------------------------------------------------

/// The fields the index keeps, and how people have spelled them. Lowercased, with any
/// run of whitespace as one space.
fn field_for_label(label: &str) -> Option<Field> {
    Some(match label {
        "title" | "wad title" | "name" | "level name" | "levelname" => Field::Title,
        "author" | "authors" | "author(s)" | "created by" => Field::Author,
        "uploaded by" => Field::UploadedBy,
        "email address" | "email" | "e-mail" | "e-mail address" | "email addresses" => Field::Email,
        "description" | "your mission" | "story" | "descriptions" => Field::Description,
        "additional credits to"
        | "additional credits"
        | "credits to"
        | "credits"
        | "additional credit to" => Field::Credits,
        "base" | "based on" => Field::Base,
        "build time" | "buildtime" | "time to build" => Field::BuildTime,
        "editor(s) used" | "editors used" | "editor used" | "editors" | "editor" | "editor(s)"
        | "tools used" => Field::Editors,
        "known bugs" | "bugs" | "known bug" => Field::Bugs,
        // The rest of the template. Nothing here keeps them, and reading them is what
        // ends the field before them.
        "filename"
        | "file name"
        | "release date"
        | "date finished"
        | "date completed"
        | "misc. author info"
        | "misc author info"
        | "other files by author"
        | "archive maintainer"
        | "update to"
        | "advanced engine needed"
        | "primary purpose"
        | "new levels"
        | "sounds"
        | "music"
        | "graphics"
        | "dehacked/bex patch"
        | "demos"
        | "other"
        | "other files required"
        | "game"
        | "map #"
        | "map"
        | "level #"
        | "levels"
        | "single player"
        | "cooperative 2-4 player"
        | "deathmatch 2-4 player"
        | "other game styles"
        | "difficulty settings"
        | "may not run with"
        | "may not run with..."
        | "tested with"
        | "the usual"
        | "web sites"
        | "ftp sites" => Field::Other,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Title,
    Author,
    /// Whoever put it on the archive, who for a lot of the old ones is someone tidying up
    /// on behalf of an author named further down. It is above `Author` in the file, so
    /// this is a last resort rather than what comes first.
    UploadedBy,
    Email,
    Description,
    Credits,
    Base,
    BuildTime,
    Editors,
    Bugs,
    /// A part of the template the index does not keep.
    Other,
}

static FIELD_LINE: LazyLock<Regex> = LazyLock::new(|| {
    // A label starting near the margin, then a colon, with the value after it. `Note: `
    // in the middle of a description looks the same, which is what the check against the
    // template's labels is for, and an unknown label only counts if it was lined up with
    // padding before the colon, as the template does and prose does not.
    Regex::new(r"^\s{0,3}([A-Za-z][A-Za-z0-9 ./#()\-]{0,38}?)(\s*):[ \t]*(.*)$").unwrap()
});

/// One line read as a field: which one, if the template has it, and its value.
fn field_line(line: &str) -> Option<(Option<Field>, &str)> {
    let captures = FIELD_LINE.captures(line)?;
    let label = captures.get(1)?.as_str();
    let gap = captures.get(2)?.as_str();
    let padded = gap.len() >= 2 || gap.contains('\t');
    let value = captures.get(3)?.as_str();
    let normal: String = label
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    match field_for_label(&normal) {
        Some(field) => Some((Some(field), value)),
        None if padded => Some((None, value)),
        None => None,
    }
}

/// A line of nothing but a rule (`=====`, `-----`, a box from DOS), or a heading such as
/// `* Play Information *`. Either ends whatever field was running on.
fn is_divider(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.chars().all(|c| !c.is_alphanumeric()) {
        return true;
    }
    trimmed.starts_with('*') && trimmed.ends_with('*')
}

/// How far in a line has to start, with a blank line before it, to still belong to the
/// field above it. The template's values start in column 26, and people line up their
/// paragraphs with them.
const CONTINUATION_INDENT: usize = 8;

fn indent_of(line: &str) -> usize {
    line.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 8 } else { 1 })
        .sum()
}

/// Read a text file.
pub fn parse(text: &str) -> TextFile {
    let clean = clean_control_characters(text);

    let mut read = Reading::default();
    let mut any_field = false;
    let mut current: Option<Field> = None;
    let mut value: Vec<&str> = Vec::new();
    let mut after_blank = false;

    for line in clean.lines() {
        if is_divider(line) {
            read.finish_field(current.take(), &mut value);
            after_blank = false;
            continue;
        }
        if line.trim().is_empty() {
            after_blank = true;
            continue;
        }
        if let Some((field, first)) = field_line(line) {
            read.finish_field(current.take(), &mut value);
            any_field |= field.is_some();
            current = field;
            value.push(first);
            after_blank = false;
            continue;
        }
        // Text after a value, which is that value running on: without a gap, or after
        // one if it is still lined up with the value.
        if current.is_some() && (!after_blank || indent_of(line) >= CONTINUATION_INDENT) {
            if after_blank {
                value.push("");
            }
            value.push(line);
        } else {
            read.finish_field(current.take(), &mut value);
        }
        after_blank = false;
    }
    read.finish_field(current.take(), &mut value);

    let mut found = read.found;
    if found.author.is_empty() {
        found.author = read.uploaded_by;
    }
    // A file that is not the template at all: 1994 and 1995, mostly, when there wasn't
    // one yet. It is still what the upload is, and what a search should look in.
    if !any_field || (found.title.is_empty() && found.description.is_empty()) {
        found.description = first_paragraphs(&clean);
    }
    found
}

/// A text file partway through being read.
#[derive(Default)]
struct Reading {
    found: TextFile,
    uploaded_by: String,
}

impl Reading {
    /// Put what a field collected where it goes. A field seen twice keeps the first, because
    /// a second `Description` is further down in someone's story.
    fn finish_field(&mut self, field: Option<Field>, value: &mut Vec<&str>) {
        let lines = std::mem::take(value);
        let Some(field) = field else {
            return;
        };
        let found = &mut self.found;
        let slot = match field {
            Field::Title => &mut found.title,
            Field::Author => &mut found.author,
            Field::UploadedBy => &mut self.uploaded_by,
            Field::Email => &mut found.email,
            Field::Description => &mut found.description,
            Field::Credits => &mut found.credits,
            Field::Base => &mut found.base,
            Field::BuildTime => &mut found.buildtime,
            Field::Editors => &mut found.editors,
            Field::Bugs => &mut found.bugs,
            Field::Other => return,
        };
        if !slot.is_empty() {
            return;
        }
        *slot = match field {
            Field::Title => clean_line(&lines[..first_with_text(&lines)]),
            Field::Author | Field::UploadedBy => clean_line(&names_only(&lines)),
            Field::Description => clean_paragraphs(&lines, MAX_DESCRIPTION_BYTES),
            _ => clean_line(&lines),
        };
        if is_placeholder(slot) {
            slot.clear();
        }
    }
}

/// How many lines it takes to reach the first with anything on it. A title is one line, and
/// what comes after it is usually what it is for (`--- for DOOM ][ LEGACY`) or a field
/// someone made up (`Required Port : ZDoom`).
fn first_with_text(lines: &[&str]) -> usize {
    lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .map_or(lines.len(), |position| position + 1)
}

/// The lines of an author field that are more names, which for a team runs to several, and
/// not the address or the note that often follows them.
fn names_only<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let first = first_with_text(lines);
    let more = lines[first..]
        .iter()
        .filter(|line| !line.contains(['@', ':']))
        .copied();
    lines[..first].iter().copied().chain(more).collect()
}

/// Whether a value is the upload template's own, left as it was.
fn is_placeholder(value: &str) -> bool {
    matches!(
        value.to_lowercase().as_str(),
        "(title of your masterpiece)"
            | "(your name here)"
            | "(set the mood here but put any long story at the end)"
            | "new from scratch / modified (name of original file)"
            | "none"
            | "n/a"
            | "unknown"
            | "-"
    )
}

/// One field's lines as one line of text, with the padding people line columns up with
/// taken out, and any box drawing with it.
fn clean_line(lines: &[&str]) -> String {
    let joined = lines
        .iter()
        .flat_map(|line| line.split_whitespace())
        .map(without_decoration)
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    cut(&joined, MAX_FIELD_BYTES)
}

/// A description's lines as paragraphs: lines of one paragraph joined, as the template's
/// wrapping is only there to fit an 80-column screen.
fn clean_paragraphs(lines: &[&str], max_bytes: usize) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in lines {
        let words: Vec<String> = line
            .split_whitespace()
            .map(without_decoration)
            .filter(|word| !word.is_empty())
            .collect();
        if words.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
        } else {
            current.push(words.join(" "));
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }
    cut(&paragraphs.join("\n\n"), max_bytes)
}

/// A word with any box drawing and block characters taken out of it. Nothing in a name
/// or a description wants them, and many fonts have none.
fn without_decoration(word: &str) -> String {
    word.chars()
        .filter(|c| !('\u{2500}'..='\u{259f}').contains(c))
        .collect()
}

/// The start of a file with no fields in it to go by.
fn first_paragraphs(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !is_divider(line))
        .take(40)
        .collect();
    clean_paragraphs(&lines, MAX_DESCRIPTION_BYTES / 4)
}

/// At most `max_bytes` of `text`, and not in the middle of a character.
fn cut(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.trim().to_string();
    }
    format!(
        "{}...",
        text[..text.floor_char_boundary(max_bytes)].trim_end()
    )
}

/// Line endings in one spelling, and everything else below a space gone: DOS's end of
/// file, a form feed, the stray bytes of a file that was never quite text.
fn clean_control_characters(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How a real one is laid out, from `levels/doom2/m-o/mkmines.txt`, as CP437.
    fn dos_file() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"Title                   : The Lost Mines\r\n");
        bytes.extend_from_slice(b"Filename                : MKMINES.WAD\r\n");
        bytes.extend_from_slice(b"Author                  : Mark Anthony Klem\r\n");
        bytes.extend_from_slice(b"Email Address           : ");
        bytes.extend_from_slice(&[0xc4; 10]);
        bytes.extend_from_slice(b"\r\n                          mark@netdoor.com\r\n");
        bytes.extend_from_slice(&[0xda]);
        bytes.extend_from_slice(&[0xc4; 40]);
        bytes.extend_from_slice(&[0xbf]);
        bytes.extend_from_slice(b"\r\n\r\nDescription             : The mines.\r\n\x1a");
        bytes
    }

    /// A character too few or too many would shift everything after it.
    #[test]
    fn test_the_cp437_table_has_every_byte_where_it_should_be() {
        assert_eq!(CP437_HIGH.chars().count(), 128);
        for (byte, reads) in [
            (0x80, 'Ç'),
            (0x89, 'ë'),
            (0xa8, '¿'),
            (0xb0, '░'),
            (0xbf, '┐'),
            (0xc4, '─'),
            (0xda, '┌'),
            (0xe1, 'ß'),
            (0xfd, '²'),
            (0xff, '\u{a0}'),
        ] {
            assert_eq!(cp437(byte), reads, "{byte:#x}");
        }
    }

    #[test]
    fn test_utf8_is_taken_as_it_is() {
        assert_eq!(decode("Laocoön".as_bytes()), "Laocoön");
        assert_eq!(decode(b"\xef\xbb\xbfTitle"), "Title");
    }

    #[test]
    fn test_utf16_with_a_byte_order_mark_is_read() {
        let le: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain("Tést".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode(&le), "Tést");
        let be: Vec<u8> = [0xfe, 0xff]
            .into_iter()
            .chain("Tést".encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(decode(&be), "Tést");
    }

    /// Each of these is a line out of a real text file on the archive, and what it says.
    #[test]
    fn test_real_single_byte_lines_read_as_what_they_say() {
        for (bytes, reads) in [
            // Windows
            (&b"Author : Kristian 'Kristus' K\xe4ll"[..], "Kristus' Käll"),
            (b"NWT 1.3 by Denis M\xfcller", "Denis Müller"),
            (b"Author : Laoco\xf6n (AKA MKLBST)", "Laocoön"),
            (b"Level name : Unit\xe9 de Traitement", "Unité"),
            (b"Build Time : 2\xbdmin", "2½min"),
            (b"Second finalist Map for the 13\xb0Cabro's", "13°Cabro"),
            (b"Kitsune\xb2, sequenced by", "Kitsune²"),
            (b"TEXTO ORIGINAL EN ESPA\xd1OL", "ESPAÑOL"),
            (b"M\xc1S ABAJO", "MÁS"),
            (b"Kenshiro\x92s friends", "Kenshiro’s"),
            (b"he said \x93hello\x94 to me", "“hello”"),
            // DOS
            (b"Rapha\x89l Quinet (DEU 5.1)", "Raphaël"),
            (b"Known Bugs : \xa8Bugs?... well, I did", "¿Bugs?"),
            (b"sch\x94ne Gr\x81\xe1e", "schöne Grüße"),
        ] {
            let text = decode(bytes);
            assert!(text.contains(reads), "{text:?} should have {reads:?}");
        }
    }

    #[test]
    fn test_ruled_lines_mean_dos() {
        let mut bytes = vec![0xda];
        bytes.extend_from_slice(&[0xc4; 20]);
        bytes.extend_from_slice(b"\xbf\nby Rapha\x89l");
        let text = decode(&bytes);
        assert!(text.starts_with("┌───"), "{text}");
        assert!(text.ends_with("Raphaël"), "{text}");
    }

    #[test]
    fn test_the_template_is_read() {
        let found = parse(&decode(&dos_file()));
        assert_eq!(found.title, "The Lost Mines");
        assert_eq!(found.author, "Mark Anthony Klem");
        assert_eq!(found.email, "mark@netdoor.com");
        assert_eq!(found.description, "The mines.");
    }

    #[test]
    fn test_each_kept_field_is_read_from_how_the_template_spells_it() {
        let found = parse(
            "===========================================================================\n\
             Archive Maintainer      : place this in levels/doom2/\n\
             Advanced engine needed  : Boom\n\
             ===========================================================================\n\
             Title                   : Sunlust\n\
             Filename                : sunlust.wad\n\
             Release date            : 2015\n\
             Author                  : Ribbiks & Dannebubinga\n\
             Email Address           : ribbiks@example.com\n\
             Misc. Author Info       : Two people\n\
             \n\
             Description             : 32 boom-compatible maps.\n\
             \n\
             Additional Credits to   : Everyone at DW\n\
             ===========================================================================\n\
             * Construction *\n\
             \n\
             Base                    : New from scratch\n\
             Build Time              : Three years\n\
             Editor(s) used          : GZDoom Builder\n\
             Known Bugs              : A few\n\
             May Not Run With        : Vanilla\n",
        );
        assert_eq!(
            found,
            TextFile {
                title: "Sunlust".to_string(),
                author: "Ribbiks & Dannebubinga".to_string(),
                email: "ribbiks@example.com".to_string(),
                description: "32 boom-compatible maps.".to_string(),
                credits: "Everyone at DW".to_string(),
                base: "New from scratch".to_string(),
                buildtime: "Three years".to_string(),
                editors: "GZDoom Builder".to_string(),
                bugs: "A few".to_string(),
            }
        );
    }

    #[test]
    fn test_a_value_runs_on_when_it_is_lined_up_or_has_no_gap() {
        let found = parse(
            "Title         : Frag City\n\
             Description   : You start out in the middle of an arena,\n\
             \x20                if you are able to reach a teleporter.\n\
             \n\
             \x20                What is different about this WAD?\n\
             \n\
             Credits to    : My friends\n",
        );
        assert_eq!(
            found.description,
            "You start out in the middle of an arena, if you are able to reach a \
             teleporter.\n\nWhat is different about this WAD?"
        );
        assert_eq!(found.credits, "My friends");
    }

    #[test]
    fn test_text_at_the_margin_runs_on_until_a_gap() {
        let found = parse(
            "Description : A megawad.\n\
             All maps were made by me.\n\
             \n\
             This is a remark at the end of the file.\n",
        );
        assert_eq!(found.description, "A megawad. All maps were made by me.");
    }

    /// `Note:` is not in the template, and nor is it lined up like one.
    #[test]
    fn test_a_colon_in_prose_does_not_end_a_description() {
        let found = parse(
            "Description : A megawad.\n\
             Note: the secret exit is on MAP15.\n\
             Warning: it is hard.\n\
             Additional Credits to : Everyone\n",
        );
        assert_eq!(
            found.description,
            "A megawad. Note: the secret exit is on MAP15. Warning: it is hard."
        );
        assert_eq!(found.credits, "Everyone");
    }

    /// `levels/doom2/m-o/mkmines.txt`, which someone tidied up for the archive in 2004,
    /// ten years after Mark Anthony Klem made it.
    #[test]
    fn test_whoever_uploaded_it_is_not_its_author_unless_nothing_else_says_so() {
        let found = parse(
            "Uploaded by             : myk helnyte (myk@helnyte.zzn.com)\n\
             Title                   : The Lost Mines\n\
             Author                  : Mark Anthony Klem\n",
        );
        assert_eq!(found.author, "Mark Anthony Klem");

        let found = parse("Uploaded by : myk helnyte\nTitle : The Lost Mines\n");
        assert_eq!(found.author, "myk helnyte");
    }

    /// `andydm1.txt` and `d2torn-x.txt`, both of which the API reads as just the first line.
    #[test]
    fn test_a_title_is_one_line() {
        let found = parse(
            "Title : A N D Y D M 1\n\
             \x20       --- for DOOM ][ LEGACY\n\
             Author : Andy\n",
        );
        assert_eq!(found.title, "A N D Y D M 1");

        let found = parse("Title : Doom 2 Tournament\nRequired Port : ZDoom\n");
        assert_eq!(found.title, "Doom 2 Tournament");

        // On the line below the label, which people do.
        let found = parse("Title :\n        Frag City\nAuthor : Me\n");
        assert_eq!(found.title, "Frag City");
    }

    #[test]
    fn test_an_author_runs_on_with_more_names_and_not_with_an_address() {
        let found = parse(
            "Author : Obsidian (project lead)\n\
             \x20        Pinchy, Jimmy\n\
             \x20        INET ADDRESS: someone@example.com\n\
             \x20        them@example.com\n\
             Title : A Project\n",
        );
        assert_eq!(found.author, "Obsidian (project lead) Pinchy, Jimmy");
    }

    /// Lined up like one, it is a field: this one is from `fragcity.txt`.
    #[test]
    fn test_a_label_that_is_not_in_the_template_still_ends_the_field_before_it() {
        let found = parse(
            "Misc. Author Info       : A student.\n\
             Technical Advisor       : Paul Robinson, who figured it out\n\
             Description             : An arena.\n",
        );
        assert_eq!(found.description, "An arena.");
    }

    #[test]
    fn test_a_heading_or_a_rule_ends_a_value() {
        let found = parse("Description : A map.\n* Play Information *\nGame : DOOM2\n");
        assert_eq!(found.description, "A map.");
        let found = parse("Description : A map.\n=======\nsomething else\n");
        assert_eq!(found.description, "A map.");
    }

    #[test]
    fn test_labels_are_read_however_they_are_spelled() {
        for (line, field) in [
            ("Authors : Erwen Tang", "Erwen Tang"),
            ("Author(s)\t\t: Erwen Tang", "Erwen Tang"),
            ("AUTHOR: Erwen Tang", "Erwen Tang"),
            ("  Author      :     Erwen    Tang   ", "Erwen Tang"),
        ] {
            assert_eq!(parse(line).author, field, "{line}");
        }
        assert_eq!(
            parse("Your Mission : Kill them all").description,
            "Kill them all"
        );
        assert_eq!(parse("Editors used : DEU").editors, "DEU");
    }

    #[test]
    fn test_what_the_template_left_there_is_not_a_value() {
        let found = parse(
            "Title       : (title of your masterpiece)\n\
             Author      : (your name here)\n\
             Description : A map.\n\
             Known Bugs  : None\n",
        );
        assert_eq!(found.title, "");
        assert_eq!(found.author, "");
        assert_eq!(found.bugs, "");
        assert_eq!(found.description, "A map.");
    }

    #[test]
    fn test_the_first_of_two_descriptions_is_the_one() {
        let found = parse(
            "Description : The short one.\n\
             \n\
             Story : Long ago, in a Mars base far away.\n",
        );
        assert_eq!(found.description, "The short one.");
    }

    /// `levels/doom/a-c/asdoom.txt`, from 1994, before there was a template.
    #[test]
    fn test_a_file_that_is_not_the_template_is_kept_as_its_description() {
        let found = parse(
            "DOOM - the entire ASDOOM series,\n\
             replacing levels E1M1 to E1M9.\n\
             A whole new episode - FREE!\n\
             \n\
             No modification to your existing\n\
             DOOM files.\n",
        );
        assert_eq!(found.title, "");
        assert_eq!(
            found.description,
            "DOOM - the entire ASDOOM series, replacing levels E1M1 to E1M9. A whole new \
             episode - FREE!\n\nNo modification to your existing DOOM files."
        );
    }

    #[test]
    fn test_a_file_with_no_title_or_description_field_keeps_its_text() {
        let found = parse("Filename : a.wad\n\nThis level has a big room in it.\n");
        assert_eq!(found.title, "");
        assert!(found.description.contains("big room"), "{found:?}");
    }

    #[test]
    fn test_an_empty_file_says_nothing() {
        assert_eq!(parse(""), TextFile::default());
        assert_eq!(parse("\n\n\x1a"), TextFile::default());
    }

    #[test]
    fn test_a_description_is_only_so_long() {
        let found = parse(&format!("Description : {}", "word ".repeat(10_000)));
        assert!(found.description.len() <= MAX_DESCRIPTION_BYTES + 3);
        assert!(found.description.ends_with("..."));
    }

    /// Not splitting a multi-byte character is the point.
    #[test]
    fn test_a_cut_lands_between_characters() {
        let found = parse(&format!("Title : {}", "é".repeat(1000)));
        assert!(found.title.len() <= MAX_FIELD_BYTES + 3);
        assert!(found.title.ends_with("é..."), "{}", found.title);
    }

    /// Control characters and box drawing are what would paint as nothing, or as tofu.
    #[test]
    fn test_nothing_unprintable_gets_through() {
        let found = parse("Title : A\u{7}B ──── C\u{2588}\u{1a}");
        assert_eq!(found.title, "AB C");
    }
}
