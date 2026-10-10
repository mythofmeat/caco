//! Answer "can this machine build this sourceport?" before a build spends minutes
//! finding out that it cannot.
//!
//! Reporting only — caco never installs system packages on the user's behalf.
//! It prints the exact command so the user runs it themselves and their
//! package manager stays the only thing that has touched the system.

use std::process::Command;

use super::recipe::{DepSpec, SourceportRecipe};

/// The distributions caco keeps package lists for.
///
/// A recipe carries one list per distribution because the names differ —
/// Arch's `sdl2_image` is Fedora's `SDL2_image-devel` — so the doctor has to
/// know which list applies before it can ask anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distro {
    Arch,
    Fedora,
}

impl Distro {
    /// The host's distribution per os-release(5), or `None` when caco has no
    /// package list for it.
    pub fn detect() -> Option<Distro> {
        ["/etc/os-release", "/usr/lib/os-release"]
            .iter()
            .find_map(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| Self::from_os_release(&text))
    }

    /// `ID` first, then each `ID_LIKE` entry in order, so a derivative such as
    /// EndeavourOS or the Fedora Asahi Remix gets the list of the distribution
    /// it is built from.
    fn from_os_release(text: &str) -> Option<Distro> {
        let field = |key: &str| {
            text.lines().find_map(|line| {
                let value = line.strip_prefix(key)?.strip_prefix('=')?;
                Some(value.trim().trim_matches(['"', '\'']))
            })
        };
        field("ID")
            .into_iter()
            .chain(field("ID_LIKE").into_iter().flat_map(str::split_whitespace))
            .find_map(|id| match id {
                "arch" => Some(Distro::Arch),
                "fedora" => Some(Distro::Fedora),
                _ => None,
            })
    }

    fn name(self) -> &'static str {
        match self {
            Distro::Arch => "Arch",
            Distro::Fedora => "Fedora",
        }
    }

    fn package_manager(self) -> &'static str {
        match self {
            Distro::Arch => "pacman",
            Distro::Fedora => "rpm",
        }
    }

    fn packages(self, deps: &DepSpec) -> &[String] {
        match self {
            Distro::Arch => &deps.arch,
            Distro::Fedora => &deps.fedora,
        }
    }

    /// The package that puts `tool` on PATH. Fedora ships ninja as
    /// `ninja-build`, and `dnf install ninja` finds nothing.
    fn tool_package(self, tool: &str) -> &str {
        match (self, tool) {
            (Distro::Fedora, "ninja") => "ninja-build",
            _ => tool,
        }
    }

    fn install_command(self, packages: &[&str]) -> String {
        let packages = packages.join(" ");
        match self {
            Distro::Arch => format!("sudo pacman -S --needed {packages}"),
            Distro::Fedora => format!("sudo dnf install {packages}"),
        }
    }

    /// Which of `packages` are not installed, or `None` when the package
    /// manager could not be asked.
    ///
    /// Both queries exit non-zero exactly when something is missing, so a
    /// failure that names nothing is the query itself failing, not a machine
    /// with everything installed.
    fn missing(self, packages: &[String]) -> Option<Vec<String>> {
        let (out, parse): (_, fn(&str) -> Vec<String>) = match self {
            // The "which of these are unsatisfied" query: it prints the unmet
            // ones, one per line.
            Distro::Arch => (
                Command::new("pacman").arg("-T").args(packages).output(),
                parse_pacman_output,
            ),
            // --whatprovides, so a list may name a capability as well as a
            // package. LC_ALL because the "no package provides" line is
            // translated.
            Distro::Fedora => (
                Command::new("rpm")
                    .args(["-q", "--whatprovides"])
                    .args(packages)
                    .env("LC_ALL", "C")
                    .output(),
                parse_rpm_output,
            ),
        };
        let out = out.ok()?;
        let missing = parse(&String::from_utf8_lossy(&out.stdout));
        if !out.status.success() && missing.is_empty() {
            return None;
        }
        Some(missing)
    }
}

/// Which package manager caco was able to ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageCheck {
    /// Packages were checked against the distribution's package manager.
    Checked,
    /// No package list is known for this distribution, or its package manager
    /// could not be asked. Deps are reported unverified.
    Unsupported(String),
}

