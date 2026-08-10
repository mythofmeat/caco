//! Answer "can this machine build this port?" before a build spends minutes
//! finding out that it cannot.
//!
//! Reporting only — caco never installs system packages on the user's behalf.
//! It prints the exact command so the user runs it themselves and their
//! package manager stays the only thing that has touched the system.

use std::process::Command;

use super::recipe::PortRecipe;

/// Which package manager caco was able to ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageCheck {
    /// Packages were checked against `pacman -T` or `brew list`.
    Checked,
    /// No package list is known for this platform, or the package manager
    /// caco knows about is not installed. Deps are reported unverified.
    Unsupported(String),
}

/// What a machine is missing before it can build a recipe.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub port: String,
    /// Build tools absent from PATH. Hard blockers.
    pub missing_tools: Vec<String>,
    /// System packages the recipe lists that are not installed.
    pub missing_packages: Vec<String>,
    pub package_check: PackageCheck,
}

impl DoctorReport {
    /// Whether a build is worth attempting.
    ///
    /// Missing packages do not block on their own: caco cannot tell an
    /// optional dependency from a required one (uzdoom builds fine without
    /// `waylandpp`), so it reports them and lets cmake be the authority.
    pub fn can_build(&self) -> bool {
        self.missing_tools.is_empty()
    }

    /// The command that would install everything missing, if caco knows one.
    pub fn install_hint(&self) -> Option<String> {
        if self.missing_packages.is_empty() && self.missing_tools.is_empty() {
            return None;
        }
        let mut all: Vec<&str> = self.missing_packages.iter().map(String::as_str).collect();
        for tool in &self.missing_tools {
            if !all.contains(&tool.as_str()) {
                all.push(tool);
            }
        }
        if cfg!(target_os = "macos") {
            Some(format!("brew install {}", all.join(" ")))
        } else if cfg!(target_os = "linux") && which_exists("pacman") {
            Some(format!("sudo pacman -S --needed {}", all.join(" ")))
        } else {
            None
        }
    }
}

/// Check tools and system packages for one recipe.
pub fn doctor(recipe: &PortRecipe) -> DoctorReport {
    let missing_tools = super::build::required_tools(recipe)
        .into_iter()
        .filter(|t| crate::config::which(t).is_none())
        .collect();

    let deps = recipe.deps.for_host();
    let (missing_packages, package_check) = check_packages(deps);

    DoctorReport {
        port: recipe.name.clone(),
        missing_tools,
        missing_packages,
        package_check,
    }
}

fn which_exists(name: &str) -> bool {
    crate::config::which(name).is_some()
}

/// Ask the host package manager which of `deps` are not installed.
fn check_packages(deps: &[String]) -> (Vec<String>, PackageCheck) {
    if deps.is_empty() {
        return (
            Vec::new(),
            PackageCheck::Unsupported("no dependency list for this platform".to_string()),
        );
    }
    if cfg!(target_os = "macos") {
        return match brew_installed() {
            Some(installed) => (
                deps.iter()
                    .filter(|d| !installed.iter().any(|i| i == *d))
                    .cloned()
                    .collect(),
                PackageCheck::Checked,
            ),
            None => (
                Vec::new(),
                PackageCheck::Unsupported("brew is not installed".to_string()),
            ),
        };
    }
    if cfg!(target_os = "linux") {
        if !which_exists("pacman") {
            return (
                Vec::new(),
                PackageCheck::Unsupported(
                    "package names are Arch's; check them against your distribution".to_string(),
                ),
            );
        }
        return match pacman_missing(deps) {
            Some(missing) => (missing, PackageCheck::Checked),
            None => (
                Vec::new(),
                PackageCheck::Unsupported("pacman -T failed".to_string()),
            ),
        };
    }
    (
        Vec::new(),
        PackageCheck::Unsupported("unsupported platform".to_string()),
    )
}

/// `pacman -T` is the "which of these are unsatisfied" query: it prints the
/// unmet ones and exits non-zero, so a clean exit means everything is present.
fn pacman_missing(deps: &[String]) -> Option<Vec<String>> {
    let out = Command::new("pacman").arg("-T").args(deps).output().ok()?;
    Some(parse_pacman_output(&String::from_utf8_lossy(&out.stdout)))
}

fn parse_pacman_output(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn brew_installed() -> Option<Vec<String>> {
    let out = Command::new("brew")
        .args(["list", "--formula", "-1"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_pacman_output(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(missing_tools: Vec<&str>, missing_packages: Vec<&str>) -> DoctorReport {
        DoctorReport {
            port: "uzdoom".to_string(),
            missing_tools: missing_tools.into_iter().map(String::from).collect(),
            missing_packages: missing_packages.into_iter().map(String::from).collect(),
            package_check: PackageCheck::Checked,
        }
    }

    #[test]
    fn missing_packages_alone_do_not_block_a_build() {
        // waylandpp is optional for uzdoom; cmake is the authority on what is
        // actually required, so caco reports rather than refuses.
        assert!(report(vec![], vec!["waylandpp"]).can_build());
        assert!(!report(vec!["ninja"], vec![]).can_build());
    }

    #[test]
    fn a_clean_machine_gets_no_install_hint() {
        assert!(report(vec![], vec![]).install_hint().is_none());
    }

    #[test]
    fn the_hint_covers_tools_and_packages_without_duplicates() {
        let hint = report(vec!["cmake", "ninja"], vec!["openal", "cmake"])
            .install_hint()
            .unwrap_or_default();
        if hint.is_empty() {
            // Unknown platform / no package manager — nothing to assert.
            return;
        }
        assert!(hint.contains("openal"));
        assert!(hint.contains("ninja"));
        assert_eq!(hint.matches("cmake").count(), 1, "got: {hint}");
    }

    #[test]
    fn pacman_output_parses_to_a_package_list() {
        assert_eq!(
            parse_pacman_output("libxmp\nportmidi\n\n"),
            vec!["libxmp".to_string(), "portmidi".to_string()]
        );
        assert!(parse_pacman_output("").is_empty());
    }

    #[test]
    fn an_empty_dep_list_is_reported_as_unchecked_not_satisfied() {
        let (missing, check) = check_packages(&[]);
        assert!(missing.is_empty());
        assert!(matches!(check, PackageCheck::Unsupported(_)));
    }

    #[test]
    fn doctor_reports_a_port_by_name() {
        let recipe = crate::ports::recipe::builtin_recipes()
            .into_iter()
            .find(|r| r.name == "uzdoom")
            .unwrap();
        assert_eq!(doctor(&recipe).port, "uzdoom");
    }
}
