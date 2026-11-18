//! A WAD's demos in one zip, for somebody else to play: each demo, a text file saying what it
//! needs, caco's record of it, and if asked, the WAD's own files.
//!
//! A demo on its own is a list of inputs, and the first thing anyone who is sent one asks is
//! what it was recorded with. So each goes with a text file beside it, as speedrun archives
//! have always shipped them, naming the WAD (where to get it and its hash), the IWAD, the
//! sourceport and its build, the complevel and anything else that was loaded, and ending with
//! the command line that plays it back. caco's own record goes too, so another caco that adds
//! the demo from the zip plays it as it was recorded.
//!
//! The WAD's files are optional: the WAD, its companions and caco's stats reporter where the
//! demo was recorded with it. IWADs and id24 resources never are, since they are id
//! Software's to give away, and the text file names them instead.
//!
//! Planning is separate from writing ([`plan_export`], [`write_export`]), as Clean's is: the
//! plan reads the library and the demos, which is quick, and writing copies the WAD, which can
//! be hundreds of megabytes and belongs on a worker.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::{DemoFile, FileRole, record_path};
use crate::db::{self, WadRecord};
use crate::identity;
use crate::sourceports;

/// What goes in the zip, and what was wanted and isn't here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPlan {
    /// By the name each has in the zip, in order: each demo with its text and record, then the
    /// WAD's files.
    pub entries: Vec<ExportEntry>,
    /// How many demos.
    pub demos: usize,
    /// The WAD's files it has, by name.
    pub wad_files: Vec<String>,
    /// The WAD's files it was asked for and couldn't have, since they aren't on this machine.
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEntry {
    pub name: String,
    pub source: EntrySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntrySource {
    File(PathBuf),
    Text(String),
}

impl ExportPlan {
    /// How big the WAD's files are, to say what including them costs.
    pub fn wad_files_size(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| self.wad_files.contains(&e.name))
            .filter_map(|e| match &e.source {
                EntrySource::File(path) => path.metadata().ok().map(|m| m.len()),
                EntrySource::Text(_) => None,
            })
            .sum()
    }
}

/// What a zip of these demos would hold. `episode_names` says how to read a header's map
/// numbers: see [`super::uses_episode_names`].
pub fn plan_export(
    conn: &Connection,
    wad_id: i64,
    demos: &[DemoFile],
    with_wads: bool,
    episode_names: bool,
) -> crate::Result<ExportPlan> {
    let wad = db::get_wad(conn, wad_id, false)?.ok_or(crate::Error::WadNotFound(wad_id))?;
    let inner = identity::inner_files(conn, wad_id)?;
    let loads = wad_loads(&wad, &inner);

    let mut plan = ExportPlan {
        entries: Vec::new(),
        demos: demos.len(),
        wad_files: Vec::new(),
        missing: Vec::new(),
    };
    let mut names = HashSet::new();
    let mut add = |plan: &mut ExportPlan, name: String, source: EntrySource| -> String {
        let name = free_name(&mut names, &name);
        plan.entries.push(ExportEntry {
            name: name.clone(),
            source,
        });
        name
    };

    for demo in demos {
        let stem = Path::new(&demo.name)
            .file_stem()
            .map_or_else(|| demo.name.clone(), |s| s.to_string_lossy().into_owned());
        let name = add(
            &mut plan,
            demo.name.clone(),
            EntrySource::File(demo.path.clone()),
        );
        let text = describe(&wad, demo, &name, &loads, episode_names);
        add(&mut plan, format!("{stem}.txt"), EntrySource::Text(text));
        let record = record_path(&demo.path);
        if demo.record.is_some() && record.is_file() {
            add(&mut plan, format!("{name}.toml"), EntrySource::File(record));
        }
    }

    if with_wads {
        for (name, found) in wad_files(conn, &wad, demos)? {
            match found {
                Some(path) => {
                    let name = add(&mut plan, name, EntrySource::File(path));
                    plan.wad_files.push(name);
                }
                None => plan.missing.push(name),
            }
        }
    }
    Ok(plan)
}

