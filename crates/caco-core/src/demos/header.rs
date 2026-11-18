//! What a demo says about itself, from its own bytes: its format, skill, the map it starts on,
//! how many players and how long it runs. So a demo caco did not record, one from a speedrun
//! archive or one that came in a WAD's zip, says as much in the list as one it did.
//!
//! The layouts are the engines' own. Vanilla, Boom, MBF, PrBoom+, MBF21 and the two headers
//! that open with a 255 are read as nyan-doom's `G_ReadDemoHeaderEx` reads them, and a ZDoom
//! demo as uzdoom's `G_BeginRecording` writes it. A dsda-family sourceport also leaves a footer
//! after the end marker, a small WAD whose `PORTNAME` and `CMDLINE` lumps say what recorded it
//! and with which files (`dsda_WriteExDemoFooter`), which is all there is to go on for a demo
//! from somewhere else.
//!
//! Anything that does not read cleanly is `None` rather than a guess: a wrong map or length in
//! the list is worse than a dash.

/// What ends the tics, in every format here but ZDoom's.
const DEMO_MARKER: u8 = 0x80;

/// Tics a second.
const TICRATE: u32 = 35;

/// How much of a file is worth reading. A vanilla demo is about 8 KB a minute, so this is
/// days of play, and anything larger is not a demo.
const MAX_DEMO_BYTES: u64 = 64 * 1024 * 1024;

/// The versions nyan-doom will play: up to Doom 1.2 the first byte is the skill, then Doom
/// 1.4 to 1.91, Boom to PrBoom+, and MBF21.
fn is_known_version(version: u8) -> bool {
    matches!(version, 0..=4 | 104..=111 | 200..=214 | 221)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoHeader {
    pub format: DemoFormat,
    /// From 0, I'm Too Young to Die, to 4, Nightmare. `None` when a ZDoom demo does not say.
    pub skill: Option<u8>,
    pub start: Option<StartMap>,
    pub players: usize,
    /// How long it runs, when that can be read without playing it.
    pub tics: Option<u32>,
    pub footer: Option<DemoFooter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoFormat {
    /// Doom 1.2 and earlier, with no version byte.
    Doom12,
    /// Doom 1.4 to 1.91, by version byte.
    Vanilla(u8),
    /// Boom, MBF, LxDoom and PrBoom+, by version byte.
    Boom(u8),
    Mbf21,
    /// dsda-doom's own, which only it and its descendants play.
    Dsda,
    /// ZDoom and its descendants', by the engine's demo version.
    ZDoom(u16),
}

impl DemoFormat {
    /// What the format is called by the people who use it.
    pub fn name(self) -> String {
        match self {
            DemoFormat::Doom12 => "Doom 1.2".to_string(),
            DemoFormat::Vanilla(109) => "Doom 1.9".to_string(),
            DemoFormat::Vanilla(110) => "Doom 1.10".to_string(),
            DemoFormat::Vanilla(111) => "Doom 1.9 longtics".to_string(),
            DemoFormat::Vanilla(v) => format!("Doom 1.{}", v - 100),
            DemoFormat::Boom(200..=202) => "Boom".to_string(),
            DemoFormat::Boom(203) => "MBF".to_string(),
            DemoFormat::Boom(_) => "PrBoom+".to_string(),
            DemoFormat::Mbf21 => "MBF21".to_string(),
            DemoFormat::Dsda => "dsda-doom".to_string(),
            DemoFormat::ZDoom(_) => "ZDoom".to_string(),
        }
    }
}

/// Doom's skills, from the easiest, as speedrunners write them and as the menu does. A demo
/// numbers them from 0, and `-skill` from 1.
pub const SKILLS: [(&str, &str); 5] = [
    ("ITYTD", "I'm Too Young to Die"),
    ("HNTR", "Hey, Not Too Rough"),
    ("HMP", "Hurt Me Plenty"),
    ("UV", "Ultra-Violence"),
    ("NM", "Nightmare!"),
];

/// The map a demo starts on: by number, or by name in a ZDoom demo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartMap {
    Numbers { episode: u8, map: u8 },
    Named(String),
}

impl StartMap {
    /// Whether this is the map a stats file or a WAD calls `lump`. A Doom II demo says
    /// episode 1, as the game does, so `MAP07` is episode 1 map 7, and so is `E1M7`, which no
    /// WAD has as well.
    pub fn is(&self, lump: &str) -> bool {
        match self {
            StartMap::Named(name) => name.eq_ignore_ascii_case(lump.trim()),
            StartMap::Numbers { episode, map } => match crate::sourceports::map_numbers(lump) {
                Some((Some(e), m)) => (e, m) == (*episode, *map),
                Some((None, m)) => *episode <= 1 && m == *map,
                None => false,
            },
        }
    }
}

/// What a dsda-family sourceport says about the demo it recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DemoFooter {
    /// Its name and version, such as `nyan-doom 1.2.1`.
    pub sourceport: Option<String>,
    /// The files and settings it was recorded with, as a command line: `-iwad "doom2.wad"
    /// -file "scythe.wad" -complevel 2`. Names only, never where they were.
    pub command_line: Option<String>,
}

