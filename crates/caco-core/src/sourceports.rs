use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Family definition
// ---------------------------------------------------------------------------

/// A sourceport family and its CLI flag conventions.
#[derive(Debug, Clone)]
pub struct SourceportFamily {
    pub name: &'static str,
    pub executables: &'static [&'static str],
    pub data_arg: Option<&'static str>,
    pub save_arg: Option<&'static str>,
    pub complevel_arg: Option<&'static str>,
    /// Config file extension (e.g. "cfg" or "ini").
    pub config_ext: &'static str,
}

/// All known sourceport families.
pub static FAMILIES: &[SourceportFamily] = &[
    SourceportFamily {
        name: "dsda",
        executables: &[
            "dsda-doom",
            "nyan-doom",
            "nugget-doom",
            "prboom+",
            "prboom-plus",
            "glboom+",
            "glboom-plus",
        ],
        data_arg: Some("-data"),
        save_arg: Some("-save"),
        complevel_arg: Some("-complevel"),
        config_ext: "cfg",
    },
    SourceportFamily {
        name: "zdoom",
        executables: &["uzdoom", "gzdoom", "lzdoom", "vkdoom", "qzdoom", "zdoom"],
        data_arg: None,
        save_arg: Some("-savedir"),
        complevel_arg: None,
        config_ext: "ini",
    },
    SourceportFamily {
        name: "chocolate",
        executables: &["chocolate-doom", "crispy-doom"],
        data_arg: None,
        save_arg: Some("-savedir"),
        complevel_arg: None,
        config_ext: "cfg",
    },
    SourceportFamily {
        name: "woof",
        executables: &["woof"],
        data_arg: Some("-data"),
        save_arg: Some("-save"),
        complevel_arg: Some("-complevel"),
        config_ext: "cfg",
    },
    SourceportFamily {
        name: "eternity",
        executables: &["eternity"],
        data_arg: None,
        save_arg: Some("-savedir"),
        complevel_arg: None,
        config_ext: "cfg",
    },
    SourceportFamily {
        name: "helion",
        executables: &["helion", "Helion"],
        data_arg: None,
        save_arg: Some("-savedir"),
        complevel_arg: None,
        config_ext: "ini",
    },
];

/// Save file extensions by sourceport family.
pub static SAVE_EXTENSIONS: LazyLock<HashMap<&'static str, &'static [&'static str]>> =
    LazyLock::new(|| {
        HashMap::from([
            ("dsda", &[".dsg"][..]),
            ("zdoom", &[".zds"][..]),
            ("chocolate", &[".dsg"][..]),
            ("woof", &[".dsg"][..]),
            ("eternity", &[".dsg"][..]),
            ("helion", &[".hsg"][..]),
        ])
    });

/// All known save file extensions.
pub static ALL_SAVE_EXTENSIONS: &[&str] = &[".dsg", ".zds", ".hsg"];

// Reverse lookup: executable basename -> family
static EXE_MAP: LazyLock<HashMap<&'static str, &'static SourceportFamily>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    for family in FAMILIES {
        for exe in family.executables {
            m.insert(*exe, family);
        }
    }
    m
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Identify a sourceport family from an executable path or name.
///
/// Strips the path to match just the basename (e.g., `/usr/bin/nyan-doom` -> `nyan-doom`).
pub fn identify_family(executable: &str) -> Option<&'static SourceportFamily> {
    let basename = Path::new(executable)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(executable);
    EXE_MAP.get(basename).copied()
}

/// Get the family name string for an executable.
pub fn family_name(executable: &str) -> Option<&'static str> {
    identify_family(executable).map(|f| f.name)
}

