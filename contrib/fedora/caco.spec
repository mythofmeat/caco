# Built by .github/workflows/package.yml from a `git archive` of the checkout. cargo isn't a BuildRequires: CI
# installs current stable Rust with rustup, since the weekly dependency updates can need a newer Rust than Fedora
# ships.

%global debug_package %{nil}

Name:           caco
Version:        4.2.0
Release:        1%{?dist}
Summary:        Doom WAD library manager
License:        MIT
URL:            https://github.com/mythofmeat/caco
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  gcc
BuildRequires:  desktop-file-utils
Requires:       hicolor-icon-theme
# winit and wgpu dlopen these, so rpm's dependency scan can't see them. Without libXcursor, libXi or
# libxkbcommon-x11, caco dies at startup in an X session.
Requires:       libwayland-client%{?_isa}
Requires:       libxkbcommon%{?_isa}
Requires:       libX11-xcb%{?_isa}
Requires:       libXcursor%{?_isa}
Requires:       libXi%{?_isa}
Requires:       libxkbcommon-x11%{?_isa}
Requires:       (vulkan-loader%{?_isa} or libglvnd-egl%{?_isa})
# The PKGBUILD's optdepends. wgpu falls back to GL without a Vulkan driver, file dialogs to none without a portal,
# and git is only for building sourceports from source.
Recommends:     mesa-vulkan-drivers
Recommends:     xdg-desktop-portal
Recommends:     git

%description
A personal Doom WAD library manager inspired by beets. Import WADs from idgames,
the Doom Wiki, Doomworld or local files, track what you have played, and launch
them with your preferred sourceport.

%prep
%autosetup

%build
# Fedora's RUSTFLAGS add full debug info and turn stripping off, which would override the workspace's release profile.
unset RUSTFLAGS
cargo build --release --locked --workspace

%install
install -Dm755 target/release/caco %{buildroot}%{_bindir}/caco
install -Dm644 assets/caco.desktop %{buildroot}%{_datadir}/applications/caco.desktop
install -Dm644 assets/caco.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/caco.svg

# As the PKGBUILD's check(), and the only place the tests run on aarch64.
%check
desktop-file-validate %{buildroot}%{_datadir}/applications/caco.desktop
unset RUSTFLAGS
cargo test --locked --workspace

%files
%license LICENSE
%doc README.md
%{_bindir}/caco
%{_datadir}/applications/caco.desktop
%{_datadir}/icons/hicolor/scalable/apps/caco.svg