impl DemoHeader {
    /// Read a demo file, or `None` if it is not one this can read.
    pub fn read_file(path: &std::path::Path) -> Option<DemoHeader> {
        use std::io::Read;
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > MAX_DEMO_BYTES {
            return None;
        }
        let mut data = Vec::new();
        file.take(MAX_DEMO_BYTES).read_to_end(&mut data).ok()?;
        Self::read(&data)
    }

    /// Read a demo's bytes, or `None` if they are not a demo this can read.
    pub fn read(data: &[u8]) -> Option<DemoHeader> {
        if data.starts_with(b"FORM") && data.get(8..12) == Some(b"ZDEM") {
            return read_zdoom(data);
        }
        read_doom(data)
    }

    /// How long it runs, in seconds.
    pub fn duration_secs(&self) -> Option<u32> {
        self.tics.map(|tics| tics / TICRATE)
    }

    /// The skill as speedrunners write it: UV, NM.
    pub fn skill_name(&self) -> Option<String> {
        self.skill
            .map(|skill| match SKILLS.get(usize::from(skill)) {
                Some((short, _)) => short.to_string(),
                None => format!("Skill {}", u16::from(skill) + 1),
            })
    }

    /// The map it starts on, by lump name. A vanilla or Boom demo only has numbers, and Doom
    /// 2's first episode looks the same as Doom's: `episode_names` says which the WAD uses,
    /// for the numbers that could be either.
    pub fn map_name(&self, episode_names: bool) -> Option<String> {
        match self.start.as_ref()? {
            StartMap::Named(name) => Some(name.clone()),
            StartMap::Numbers { episode, map } => {
                let doom1 = *episode > 1 || (episode_names && *map <= 9);
                Some(if doom1 {
                    format!("E{episode}M{map}")
                } else {
                    format!("MAP{map:02}")
                })
            }
        }
    }
}