/// What a machine is missing before it can build a recipe.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub sourceport: String,
    /// Build tools absent from PATH. Hard blockers.
    pub missing_tools: Vec<String>,
    /// System packages the recipe lists that are not installed.
    pub missing_packages: Vec<String>,
    pub package_check: PackageCheck,
    /// Which distribution's names `missing_packages` uses, and so which
    /// package manager the install hint addresses.
    pub distro: Option<Distro>,
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
        let distro = self.distro?;
        if self.missing_packages.is_empty() && self.missing_tools.is_empty() {
            return None;
        }
        let mut all: Vec<&str> = self.missing_packages.iter().map(String::as_str).collect();
        for tool in &self.missing_tools {
            let package = distro.tool_package(tool);
            if !all.contains(&package) {
                all.push(package);
            }
        }
        Some(distro.install_command(&all))
    }
}

/// Check tools and system packages for one recipe.
pub fn doctor(recipe: &SourceportRecipe) -> DoctorReport {
    let missing_tools = super::build::required_tools(recipe)
        .into_iter()
        .filter(|t| crate::config::which(t).is_none())
        .collect();

    let distro = Distro::detect();
    let (missing_packages, package_check) = check_packages(distro, &recipe.deps);

    DoctorReport {
        sourceport: recipe.name.clone(),
        missing_tools,
        missing_packages,
        package_check,
        distro,
    }
}

/// Ask the host package manager which of the recipe's packages are missing.
fn check_packages(distro: Option<Distro>, deps: &DepSpec) -> (Vec<String>, PackageCheck) {
    let unchecked = |reason: String| (Vec::new(), PackageCheck::Unsupported(reason));
    let Some(distro) = distro else {
        return unchecked(
            "caco keeps package lists for Arch and Fedora; check the recipe's against your \
             distribution"
                .to_string(),
        );
    };
    let packages = distro.packages(deps);
    if packages.is_empty() {
        return unchecked(format!("the recipe lists no {} packages", distro.name()));
    }
    match distro.missing(packages) {
        Some(missing) => (missing, PackageCheck::Checked),
        None => unchecked(format!("could not ask {}", distro.package_manager())),
    }
}