/// The WAD's files a demo needs, by name, and where each is if it is here: the WAD's own file,
/// then whatever else was loaded with it, by the records where there are any and by what the
/// WAD loads now where there aren't. IWADs and id24 resources are left out on purpose.
fn wad_files(
    conn: &Connection,
    wad: &WadRecord,
    demos: &[DemoFile],
) -> crate::Result<Vec<(String, Option<PathBuf>)>> {
    let mut files = Vec::new();
    let own_name = wad
        .cached_path
        .as_deref()
        .and_then(|p| Path::new(p).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .or_else(|| wad.filename.clone())
        .unwrap_or_else(|| wad.title.clone());
    let own = wad
        .cached_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    files.push((own_name, own));

    let mut seen = HashSet::new();
    let mut companion = |name: String, md5: &str| -> crate::Result<()> {
        if !seen.insert(md5.to_ascii_lowercase()) {
            return Ok(());
        }
        let found = db::find_companion_by_md5(conn, md5)?
            .map(|c| PathBuf::from(c.path))
            .filter(|p| p.is_file());
        files.push((name, found));
        Ok(())
    };
    let mut reporter = false;
    for demo in demos {
        match &demo.record {
            Some(record) => {
                for file in &record.files {
                    match (file.role, &file.md5) {
                        (FileRole::Companion, Some(md5)) => companion(file.name.clone(), md5)?,
                        (FileRole::StatsReporter, _) => reporter = true,
                        _ => {}
                    }
                }
            }
            None => {
                for comp in db::get_companions_for_wad(conn, wad.id)? {
                    if comp.enabled {
                        companion(comp.filename, &comp.md5)?;
                    }
                }
            }
        }
    }
    if reporter {
        let path = crate::stats_watcher::get_stats_mod_path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        files.push((name, Some(path).filter(|p| p.is_file())));
    }
    Ok(files)
}

/// What a sourceport is handed as the WAD, by name: what its zip wraps, in the order caco and
/// nyan-doom load them, or the file itself.
fn wad_loads(wad: &WadRecord, inner: &[identity::InnerFile]) -> Vec<String> {
    let is_container = wad.identity.as_ref().is_some_and(|i| i.is_container());
    if is_container && !inner.is_empty() {
        let mut names: Vec<&str> = inner.iter().map(|f| f.path.as_str()).collect();
        names.sort_by_key(|name| crate::unpack::load_order(name));
        return names
            .into_iter()
            .map(|name| identity::base_name(name).to_string())
            .collect();
    }
    let file = wad
        .cached_path
        .as_deref()
        .and_then(|p| Path::new(p).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .or_else(|| wad.filename.clone());
    file.into_iter().collect()
}

/// A name nothing in the zip has yet: `name`, or `name-2` before its extension.
fn free_name(taken: &mut HashSet<String>, name: &str) -> String {
    let path = Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    let mut candidate = name.to_string();
    let mut n = 2;
    while !taken.insert(candidate.to_lowercase()) {
        candidate = match (&stem, &ext) {
            (Some(stem), Some(ext)) => format!("{stem}-{n}.{ext}"),
            _ => format!("{name}-{n}"),
        };
        n += 1;
    }
    candidate
}

/// The text file that goes beside a demo: what it is, what it needs, and how to play it.
fn describe(
    wad: &WadRecord,
    demo: &DemoFile,
    zipped_as: &str,
    loads: &[String],
    episode_names: bool,
) -> String {
    let header = demo.header.as_ref();
    let record = demo.record.as_ref();
    let mut lines: Vec<(&str, String)> = Vec::new();

    if let Some(map) = header.and_then(|h| h.map_name(episode_names)) {
        lines.push(("Map", map));
    }
    if let Some(skill) = header.and_then(|h| h.skill_name()) {
        lines.push(("Skill", skill));
    }
    if let Some(secs) = header.and_then(|h| h.duration_secs()) {
        lines.push(("Length", crate::player::format_duration(i64::from(secs))));
    }
    if let Some(header) = header {
        let format = match header.players {
            0 | 1 => header.format.name(),
            n => format!("{}, {n} players", header.format.name()),
        };
        lines.push(("Format", format));
    }
    if let Some(at) = record.and_then(|r| r.recorded_at.get(..10)) {
        lines.push(("Recorded", at.to_string()));
    }
    if let Some(sourceport) = demo.sourceport() {
        let built = record
            .and_then(|r| r.build.as_ref())
            .map(|b| format!(" (commit {})", b.commit.get(..12).unwrap_or(&b.commit)))
            .unwrap_or_default();
        lines.push(("Sourceport", format!("{sourceport}{built}")));
    }
    if let Some(complevel) = record.and_then(|r| r.complevel) {
        lines.push(("Complevel", complevel.to_string()));
    }
    if let Some(iwad) = record.and_then(|r| r.iwad.as_ref()) {
        let hash = iwad
            .sha256
            .as_deref()
            .map(|h| format!(", sha256 {h}"))
            .unwrap_or_default();
        lines.push(("IWAD", format!("{}{hash}", iwad_file(&iwad.name))));
    }

    let mut wad_lines = Vec::new();
    if let Some(name) = wad
        .cached_path
        .as_deref()
        .and_then(|p| Path::new(p).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .or_else(|| wad.filename.clone())
    {
        wad_lines.push(name);
    }
    if let Some(url) = wad.source_url.as_deref().filter(|u| u.starts_with("http")) {
        wad_lines.push(url.to_string());
    }
    if let Some(identity) = &wad.identity {
        wad_lines.push(format!("sha256 {}", identity.file.sha256));
        if identity.is_container() && !loads.is_empty() {
            wad_lines.push(format!("which holds {}", loads.join(", ")));
        }
    }
    for (i, line) in wad_lines.into_iter().enumerate() {
        lines.push((if i == 0 { "WAD" } else { "" }, line));
    }

    let also: Vec<String> = record
        .map(|r| {
            r.files
                .iter()
                .filter(|f| f.role != FileRole::Wad)
                .map(|f| match (f.role, &f.md5, &f.sha256) {
                    (FileRole::Id24, ..) => format!("{} (id24)", iwad_file(&f.name)),
                    (_, Some(md5), _) => format!("{}, md5 {md5}", f.name),
                    (_, _, Some(sha256)) => format!("{}, sha256 {sha256}", f.name),
                    _ => f.name.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    for (i, line) in also.into_iter().enumerate() {
        lines.push((if i == 0 { "Also loaded" } else { "" }, line));
    }
    if let Some(record) = record {
        let args: Vec<&str> = record
            .args
            .iter()
            .chain(&record.custom_args)
            .map(String::as_str)
            .collect();
        if !args.is_empty() {
            lines.push(("Arguments", args.join(" ")));
        }
    }

    let width = lines
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0)
        + 2;
    let mut text = String::new();
    let by = wad
        .author
        .as_deref()
        .filter(|a| !a.is_empty())
        .map(|a| format!(" by {a}"))
        .unwrap_or_default();
    let _ = writeln!(text, "{zipped_as}: a demo of {}{by}", wad.title);
    let _ = writeln!(text);
    for (label, value) in &lines {
        let _ = writeln!(text, "{label:width$}{value}");
    }

    let command = match record {
        Some(record) => Some(playback_command(record, loads, zipped_as)),
        // What a dsda-family sourceport said it was recorded with is as good as there is.
        None => header
            .and_then(|h| h.footer.as_ref())
            .and_then(|f| f.command_line.as_deref())
            .map(|recorded| format!("{recorded} -playdemo {}", quoted(zipped_as))),
    };
    if let Some(command) = command {
        let _ = writeln!(text);
        let _ = writeln!(text, "To play it back:");
        let _ = writeln!(text, "{command}");
    }
    let _ = writeln!(text);
    let _ = writeln!(
        text,
        "Exported by caco. IWADs and id24 resources aren't included: they're id Software's to give."
    );
    text
}

/// What a recipient runs to play the demo back, by file names they will have.
fn playback_command(record: &super::DemoRecord, loads: &[String], demo: &str) -> String {
    let sourceport = record.sourceport.as_str();
    let mut args: Vec<String> = vec![sourceport.to_string()];
    if let Some(iwad) = &record.iwad {
        args.extend(["-iwad".to_string(), iwad_file(&iwad.name)]);
    }
    if let Some(cl) = record.complevel {
        args.extend(sourceports::get_complevel_args(sourceport, cl));
    }
    let mut deh = Vec::new();
    let mut files = Vec::new();
    for file in &record.files {
        let names = match file.role {
            FileRole::Wad => loads.to_vec(),
            FileRole::Id24 => vec![iwad_file(&file.name)],
            _ => vec![file.name.clone()],
        };
        for name in names {
            if crate::companion_service::is_deh_bex(Path::new(&name))
                && sourceports::uses_deh_flag(sourceport)
            {
                deh.extend(["-deh".to_string(), name]);
            } else {
                files.push(name);
            }
        }
    }
    args.extend(deh);
    if !files.is_empty() {
        args.push("-file".to_string());
        args.extend(files);
    }
    args.extend(record.args.iter().chain(&record.custom_args).cloned());
    args.extend(["-playdemo".to_string(), demo.to_string()]);
    args.iter().map(|a| quoted(a)).collect::<Vec<_>>().join(" ")
}

/// An IWAD or id24 WAD as a file name: what it was called, with `.wad` if that had none.
fn iwad_file(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string());
    if Path::new(&base).extension().is_some() {
        base
    } else {
        format!("{base}.wad")
    }
}

/// An argument as a shell would need it.
fn quoted(arg: &str) -> String {
    if arg.is_empty() || arg.contains(|c: char| c.is_whitespace() || "\"'$`\\".contains(c)) {
        format!("\"{}\"", arg.replace(['"', '\\'], ""))
    } else {
        arg.to_string()
    }
}

/// Write the zip a plan describes to `dest`, through a temporary name beside it, so a zip that
/// is there is whole. Returns its size.
pub fn write_export(plan: &ExportPlan, dest: &Path) -> crate::Result<u64> {
    use zip::write::SimpleFileOptions;

    let mut partial = dest.as_os_str().to_os_string();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    let written = (|| -> crate::Result<()> {
        let mut zip = zip::ZipWriter::new(File::create(&partial)?);
        for entry in &plan.entries {
            // An archive in an archive gains nothing from being squeezed again.
            let already = matches!(
                Path::new(&entry.name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("zip" | "pk3" | "pk7" | "7z" | "ipk3")
            );
            let method = if already {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            };
            let options = SimpleFileOptions::default()
                .compression_method(method)
                .large_file(true);
            zip.start_file(entry.name.as_str(), options)
                .map_err(|e| crate::Error::Config(format!("cannot write {}: {e}", entry.name)))?;
            match &entry.source {
                EntrySource::File(path) => {
                    io::copy(&mut File::open(path)?, &mut zip)?;
                }
                EntrySource::Text(text) => {
                    io::Write::write_all(&mut zip, text.as_bytes())?;
                }
            }
        }
        zip.finish()
            .map_err(|e| crate::Error::Config(format!("cannot finish the zip: {e}")))?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&partial);
        return Err(e);
    }
    fs::rename(&partial, dest)?;
    Ok(dest.metadata()?.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{NewWad, SourceType, WadUpdate};
    use crate::demos::record::{RECORD_VERSION, RecordedBuild, RecordedFile, RecordedIwad};
    use crate::demos::{DemoHeader, DemoRecord, write_record};

    fn record() -> DemoRecord {
        DemoRecord {
            version: RECORD_VERSION,
            session: None,
            recorded_at: "2026-11-18T09:30:00+00:00".to_string(),
            sourceport: "nyan-doom".to_string(),
            build: Some(RecordedBuild {
                git_ref: "v1.6.2".to_string(),
                commit: "fd7fb8778524cd3540c1b89199d4bc23".to_string(),
            }),
            iwad: Some(RecordedIwad {
                name: "doom2".to_string(),
                sha256: Some("ab".repeat(32)),
            }),
            complevel: Some(9),
            profile: "default".to_string(),
            args: vec!["-nomusic".to_string()],
            custom_args: vec!["-fast".to_string()],
            exited: None,
            files: vec![
                RecordedFile {
                    role: FileRole::Companion,
                    name: "fix.deh".to_string(),
                    sha256: None,
                    md5: Some("f1x".to_string()),
                },
                RecordedFile {
                    role: FileRole::Companion,
                    name: "gone.wad".to_string(),
                    sha256: None,
                    md5: Some("g0ne".to_string()),
                },
                RecordedFile {
                    role: FileRole::Wad,
                    name: "scythe.zip".to_string(),
                    sha256: Some("cd".repeat(32)),
                    md5: None,
                },
            ],
        }
    }

    /// Doom 1.9, UV, MAP07, a second.
    fn vanilla() -> Vec<u8> {
        let mut data = vec![109, 3, 1, 7, 0, 0, 0, 0, 0, 1, 0, 0, 0];
        data.extend([0u8; 4 * 35]);
        data.push(0x80);
        data
    }

    struct Library {
        conn: Connection,
        wad_id: i64,
        demos: Vec<DemoFile>,
        _dir: tempfile::TempDir,
    }

    /// A WAD whose file is a zip holding a WAD and a DEH, a companion that is here and one
    /// that isn't, and two demos, only one of which caco recorded.
    fn library() -> Library {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_memory().unwrap();
        db::init_db(&conn).unwrap();

        let zip_path = dir.path().join("scythe.zip");
        let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        for (name, data) in [
            ("SCYTHE.WAD", &b"PWAD scythe"[..]),
            ("scythe.deh", b"Patch"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            io::Write::write_all(&mut zip, data).unwrap();
        }
        zip.finish().unwrap();

        let wad_id = db::add_wad(
            &conn,
            &NewWad::new("Scythe", SourceType::Idgames)
                .source_url("https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/scythe")
                .cached_path(zip_path.display().to_string()),
        )
        .unwrap();
        db::update_wad(
            &conn,
            wad_id,
            &WadUpdate::new().set_text("author", Some("Erik Alm".to_string())),
        )
        .unwrap();
        let found = identity::identify(&zip_path).unwrap();
        identity::record(&conn, wad_id, &zip_path, &found).unwrap();

        let deh = dir.path().join("fix.deh");
        fs::write(&deh, b"Patch File").unwrap();
        db::add_companion(&conn, "f1x", "fix.deh", &deh.display().to_string(), 10).unwrap();

        let demos_dir = dir.path().join("demos");
        fs::create_dir_all(&demos_dir).unwrap();
        let recorded = demos_dir.join("map07-uv.lmp");
        fs::write(&recorded, vanilla()).unwrap();
        write_record(&recorded, &record()).unwrap();
        let other = demos_dir.join("from-elsewhere.lmp");
        fs::write(&other, vanilla()).unwrap();

        let demos = super::super::find_demo_files(dir.path());
        Library {
            conn,
            wad_id,
            demos,
            _dir: dir,
        }
    }

    fn names(plan: &ExportPlan) -> Vec<&str> {
        plan.entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn test_each_demo_goes_with_what_it_needs_and_the_wad_only_when_asked() {
        let lib = library();
        let plan = plan_export(&lib.conn, lib.wad_id, &lib.demos, false, false).unwrap();
        assert_eq!(
            names(&plan),
            [
                "from-elsewhere.lmp",
                "from-elsewhere.txt",
                "map07-uv.lmp",
                "map07-uv.txt",
                "map07-uv.lmp.toml",
            ]
        );
        assert_eq!(plan.demos, 2);
        assert!(plan.wad_files.is_empty() && plan.missing.is_empty());

        let plan = plan_export(&lib.conn, lib.wad_id, &lib.demos, true, false).unwrap();
        assert_eq!(&names(&plan)[5..], ["scythe.zip", "fix.deh"]);
        assert_eq!(plan.wad_files, ["scythe.zip", "fix.deh"]);
        assert_eq!(plan.missing, ["gone.wad"]);
        assert!(plan.wad_files_size() > 0);
    }

    #[test]
    fn test_the_text_says_what_it_needs_and_how_to_play_it() {
        let lib = library();
        let plan = plan_export(&lib.conn, lib.wad_id, &lib.demos[1..], false, false).unwrap();
        let EntrySource::Text(text) = &plan.entries[1].source else {
            panic!("no text");
        };
        for expected in [
            "map07-uv.lmp: a demo of Scythe by Erik Alm",
            "Map          MAP07",
            "Skill        UV",
            "Sourceport   nyan-doom v1.6.2 (commit fd7fb8778524)",
            "Complevel    9",
            "IWAD         doom2.wad, sha256 abab",
            "https://www.doomworld.com/idgames/levels/doom2/Ports/megawads/scythe",
            "which holds SCYTHE.WAD, scythe.deh",
            "Also loaded  fix.deh, md5 f1x",
            "Arguments    -nomusic -fast",
            "nyan-doom -iwad doom2.wad -complevel 9 -deh fix.deh -deh scythe.deh -file \
             gone.wad SCYTHE.WAD -nomusic -fast -playdemo map07-uv.lmp",
            "IWADs and id24 resources aren't included",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
        }
    }

    #[test]
    fn test_a_demo_caco_did_not_record_goes_by_what_its_footer_says() {
        let lib = library();
        let mut demo = lib.demos[0].clone();
        let mut header = DemoHeader::read(&vanilla()).unwrap();
        header.footer = Some(crate::demos::header::DemoFooter {
            sourceport: Some("dsda-doom 0.29.4".to_string()),
            command_line: Some("-iwad \"doom2.wad\" -complevel 2".to_string()),
        });
        demo.header = Some(header);
        let text = describe(
            &db::get_wad(&lib.conn, lib.wad_id, false).unwrap().unwrap(),
            &demo,
            "from-elsewhere.lmp",
            &["SCYTHE.WAD".to_string()],
            false,
        );
        assert!(text.contains("Sourceport  dsda-doom 0.29.4"), "{text}");
        assert!(
            text.contains("-iwad \"doom2.wad\" -complevel 2 -playdemo from-elsewhere.lmp"),
            "{text}"
        );
    }

    #[test]
    fn test_the_zip_holds_what_the_plan_says_and_nothing_half_written() {
        let lib = library();
        let plan = plan_export(&lib.conn, lib.wad_id, &lib.demos, true, false).unwrap();
        let out = tempfile::tempdir().unwrap();
        let dest = out.path().join("scythe-demos.zip");
        let size = write_export(&plan, &dest).unwrap();
        assert_eq!(size, dest.metadata().unwrap().len());
        assert!(!out.path().join("scythe-demos.zip.part").exists());

        let mut zip = zip::ZipArchive::new(File::open(&dest).unwrap()).unwrap();
        let mut in_zip: Vec<String> = zip
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect();
        in_zip.sort();
        let mut planned: Vec<String> = names(&plan).into_iter().map(str::to_string).collect();
        planned.sort();
        assert_eq!(in_zip, planned);
        let mut demo = Vec::new();
        io::Read::read_to_end(&mut zip.by_name("map07-uv.lmp").unwrap(), &mut demo).unwrap();
        assert_eq!(demo, vanilla());

        // A file that has gone since the plan leaves no zip, whole or partial.
        let mut broken = plan.clone();
        broken.entries.push(ExportEntry {
            name: "late.wad".to_string(),
            source: EntrySource::File(out.path().join("not-there.wad")),
        });
        let dest = out.path().join("broken.zip");
        assert!(write_export(&broken, &dest).is_err());
        assert!(!dest.exists());
        assert!(!out.path().join("broken.zip.part").exists());
    }

    #[test]
    fn test_two_files_of_one_name_are_both_kept() {
        let mut taken = HashSet::new();
        assert_eq!(free_name(&mut taken, "fix.deh"), "fix.deh");
        assert_eq!(free_name(&mut taken, "FIX.deh"), "FIX-2.deh");
        assert_eq!(free_name(&mut taken, "fix.deh"), "fix-3.deh");
        assert_eq!(free_name(&mut taken, "README"), "README");
        assert_eq!(free_name(&mut taken, "README"), "README-2");
    }

    #[test]
    fn test_a_name_with_spaces_is_quoted_for_a_shell() {
        assert_eq!(quoted("map01 uv.lmp"), "\"map01 uv.lmp\"");
        assert_eq!(quoted("doom2.wad"), "doom2.wad");
        assert_eq!(iwad_file("doom2"), "doom2.wad");
        assert_eq!(iwad_file("/iwads/DOOM2.WAD"), "DOOM2.WAD");
    }
}