/// Every format that ends its tics with [`DEMO_MARKER`].
fn read_doom(data: &[u8]) -> Option<DemoHeader> {
    let mut p = 0;
    let mut version = *data.first()?;
    p += 1;
    // dsda-doom's own format says where its tics end and how many there are.
    let mut dsda: Option<(usize, u32)> = None;

    if version == 255 {
        if data.get(1..7) == Some(&[0x1d, b'D', b'S', b'D', b'A', 0xe6][..]) {
            let dsda_version = *data.get(7)?;
            // The header data's size in each version there has been.
            let extra = match dsda_version {
                1 => 8,
                2 => 9,
                3 => 10,
                _ => return None,
            };
            let end_marker = read_be32(data, 8)? as usize;
            let tics = read_be32(data, 12)?;
            dsda = Some((end_marker, tics));
            p = 8 + extra;
        } else if data.get(1..6) == Some(&b"PR+UM"[..]) {
            // PrBoom+um's, which named UMAPINFO and the start map before the real header.
            // 255, the signature (6), its version (1), how many extensions (2), the one
            // extension's length (1) and name (8), and the map (8).
            if data.get(7) != Some(&1)
                || data.get(8..10) != Some(&[1, 0][..])
                || data.get(10) != Some(&8)
                || data.get(11..19) != Some(&b"UMAPINFO"[..])
            {
                return None;
            }
            p = 27;
        } else {
            // Eternity's, or something nobody has written down.
            return None;
        }
        version = *data.get(p)?;
        p += 1;
    }
    let header_start = p - 1;

    if !is_known_version(version) {
        return None;
    }

    let format;
    let skill;
    let episode;
    let map;
    let longtics;
    // Four players in Doom, where Boom and later keep room for thirty-two.
    let players_stored = if version < 200 { 4 } else { 32 };

    if version < 200 {
        longtics = version >= 111;
        if version >= 100 {
            let bytes = data.get(p..p + 8)?;
            format = DemoFormat::Vanilla(version);
            skill = bytes[0];
            episode = bytes[1];
            map = bytes[2];
            p += 8;
        } else {
            let bytes = data.get(p..p + 2)?;
            format = DemoFormat::Doom12;
            skill = version;
            episode = bytes[0];
            map = bytes[1];
            p += 2;
            // What nyan-doom refuses as not Doom: Heretic's and Hexen's have the same header.
            let size = data.len() - header_start;
            if data.last() == Some(&DEMO_MARKER)
                && ((size >= 8 && !(size - 8).is_multiple_of(4))
                    || map > 9
                    || data.get(header_start + 4) == Some(&2)
                    || data.get(header_start + 6) == Some(&2))
            {
                return None;
            }
        }
    } else {
        // The signature.
        p += 6;
        // The compatibility byte, in the versions that have one.
        match version {
            200..=202 | 210..=214 => p += 1,
            203 if data.get(header_start + 2) == Some(&b'M') => p += 1,
            _ => {}
        }
        longtics = matches!(version, 214 | 221);
        let bytes = data.get(p..p + 5)?;
        skill = bytes[0];
        episode = bytes[1];
        map = bytes[2];
        p += 5;
        // The game options: MBF21's say how many compatibility flags follow.
        if version == 221 {
            let count = *data.get(p + 20)? as usize;
            if count > 25 {
                return None;
            }
            p += 21 + count;
            format = DemoFormat::Mbf21;
        } else {
            p += if version == 200 { 256 } else { 64 };
            format = DemoFormat::Boom(version);
        }
    }

    let in_game = data.get(p..p + players_stored)?;
    p += players_stored;
    // What a demo's header can hold, which is little enough to tell one from a file that
    // only happens to start with a byte a demo could: each player is in or out, someone
    // is playing, and the map is one there could be.
    if !in_game[..4].iter().all(|&b| b <= 1) || !is_plausible(format, skill, episode, map) {
        return None;
    }
    let players = in_game[..4].iter().filter(|&&b| b != 0).count();
    if players == 0 {
        return None;
    }

    let bytes_per_tic = if longtics { 5 } else { 4 } + usize::from(dsda.is_some());

    let (marker, tics) = match dsda {
        Some((end_marker, tics)) => {
            if data.get(end_marker) != Some(&DEMO_MARKER) {
                return None;
            }
            (Some(end_marker), Some(tics))
        }
        None => match find_marker(data, p, bytes_per_tic) {
            Some((marker, steps)) => (Some(marker), Some((steps / players) as u32)),
            // With no version byte, the end where the tics say it is is the only other sign
            // this is a demo at all.
            None if format == DemoFormat::Doom12 => return None,
            None => (None, None),
        },
    };

    Some(DemoHeader {
        format: if dsda.is_some() {
            DemoFormat::Dsda
        } else {
            format
        },
        skill: Some(skill),
        start: Some(StartMap::Numbers { episode, map }),
        players,
        tics,
        footer: marker.and_then(|marker| read_footer(data, marker + 1)),
    })
}

/// Whether a header's numbers are ones a game could have started with. Before Boom there were
/// five skills and four episodes of nine maps or a Doom 2 of up to 32, written as episode 1;
/// Boom and later can start anywhere UMAPINFO can name, and dsda adds skills of its own.
fn is_plausible(format: DemoFormat, skill: u8, episode: u8, map: u8) -> bool {
    match format {
        DemoFormat::Doom12 => skill <= 4 && (1..=4).contains(&episode) && (1..=9).contains(&map),
        DemoFormat::Vanilla(_) => {
            skill <= 4 && (1..=4).contains(&episode) && (1..=99).contains(&map)
        }
        _ => skill <= 7 && episode <= 9 && map <= 99,
    }
}

