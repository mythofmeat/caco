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
    },
    SourceportFamily {
        name: "zdoom",
        executables: &["gzdoom", "lzdoom", "vkdoom", "qzdoom", "zdoom"],
        data_arg: None,
        save_arg: Some("-savedir"),
        complevel_arg: None,
    },
    SourceportFamily {
        name: "chocolate",
        executables: &["chocolate-doom", "crispy-doom"],
        data_arg: None,
        save_arg: Some("-savedir"),
        complevel_arg: None,
    },
    SourceportFamily {
        name: "woof",
        executables: &["woof"],
        data_arg: Some("-data"),
        save_arg: Some("-save"),
        complevel_arg: Some("-complevel"),
    },
    SourceportFamily {
        name: "eternity",
        executables: &["eternity"],
        data_arg: None,
        save_arg: Some("-savedir"),
        complevel_arg: None,
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
        ])
    });

/// All known save file extensions.
pub static ALL_SAVE_EXTENSIONS: &[&str] = &[".dsg", ".zds"];

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

/// Return CLI args to set the config file for the sourceport.
///
/// Only dsda-family ports support `-config`.
pub fn get_config_args(executable: &str, config_path: &str) -> Vec<String> {
    match family_name(executable) {
        Some("dsda") => vec!["-config".to_string(), config_path.to_string()],
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
        assert_eq!(identify_family("chocolate-doom").unwrap().name, "chocolate");
        assert_eq!(identify_family("woof").unwrap().name, "woof");
        assert_eq!(identify_family("eternity").unwrap().name, "eternity");
        assert!(identify_family("unknown-port").is_none());
    }

    #[test]
    fn test_identify_family_with_path() {
        assert_eq!(
            identify_family("/usr/bin/dsda-doom").unwrap().name,
            "dsda"
        );
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
        assert_eq!(
            get_complevel_args("dsda-doom", 9),
            vec!["-complevel", "9"]
        );
        assert_eq!(
            get_complevel_args("woof", 21),
            vec!["-complevel", "21"]
        );
        assert!(get_complevel_args("gzdoom", 9).is_empty());
        assert!(get_complevel_args("unknown", 9).is_empty());
    }

    #[test]
    fn test_get_config_args() {
        assert_eq!(
            get_config_args("dsda-doom", "/path/to/config.cfg"),
            vec!["-config", "/path/to/config.cfg"]
        );
        assert!(get_config_args("gzdoom", "/path/to/config.cfg").is_empty());
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
        assert_eq!(family_name("unknown"), None);
    }
}