fn parse_pacman_output(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// `rpm -q --whatprovides` prints the providing package for each capability
/// it can satisfy and `no package provides X` for each it cannot.
fn parse_rpm_output(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|l| l.trim().strip_prefix("no package provides "))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(
        distro: Option<Distro>,
        missing_tools: Vec<&str>,
        missing_packages: Vec<&str>,
    ) -> DoctorReport {
        DoctorReport {
            sourceport: "uzdoom".to_string(),
            missing_tools: missing_tools.into_iter().map(String::from).collect(),
            missing_packages: missing_packages.into_iter().map(String::from).collect(),
            package_check: PackageCheck::Checked,
            distro,
        }
    }

    #[test]
    fn missing_packages_alone_do_not_block_a_build() {
        // waylandpp is optional for uzdoom; cmake is the authority on what is
        // actually required, so caco reports rather than refuses.
        assert!(report(Some(Distro::Arch), vec![], vec!["waylandpp"]).can_build());
        assert!(!report(Some(Distro::Arch), vec!["ninja"], vec![]).can_build());
    }

    #[test]
    fn a_clean_machine_gets_no_install_hint() {
        assert!(
            report(Some(Distro::Arch), vec![], vec![])
                .install_hint()
                .is_none()
        );
    }

    #[test]
    fn an_unknown_distribution_gets_no_install_hint() {
        assert!(
            report(None, vec!["cmake"], vec!["openal"])
                .install_hint()
                .is_none()
        );
    }

    #[test]
    fn the_hint_covers_tools_and_packages_without_duplicates() {
        let hint = report(
            Some(Distro::Arch),
            vec!["cmake", "ninja"],
            vec!["openal", "cmake"],
        )
        .install_hint()
        .unwrap();
        assert_eq!(hint, "sudo pacman -S --needed openal cmake ninja");
    }

    #[test]
    fn the_fedora_hint_names_the_package_a_tool_comes_in() {
        // Missing as a package and as a tool: ninja-build once, bare ninja never,
        // since dnf has no package by that name.
        let hint = report(
            Some(Distro::Fedora),
            vec!["git", "ninja"],
            vec!["ninja-build", "openal-soft-devel"],
        )
        .install_hint()
        .unwrap();
        assert_eq!(hint, "sudo dnf install ninja-build openal-soft-devel git");

        let hint = report(Some(Distro::Fedora), vec!["ninja"], vec![])
            .install_hint()
            .unwrap();
        assert_eq!(hint, "sudo dnf install ninja-build");
    }

    #[test]
    fn os_release_id_picks_the_distribution() {
        let arch = "NAME=\"Arch Linux\"\nID=arch\nBUILD_ID=rolling\n";
        assert_eq!(Distro::from_os_release(arch), Some(Distro::Arch));
        let fedora = "NAME=\"Fedora Linux\"\nVERSION_ID=43\nID=fedora\n";
        assert_eq!(Distro::from_os_release(fedora), Some(Distro::Fedora));
    }

    #[test]
    fn a_derivative_gets_the_list_of_the_distribution_it_is_like() {
        let endeavour = "ID=endeavouros\nID_LIKE=arch\n";
        assert_eq!(Distro::from_os_release(endeavour), Some(Distro::Arch));
        let asahi =
            "NAME=\"Fedora Linux Asahi Remix\"\nID=fedora-asahi-remix\nID_LIKE=\"fedora\"\n";
        assert_eq!(Distro::from_os_release(asahi), Some(Distro::Fedora));
        let nobara = "ID=nobara\nID_LIKE=\"rhel centos fedora\"\n";
        assert_eq!(Distro::from_os_release(nobara), Some(Distro::Fedora));
    }

    #[test]
    fn a_distribution_without_lists_is_none() {
        let ubuntu = "ID=ubuntu\nID_LIKE=debian\nVERSION_ID=\"26.04\"\n";
        assert_eq!(Distro::from_os_release(ubuntu), None);
        assert_eq!(Distro::from_os_release(""), None);
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
    fn rpm_output_parses_to_the_missing_capabilities() {
        let stdout = "cmake-3.31.6-2.fc43.x86_64\n\
                      no package provides ninja-build\n\
                      sdl2-compat-devel-2.32.56-1.fc43.x86_64\n\
                      no package provides waylandpp-devel\n";
        assert_eq!(
            parse_rpm_output(stdout),
            vec!["ninja-build".to_string(), "waylandpp-devel".to_string()]
        );
        assert!(parse_rpm_output("cmake-3.31.6-2.fc43.x86_64\n").is_empty());
    }

    #[test]
    fn an_empty_dep_list_is_reported_as_unchecked_not_satisfied() {
        let deps = DepSpec {
            arch: vec!["cmake".to_string()],
            fedora: Vec::new(),
        };
        let (missing, check) = check_packages(Some(Distro::Fedora), &deps);
        assert!(missing.is_empty());
        assert!(matches!(check, PackageCheck::Unsupported(_)));
    }

    #[test]
    fn an_unknown_distribution_is_reported_as_unchecked() {
        let deps = DepSpec {
            arch: vec!["cmake".to_string()],
            fedora: vec!["cmake".to_string()],
        };
        let (missing, check) = check_packages(None, &deps);
        assert!(missing.is_empty());
        assert!(matches!(check, PackageCheck::Unsupported(_)));
    }

    #[test]
    fn every_builtin_recipe_has_a_list_for_every_distribution() {
        for recipe in crate::sourceports::recipe::builtin_recipes() {
            for distro in [Distro::Arch, Distro::Fedora] {
                assert!(
                    !distro.packages(&recipe.deps).is_empty(),
                    "{} has no {} packages",
                    recipe.name,
                    distro.name()
                );
            }
        }
    }

    #[test]
    fn doctor_reports_a_port_by_name() {
        let recipe = crate::sourceports::recipe::builtin_recipes()
            .into_iter()
            .find(|r| r.name == "uzdoom")
            .unwrap();
        assert_eq!(doctor(&recipe).sourceport, "uzdoom");
    }
}