/// Where the end marker is, and how many tics (of every player) come before it, stepping a
/// tic at a time from `start`. `None` if the steps run off the end first, which is a demo cut
/// short or one whose tics are not the size they were taken to be.
fn find_marker(data: &[u8], start: usize, bytes_per_tic: usize) -> Option<(usize, usize)> {
    let mut q = start;
    let mut steps = 0;
    loop {
        match data.get(q) {
            Some(&DEMO_MARKER) => return Some((q, steps)),
            Some(_) => {
                steps += 1;
                q += bytes_per_tic;
            }
            None => return None,
        }
    }
}

/// The footer a dsda-family sourceport writes after the end marker: a WAD, which can only be
/// found by looking for its signature, since the bytes in between are other extensions.
fn read_footer(data: &[u8], from: usize) -> Option<DemoFooter> {
    let rest = data.get(from..)?;
    let start = rest.windows(4).position(|w| w == b"PWAD")?;
    let wad = &rest[start..];
    let lumps = read_le32(wad, 4)? as usize;
    let directory = read_le32(wad, 8)? as usize;
    let mut footer = DemoFooter::default();
    for i in 0..lumps.min(64) {
        let entry = directory + i * 16;
        let pos = read_le32(wad, entry)? as usize;
        let size = read_le32(wad, entry + 4)? as usize;
        let name = wad.get(entry + 8..entry + 16)?;
        let name = name.split(|&b| b == 0).next().unwrap_or_default();
        let text = || {
            wad.get(pos..pos.checked_add(size)?)
                .map(|bytes| String::from_utf8_lossy(bytes).trim().to_string())
                .filter(|text| !text.is_empty())
        };
        match name {
            b"PORTNAME" => footer.sourceport = text(),
            b"CMDLINE" => footer.command_line = text(),
            _ => {}
        }
    }
    (footer != DemoFooter::default()).then_some(footer)
}

/// A ZDoom demo: an IFF form of chunks, whose header names the map and whose cvars hold the
/// skill. The tics are compressed, so how long it runs is not known without playing it.
fn read_zdoom(data: &[u8]) -> Option<DemoHeader> {
    let mut version = None;
    let mut start = None;
    let mut players = 0;
    let mut skill = None;
    let mut p = 12;
    while let (Some(id), Some(len)) = (data.get(p..p + 4), read_be32(data, p + 4)) {
        let len = len as usize;
        let body = data.get(p + 8..(p + 8).checked_add(len)?)?;
        match id {
            b"ZDHD" => {
                version = Some(u16::from_be_bytes([*body.first()?, *body.get(1)?]));
                let name = body.get(4..)?.split(|&b| b == 0).next()?;
                let name = String::from_utf8_lossy(name).trim().to_uppercase();
                start = (!name.is_empty()).then_some(StartMap::Named(name));
            }
            b"UINF" => players += 1,
            b"VARS" => skill = zdoom_skill(body),
            // The tics, which are all there is after this.
            b"BODY" => break,
            _ => {}
        }
        // Chunks are padded to an even length.
        p += 8 + len + (len & 1);
    }
    Some(DemoHeader {
        format: DemoFormat::ZDoom(version?),
        skill,
        start,
        players,
        tics: None,
        footer: None,
    })
}

/// The `skill` cvar, from a ZDoom demo's `\name\value` list of them.
fn zdoom_skill(vars: &[u8]) -> Option<u8> {
    let text = String::from_utf8_lossy(vars);
    let mut fields = text.trim_end_matches('\0').split('\\');
    while let Some(field) = fields.next() {
        if field.eq_ignore_ascii_case("skill") {
            return fields.next()?.trim().parse().ok();
        }
    }
    None
}