/// Detect sourceports installed on the system.
///
/// Returns a list of `(executable_name, full_path, family_name)` for found ports.
pub fn detect_sourceports() -> Vec<(&'static str, String, &'static str)> {
    let path_var = match std::env::var("PATH") {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let mut found = Vec::new();
    for family in FAMILIES {
        for exe in family.executables {
            for dir in path_var.split(':') {
                let candidate = Path::new(dir).join(exe);
                if candidate.is_file() {
                    found.push((*exe, candidate.to_string_lossy().into_owned(), family.name));
                    break;
                }
            }
        }
    }
    found
}

/// Return `true` if this sourceport uses `-deh` for DEH/BEX files.
///
/// ZDoom-family ports load DEH via `-file`; all others use `-deh`.
pub fn uses_deh_flag(executable: &str) -> bool {
    match identify_family(executable) {
        Some(f) => f.name != "zdoom",
        None => true, // safe default
    }
}

/// Return CLI args to set the compatibility level for a sourceport.
///
/// Only dsda and woof families support `-complevel`.
pub fn get_complevel_args(executable: &str, complevel: i32) -> Vec<String> {
    match identify_family(executable) {
        Some(f) => match f.complevel_arg {
            Some(arg) => vec![arg.to_string(), complevel.to_string()],
            None => Vec::new(),
        },
        None => Vec::new(),
    }
}

/// Get the config file extension for a sourceport (e.g. "cfg" or "ini").
pub fn config_ext(executable: &str) -> &'static str {
    identify_family(executable).map_or("cfg", |f| f.config_ext)
}

/// Return CLI args to set the config file for the sourceport.
///
/// dsda, helion, and zdoom-family ports support `-config`.
pub fn get_config_args(executable: &str, config_path: &str) -> Vec<String> {
    match family_name(executable) {
        Some("dsda" | "helion" | "zdoom") => {
            vec!["-config".to_string(), config_path.to_string()]
        }
        _ => Vec::new(),
    }
}

