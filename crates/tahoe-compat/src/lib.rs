// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-compat`: how Tahoe actually runs foreign software.
//!
//! Three concerns, deliberately separated because they fail in different
//! ways and are fixed by different upstreams:
//!
//! - [`wine`] — Windows integration: prefix management, discovering
//!   installed applications, reading the `.desktop` entries Wine's own
//!   `winemenubuilder` writes. This is where the Dock gets its Windows
//!   icons from.
//! - [`macos`] — Mach-O inspection, `.app` bundle resolution, plist
//!   reading, DMG packaging. Inspection only; Darling does the running.
//! - [`binfmt`] — rootless, namespaced `binfmt_misc` dispatch. The kernel
//!   mechanism, and an honest account of what it cannot express.
//! - [`proton`] — Valve's game-oriented Wine fork, invoked the way it
//!   actually expects. Separate from [`wine`] because it is not a drop-in
//!   replacement and treating it as one produces a worse desktop.
//! - [`xtool`] — building Apple applications on Linux with xtool, and
//!   feeding the resulting `.app` back through [`macos`] for inspection.
//!   This is what turns the macOS parsers from fixture-tested into
//!   artifact-tested, on a machine with no Mac.
//!
//! Nothing in this crate decides *whether* to run something. That is
//! `tahoe-uel`'s job, and it does it with a confidence report (ADR-0006).

pub mod binfmt;
pub mod macos;
pub mod proton;
pub mod wine;
pub mod xtool;

/// A discovered application, from any source.
///
/// This is the shape the Dock and Launcher consume, so it carries what a
/// launcher needs and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    /// Stable identity used for window matching and dock grouping.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Where the launcher should look for an icon, if anywhere.
    pub icon: Option<String>,
    /// How to launch it.
    pub launch: Launch,
    /// Where the record came from, for diagnostics.
    pub origin: AppOrigin,
    /// Known caveats that the UI should surface.
    pub caveats: Vec<&'static str>,
}

/// How to start a discovered application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// A native executable with arguments.
    Native {
        /// Program to run.
        program: std::path::PathBuf,
        /// Arguments.
        args: Vec<std::ffi::OsString>,
    },
    /// A Windows program through Wine.
    Windows {
        /// The `WINEPREFIX` to use.
        prefix: std::path::PathBuf,
        /// The program, as a Unix path inside the prefix.
        program: std::path::PathBuf,
    },
    /// A macOS bundle through Darling.
    MacBundle {
        /// The `DPREFIX` to use.
        prefix: std::path::PathBuf,
        /// The bundle's `Contents/MacOS/<executable>`.
        executable: std::path::PathBuf,
    },
}

/// Which subsystem a record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppOrigin {
    /// A `.desktop` file Wine's `winemenubuilder` generated.
    WineDesktopEntry,
    /// A `.lnk` found under a prefix's Start Menu.
    WineStartMenu,
    /// A `.app` bundle discovered on disk.
    MacBundle,
}

impl AppOrigin {
    /// A short label for logs.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::WineDesktopEntry => "wine-desktop",
            Self::WineStartMenu => "wine-start-menu",
            Self::MacBundle => "mac-bundle",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_labels_are_stable() {
        assert_eq!(AppOrigin::WineDesktopEntry.label(), "wine-desktop");
        assert_eq!(AppOrigin::WineStartMenu.label(), "wine-start-menu");
        assert_eq!(AppOrigin::MacBundle.label(), "mac-bundle");
    }
}