fn read_be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn read_le32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Doom 1.9 demo: one player, `tics` tics of standing still.
    fn vanilla(version: u8, skill: u8, episode: u8, map: u8, tics: usize) -> Vec<u8> {
        let mut data = vec![version, skill, episode, map, 0, 0, 0, 0, 0, 1, 0, 0, 0];
        let tic_size = if version >= 111 { 5 } else { 4 };
        data.extend(std::iter::repeat_n(0u8, tics * tic_size));
        data.push(DEMO_MARKER);
        data
    }

    /// A PrBoom+ demo (complevel 9's format, 211's layout as nyan-doom reads it).
    fn prboom(version: u8, players: usize, tics: usize) -> Vec<u8> {
        let mut data = vec![version, 0x1d, b'B', b'o', b'o', b'm', 0xe6, 0];
        data.extend([3, 1, 7, 0, 0]); // UV, episode 1, map 7, not deathmatch, player 0
        data.extend([0u8; 64]);
        let mut in_game = [0u8; 32];
        in_game[..players].fill(1);
        data.extend(in_game);
        let tic_size = if version == 214 { 5 } else { 4 };
        data.extend(std::iter::repeat_n(1u8, tics * players * tic_size));
        data.push(DEMO_MARKER);
        data
    }

    /// What `dsda_WriteExDemoFooter` leaves after the marker.
    fn footer(lumps: &[(&str, &str)]) -> Vec<u8> {
        let mut body: Vec<u8> = Vec::new();
        let mut directory: Vec<u8> = Vec::new();
        for (name, text) in lumps {
            directory.extend((12 + body.len() as u32).to_le_bytes());
            directory.extend((text.len() as u32).to_le_bytes());
            let mut lump_name = [0u8; 8];
            lump_name[..name.len()].copy_from_slice(name.as_bytes());
            directory.extend(lump_name);
            body.extend(text.as_bytes());
        }
        let mut wad = b"PWAD".to_vec();
        wad.extend((lumps.len() as u32).to_le_bytes());
        wad.extend((12 + body.len() as u32).to_le_bytes());
        wad.extend(body);
        wad.extend(directory);
        wad
    }

    #[test]
    fn test_a_doom_19_demo() {
        let header = DemoHeader::read(&vanilla(109, 3, 1, 1, 35 * 90)).unwrap();
        assert_eq!(header.format, DemoFormat::Vanilla(109));
        assert_eq!(header.format.name(), "Doom 1.9");
        assert_eq!(header.skill_name().as_deref(), Some("UV"));
        assert_eq!(header.map_name(false).as_deref(), Some("MAP01"));
        assert_eq!(header.map_name(true).as_deref(), Some("E1M1"));
        assert_eq!(header.players, 1);
        assert_eq!(header.duration_secs(), Some(90));
        assert_eq!(header.footer, None);
    }

    #[test]
    fn test_longtics_take_five_bytes_a_tic() {
        let header = DemoHeader::read(&vanilla(111, 4, 3, 2, 70)).unwrap();
        assert_eq!(header.format.name(), "Doom 1.9 longtics");
        assert_eq!(header.tics, Some(70));
        // An episode past the first can only be Doom's.
        assert_eq!(header.map_name(false).as_deref(), Some("E3M2"));
    }

    #[test]
    fn test_a_map_past_nine_is_doom_2s_whatever_the_wad_is_taken_for() {
        let header = DemoHeader::read(&vanilla(109, 2, 1, 15, 1)).unwrap();
        assert_eq!(header.map_name(true).as_deref(), Some("MAP15"));
        assert_eq!(header.skill_name().as_deref(), Some("HMP"));
    }

    #[test]
    fn test_a_doom_12_demo_has_no_version_byte() {
        let mut data = vec![2, 1, 3, 1, 0, 0, 0];
        data.extend([0u8; 4 * 35]);
        data.push(DEMO_MARKER);
        let header = DemoHeader::read(&data).unwrap();
        assert_eq!(header.format, DemoFormat::Doom12);
        assert_eq!(header.skill, Some(2));
        assert_eq!(header.map_name(true).as_deref(), Some("E1M3"));
        assert_eq!(header.duration_secs(), Some(1));
    }

    #[test]
    fn test_what_nyan_doom_would_refuse_as_not_doom_is_not_read() {
        // A Heretic demo's tics are six bytes, which leaves a size Doom's could not have.
        let mut data = vec![2, 1, 3, 1, 0, 0, 0];
        data.extend([0u8; 6 * 3]);
        data.push(DEMO_MARKER);
        assert!((data.len() - 8) % 4 != 0);
        assert_eq!(DemoHeader::read(&data), None);
    }

    #[test]
    fn test_a_prboom_demo_counts_its_players_and_divides_the_tics_between_them() {
        let header = DemoHeader::read(&prboom(211, 2, 35 * 60)).unwrap();
        assert_eq!(header.format.name(), "PrBoom+");
        assert_eq!(header.players, 2);
        assert_eq!(header.duration_secs(), Some(60));
        assert_eq!(header.map_name(false).as_deref(), Some("MAP07"));

        let longtics = DemoHeader::read(&prboom(214, 1, 35)).unwrap();
        assert_eq!(longtics.duration_secs(), Some(1));
    }

    #[test]
    fn test_an_mbf21_demo_skips_as_many_compatibility_flags_as_it_says() {
        let mut data = vec![221, 0x1d, b'M', b'B', b'F', 0xe6, 0];
        data.extend([4, 1, 30, 0, 0]);
        let mut options = vec![0u8; 20];
        options.push(25);
        options.extend([1u8; 25]);
        data.extend(options);
        let mut in_game = [0u8; 32];
        in_game[0] = 1;
        data.extend(in_game);
        data.extend([0u8; 5 * 35 * 2]);
        data.push(DEMO_MARKER);
        let header = DemoHeader::read(&data).unwrap();
        assert_eq!(header.format, DemoFormat::Mbf21);
        assert_eq!(header.skill_name().as_deref(), Some("NM"));
        assert_eq!(header.map_name(false).as_deref(), Some("MAP30"));
        assert_eq!(header.duration_secs(), Some(2));
    }

    #[test]
    fn test_a_dsda_format_demo_says_where_it_ends_and_how_long_it_is() {
        let mut data = vec![255, 0x1d, b'D', b'S', b'D', b'A', 0xe6, 3];
        let mbf21 = {
            let mut h = vec![221, 0x1d, b'M', b'B', b'F', 0xe6, 0];
            h.extend([3, 1, 1, 0, 0]);
            h.extend([0u8; 20]);
            h.push(0);
            let mut in_game = [0u8; 32];
            in_game[0] = 1;
            h.extend(in_game);
            h
        };
        let header_len = 8 + 10 + mbf21.len();
        let tics = 350u32;
        let end_marker = header_len + 6 * tics as usize;
        data.extend((end_marker as u32).to_be_bytes());
        data.extend(tics.to_be_bytes());
        data.extend([0, 0]);
        data.extend(mbf21);
        data.extend(std::iter::repeat_n(0x7fu8, 6 * tics as usize));
        data.push(DEMO_MARKER);
        data.extend(footer(&[("PORTNAME", "nyan-doom 1.2.1")]));

        let header = DemoHeader::read(&data).unwrap();
        assert_eq!(header.format, DemoFormat::Dsda);
        assert_eq!(header.duration_secs(), Some(10));
        assert_eq!(
            header.footer.unwrap().sourceport.as_deref(),
            Some("nyan-doom 1.2.1")
        );
    }

    #[test]
    fn test_the_footer_names_the_sourceport_and_the_files() {
        let mut data = vanilla(109, 3, 1, 1, 35);
        data.extend(b"\n\n");
        data.extend(footer(&[
            ("FEATURES", "\x01\x02"),
            ("PORTNAME", "dsda-doom 0.29.4"),
            (
                "CMDLINE",
                "-iwad \"doom2.wad\" -file \"scythe.wad\" -complevel 2 ",
            ),
        ]));
        let footer = DemoHeader::read(&data).unwrap().footer.unwrap();
        assert_eq!(footer.sourceport.as_deref(), Some("dsda-doom 0.29.4"));
        assert_eq!(
            footer.command_line.as_deref(),
            Some("-iwad \"doom2.wad\" -file \"scythe.wad\" -complevel 2")
        );
    }

    #[test]
    fn test_a_prboom_um_header_is_stepped_over() {
        let mut data = vec![255];
        data.extend(b"PR+UM\0");
        data.push(1);
        data.extend([1, 0, 8]);
        data.extend(b"UMAPINFO");
        data.extend(b"MAP01\0\0\0");
        data.extend(prboom(214, 1, 35));
        let header = DemoHeader::read(&data).unwrap();
        assert_eq!(header.format.name(), "PrBoom+");
        assert_eq!(header.duration_secs(), Some(1));
    }

    #[test]
    fn test_a_demo_cut_short_has_no_length() {
        let mut data = vanilla(109, 3, 1, 1, 35);
        data.pop();
        let header = DemoHeader::read(&data).unwrap();
        assert_eq!(header.tics, None);
        assert_eq!(header.map_name(false).as_deref(), Some("MAP01"));
    }

    #[test]
    fn test_what_is_not_a_demo_is_none() {
        assert_eq!(DemoHeader::read(b""), None);
        assert_eq!(DemoHeader::read(b"PWAD\0\0\0\0"), None);
        assert_eq!(DemoHeader::read(&[150, 1, 2, 3]), None);
        assert_eq!(DemoHeader::read(&[255, 1, 2, 3]), None);
    }

    /// What uzdoom's own lumps that end in `.lmp` were read as before this was checked
    /// against them: a font, a sprite, an ANIMATED table. Each starts with a byte a Doom 1.2
    /// demo could, and none is one.
    #[test]
    fn test_a_lump_that_starts_like_a_doom_12_demo_is_not_one() {
        // A patch three wide and eight high, its columns' offsets, and some pixels.
        let mut patch = vec![3, 0, 8, 0, 0, 0, 0, 0];
        patch.extend([20, 0, 0, 0, 31, 0, 0, 0, 42, 0, 0, 0]);
        patch.extend([0, 8, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0, 255]);
        assert_eq!(DemoHeader::read(&patch), None);

        // Skill, episode 78, map 85.
        let mut animated = vec![0, 78, 85, 1, 1, 1, 1];
        animated.extend([0u8; 40]);
        assert_eq!(DemoHeader::read(&animated), None);

        // A plausible start, but nothing ends where the tics say it should.
        let mut unended = vec![2, 1, 3, 1, 0, 0, 0];
        unended.extend([0u8; 10]);
        assert_eq!(DemoHeader::read(&unended), None);
    }

    #[test]
    fn test_a_header_with_nobody_playing_is_not_a_demo() {
        let mut nobody = vanilla(109, 3, 1, 1, 1);
        nobody[9] = 0;
        assert_eq!(DemoHeader::read(&nobody), None);
        let mut two = vanilla(109, 3, 1, 1, 0);
        two[10] = 7;
        assert_eq!(DemoHeader::read(&two), None);
    }

    /// What uzdoom's `G_BeginRecording` writes, up to the tics.
    fn zdoom_demo(map: &str, vars: &str, players: usize) -> Vec<u8> {
        fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = id.to_vec();
            out.extend((body.len() as u32).to_be_bytes());
            out.extend(body);
            if body.len() % 2 == 1 {
                out.push(0);
            }
            out
        }
        let mut header = 2u16.to_be_bytes().to_vec();
        header.extend([2, 3]);
        header.extend(map.as_bytes());
        header.push(0);
        header.extend(1234u32.to_be_bytes());
        header.push(0);
        let mut chunks = chunk(b"ZDHD", &header);
        for i in 0..players {
            chunks.extend(chunk(b"UINF", &[i as u8, b'\\', b'n', 0]));
        }
        let mut vars = vars.as_bytes().to_vec();
        vars.push(0);
        chunks.extend(chunk(b"VARS", &vars));
        chunks.extend(chunk(b"BODY", &[0x78, 0x9c, 1, 2, 3]));
        let mut out = b"FORM".to_vec();
        out.extend((4 + chunks.len() as u32).to_be_bytes());
        out.extend(b"ZDEM");
        out.extend(chunks);
        out
    }

    #[test]
    fn test_a_zdoom_demo_names_its_map_and_keeps_the_skill_in_its_cvars() {
        let data = zdoom_demo("map07", "\\sv_cheats\\0\\skill\\3\\teamplay\\0", 1);
        let header = DemoHeader::read(&data).unwrap();
        assert_eq!(header.format, DemoFormat::ZDoom(2));
        assert_eq!(header.format.name(), "ZDoom");
        assert_eq!(header.map_name(true).as_deref(), Some("MAP07"));
        assert_eq!(header.skill_name().as_deref(), Some("UV"));
        assert_eq!(header.players, 1);
        assert_eq!(header.tics, None);
    }

    #[test]
    fn test_a_zdoom_demo_without_a_skill_still_reads() {
        let header = DemoHeader::read(&zdoom_demo("BTW01", "\\teamplay\\0", 2)).unwrap();
        assert_eq!(header.skill, None);
        assert_eq!(header.players, 2);
        assert_eq!(header.map_name(false).as_deref(), Some("BTW01"));
    }
}