/// Compute the nested save directory for dsda-family sourceports.
///
/// Returns path like: `{data_dir}/{exe}_data/{iwad}/{wad_stem}/`
pub fn get_dsda_save_dir(executable: &str, data_dir: &str, iwad: &str, wad_path: &str) -> String {
    let exe_stem = Path::new(executable)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(executable)
        .replace('-', "_")
        + "_data";
    let wad_stem = Path::new(wad_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let save_dir = Path::new(data_dir).join(exe_stem).join(iwad).join(wad_stem);
    save_dir.to_string_lossy().into_owned()
}

/// Return CLI args to redirect sourceport data/save dirs.
///
/// For dsda-family ports with iwad+wad_path, `-save` points to the nested
/// directory where stats live so saves end up alongside them.
pub fn get_data_dir_args(
    executable: &str,
    data_dir: &str,
    iwad: Option<&str>,
    wad_path: Option<&str>,
) -> Vec<String> {
    let family = match identify_family(executable) {
        Some(f) => f,
        None => return Vec::new(),
    };

    let mut args = Vec::new();

    if let Some(data_arg) = family.data_arg {
        args.push(data_arg.to_string());
        args.push(data_dir.to_string());
    }

    if let Some(save_arg) = family.save_arg {
        let save_dir = if family.name == "dsda" {
            if let (Some(iw), Some(wp)) = (iwad, wad_path) {
                get_dsda_save_dir(executable, data_dir, iw, wp)
            } else {
                data_dir.to_string()
            }
        } else {
            data_dir.to_string()
        };
        args.push(save_arg.to_string());
        args.push(save_dir);
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identify_family() {
        assert_eq!(identify_family("dsda-doom").unwrap().name, "dsda");
        assert_eq!(identify_family("nyan-doom").unwrap().name, "dsda");
        assert_eq!(identify_family("gzdoom").unwrap().name, "zdoom");
        assert_eq!(identify_family("uzdoom").unwrap().name, "zdoom");
        assert_eq!(identify_family("chocolate-doom").unwrap().name, "chocolate");
        assert_eq!(identify_family("woof").unwrap().name, "woof");
        assert_eq!(identify_family("eternity").unwrap().name, "eternity");
        assert_eq!(identify_family("helion").unwrap().name, "helion");
        assert!(identify_family("unknown-port").is_none());
    }

    #[test]
    fn test_identify_family_with_path() {
        assert_eq!(identify_family("/usr/bin/dsda-doom").unwrap().name, "dsda");
        assert_eq!(
            identify_family("/usr/local/bin/gzdoom").unwrap().name,
            "zdoom"
        );
    }

    #[test]
    fn test_uses_deh_flag() {
        assert!(uses_deh_flag("dsda-doom"));
        assert!(uses_deh_flag("woof"));
        assert!(!uses_deh_flag("gzdoom"));
        assert!(uses_deh_flag("unknown-port"));
    }

    #[test]
    fn test_get_complevel_args() {
        assert_eq!(get_complevel_args("dsda-doom", 9), vec!["-complevel", "9"]);
        assert_eq!(get_complevel_args("woof", 21), vec!["-complevel", "21"]);
        assert!(get_complevel_args("gzdoom", 9).is_empty());
        assert!(get_complevel_args("unknown", 9).is_empty());
    }

    #[test]
    fn test_get_config_args() {
        assert_eq!(
            get_config_args("dsda-doom", "/path/to/config.cfg"),
            vec!["-config", "/path/to/config.cfg"]
        );
        assert_eq!(
            get_config_args("helion", "/path/to/config.ini"),
            vec!["-config", "/path/to/config.ini"]
        );
        assert_eq!(
            get_config_args("Helion", "/path/to/config.ini"),
            vec!["-config", "/path/to/config.ini"]
        );
        assert_eq!(
            get_config_args("gzdoom", "/path/to/config.ini"),
            vec!["-config", "/path/to/config.ini"]
        );
        assert!(get_config_args("chocolate-doom", "/path/to/config.cfg").is_empty());
        assert!(get_config_args("unknown-port", "/path/to/config.cfg").is_empty());
    }

    #[test]
    fn test_get_data_dir_args_dsda() {
        let args = get_data_dir_args("dsda-doom", "/data", Some("doom2"), Some("/wads/test.wad"));
        assert_eq!(args[0], "-data");
        assert_eq!(args[1], "/data");
        assert_eq!(args[2], "-save");
        assert!(args[3].contains("dsda_doom_data"));
        assert!(args[3].contains("doom2"));
        assert!(args[3].contains("test"));
    }

    #[test]
    fn test_get_data_dir_args_zdoom() {
        let args = get_data_dir_args("gzdoom", "/data", None, None);
        assert_eq!(args, vec!["-savedir", "/data"]);
    }

    #[test]
    fn test_get_data_dir_args_unknown() {
        assert!(get_data_dir_args("unknown-port", "/data", None, None).is_empty());
    }

    #[test]
    fn test_get_dsda_save_dir() {
        let dir = get_dsda_save_dir("dsda-doom", "/data", "doom2", "/wads/Scythe.wad");
        assert!(dir.contains("dsda_doom_data"));
        assert!(dir.contains("doom2"));
        assert!(dir.contains("scythe"));
    }

    #[test]
    fn test_family_name() {
        assert_eq!(family_name("dsda-doom"), Some("dsda"));
        assert_eq!(family_name("gzdoom"), Some("zdoom"));
        assert_eq!(family_name("uzdoom"), Some("zdoom"));
        assert_eq!(family_name("unknown"), None);
    }

    #[test]
    fn test_identify_all_dsda_executables() {
        for exe in &[
            "dsda-doom",
            "nyan-doom",
            "nugget-doom",
            "prboom+",
            "prboom-plus",
            "glboom+",
            "glboom-plus",
        ] {
            let family = identify_family(exe);
            assert!(family.is_some(), "should identify {}", exe);
            assert_eq!(family.unwrap().name, "dsda");
        }
    }

    #[test]
    fn test_identify_all_zdoom_executables() {
        for exe in &["uzdoom", "gzdoom", "lzdoom", "vkdoom", "qzdoom", "zdoom"] {
            let family = identify_family(exe);
            assert!(family.is_some(), "should identify {}", exe);
            assert_eq!(family.unwrap().name, "zdoom");
        }
    }

    #[test]
    fn test_identify_all_chocolate_executables() {
        for exe in &["chocolate-doom", "crispy-doom"] {
            let family = identify_family(exe);
            assert!(family.is_some(), "should identify {}", exe);
            assert_eq!(family.unwrap().name, "chocolate");
        }
    }

    #[test]
    fn test_identify_empty_string() {
        assert!(identify_family("").is_none());
    }

    #[test]
    fn test_identify_with_deep_path() {
        assert_eq!(
            identify_family("/opt/doom/ports/gzdoom").unwrap().name,
            "zdoom"
        );
    }

    #[test]
    fn test_get_data_dir_args_nyan_doom() {
        let args = get_data_dir_args("nyan-doom", "/data", None, None);
        assert_eq!(args, vec!["-data", "/data", "-save", "/data"]);
    }

    #[test]
    fn test_get_data_dir_args_chocolate() {
        let args = get_data_dir_args("crispy-doom", "/data", None, None);
        assert_eq!(args, vec!["-savedir", "/data"]);
    }

    #[test]
    fn test_get_data_dir_args_woof() {
        let args = get_data_dir_args("woof", "/data", None, None);
        assert_eq!(args, vec!["-data", "/data", "-save", "/data"]);
    }

    #[test]
    fn test_get_data_dir_args_eternity() {
        let args = get_data_dir_args("eternity", "/data", None, None);
        assert_eq!(args, vec!["-savedir", "/data"]);
    }

    #[test]
    fn test_get_data_dir_args_with_full_path() {
        let args = get_data_dir_args("/usr/bin/nyan-doom", "/data", None, None);
        assert_eq!(args, vec!["-data", "/data", "-save", "/data"]);
    }

    #[test]
    fn test_get_data_dir_args_dsda_without_iwad() {
        // Without iwad, dsda falls back to same dir for -save
        let args = get_data_dir_args("dsda-doom", "/data", None, Some("/wads/test.wad"));
        assert_eq!(args, vec!["-data", "/data", "-save", "/data"]);
    }

    #[test]
    fn test_get_data_dir_args_dsda_without_wad_path() {
        // Without wad_path, dsda falls back to same dir for -save
        let args = get_data_dir_args("dsda-doom", "/data", Some("doom2"), None);
        assert_eq!(args, vec!["-data", "/data", "-save", "/data"]);
    }

    #[test]
    fn test_get_data_dir_args_woof_ignores_iwad_wad_path() {
        // Woof has -data/-save but does NOT use nested save dir
        let args = get_data_dir_args("woof", "/data", Some("doom2"), Some("/wads/test.wad"));
        assert_eq!(args, vec!["-data", "/data", "-save", "/data"]);
    }

    #[test]
    fn test_get_data_dir_args_zdoom_ignores_iwad_wad_path() {
        let args = get_data_dir_args("gzdoom", "/data", Some("doom2"), Some("/wads/test.wad"));
        assert_eq!(args, vec!["-savedir", "/data"]);
    }

    #[test]
    fn test_get_complevel_args_nyan_doom() {
        assert_eq!(
            get_complevel_args("nyan-doom", 21),
            vec!["-complevel", "21"]
        );
    }

    #[test]
    fn test_get_complevel_args_chocolate_unsupported() {
        assert!(get_complevel_args("chocolate-doom", 2).is_empty());
    }

    #[test]
    fn test_get_complevel_args_eternity_unsupported() {
        assert!(get_complevel_args("eternity", 11).is_empty());
    }

    #[test]
    fn test_get_complevel_args_with_full_path() {
        assert_eq!(
            get_complevel_args("/usr/bin/dsda-doom", 21),
            vec!["-complevel", "21"]
        );
    }

    #[test]
    fn test_get_config_args_nyan_doom() {
        // nyan-doom is dsda family, should support -config
        assert_eq!(
            get_config_args("nyan-doom", "/path/to/config.cfg"),
            vec!["-config", "/path/to/config.cfg"]
        );
    }

    #[test]
    fn test_get_config_args_woof() {
        // woof is not dsda family, should not support -config
        assert!(get_config_args("woof", "/path/to/config.cfg").is_empty());
    }

    #[test]
    fn test_get_config_args_eternity() {
        assert!(get_config_args("eternity", "/path/to/config.cfg").is_empty());
    }

    #[test]
    fn test_get_config_args_unknown() {
        assert!(get_config_args("unknown-port", "/path/to/config.cfg").is_empty());
    }

    #[test]
    fn test_get_dsda_save_dir_nyan() {
        let dir = get_dsda_save_dir("nyan-doom", "/data", "tnt", "/wads/test.wad");
        assert!(dir.contains("nyan_doom_data"));
        assert!(dir.contains("tnt"));
        assert!(dir.contains("test"));
    }

    #[test]
    fn test_get_dsda_save_dir_full_path() {
        let dir = get_dsda_save_dir("/usr/bin/nyan-doom", "/data", "doom2", "/wads/test.wad");
        assert!(dir.contains("nyan_doom_data"));
        assert!(dir.contains("doom2"));
    }

    #[test]
    fn test_get_dsda_save_dir_wad_stem_lowercase() {
        let dir = get_dsda_save_dir("dsda-doom", "/data", "doom2", "/wads/MyWad.wad");
        assert!(dir.contains("mywad"));
    }

    #[test]
    fn test_uses_deh_flag_all_families() {
        // dsda family: uses -deh
        assert!(uses_deh_flag("dsda-doom"));
        assert!(uses_deh_flag("nyan-doom"));
        // zdoom family: uses -file instead
        assert!(!uses_deh_flag("gzdoom"));
        assert!(!uses_deh_flag("lzdoom"));
        assert!(!uses_deh_flag("vkdoom"));
        // Other families: uses -deh
        assert!(uses_deh_flag("chocolate-doom"));
        assert!(uses_deh_flag("woof"));
        assert!(uses_deh_flag("eternity"));
    }

    #[test]
    fn test_save_extensions() {
        assert_eq!(SAVE_EXTENSIONS.get("dsda").unwrap(), &[".dsg"]);
        assert_eq!(SAVE_EXTENSIONS.get("zdoom").unwrap(), &[".zds"]);
        assert_eq!(SAVE_EXTENSIONS.get("woof").unwrap(), &[".dsg"]);
    }

    #[test]
    fn test_all_save_extensions() {
        assert!(ALL_SAVE_EXTENSIONS.contains(&".dsg"));
        assert!(ALL_SAVE_EXTENSIONS.contains(&".zds"));
    }

    #[test]
    fn test_config_ext() {
        assert_eq!(config_ext("dsda-doom"), "cfg");
        assert_eq!(config_ext("gzdoom"), "ini");
        assert_eq!(config_ext("uzdoom"), "ini");
        assert_eq!(config_ext("zdoom"), "ini");
        assert_eq!(config_ext("helion"), "ini");
        assert_eq!(config_ext("unknown-port"), "cfg");
    }

    #[test]
    fn test_get_config_args_zdoom_family() {
        assert_eq!(
            get_config_args("gzdoom", "/tmp/p.ini"),
            vec!["-config", "/tmp/p.ini"],
        );
        assert_eq!(
            get_config_args("uzdoom", "/tmp/p.ini"),
            vec!["-config", "/tmp/p.ini"],
        );
        assert_eq!(
            get_config_args("zdoom", "/tmp/p.ini"),
            vec!["-config", "/tmp/p.ini"],
        );
    }

    #[test]
    fn test_helion_family() {
        let family = identify_family("helion").unwrap();
        assert_eq!(family.name, "helion");
        assert!(family.data_arg.is_none());
        assert_eq!(family.save_arg, Some("-savedir"));
        assert!(family.complevel_arg.is_none());
        assert_eq!(family.config_ext, "ini");
    }

    #[test]
    fn test_helion_uses_deh_flag() {
        assert!(uses_deh_flag("helion"));
    }

    #[test]
    fn test_helion_data_dir_args() {
        let args = get_data_dir_args("helion", "/data", None, None);
        assert_eq!(args, vec!["-savedir", "/data"]);
    }

    #[test]
    fn test_helion_save_extensions() {
        assert_eq!(SAVE_EXTENSIONS.get("helion").unwrap(), &[".hsg"]);
        assert!(ALL_SAVE_EXTENSIONS.contains(&".hsg"));
    }
}
