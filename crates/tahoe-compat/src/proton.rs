// SPDX-License-Identifier: MIT OR Apache-2.0
//! Proton: Valve's game-oriented Wine fork, invoked correctly from Tahoe.
//!
//! # Why this is a separate layer from [`crate::wine`]
//!
//! Proton is **not** a drop-in Wine replacement, and treating it as one
//! produces a worse desktop than plain Wine. Verified from its source:
//!
//! - `proton run` does not execute your program. It executes
//!   `c:\windows\system32\steam.exe` — a stub that accepts a Unix path and
//!   converts it — with your program as an argument.
//! - The prefix is seeded with Steam client DLLs, and `steam.exe` is forced
//!   to the builtin override.
//! - `WINEDEBUG=-all` is set, `winebth.sys` is disabled, `opencl=n,d` is
//!   forced, and a per-appid workaround table is applied.
//! - Its own FAQ calls launching an exe manually "untested".
//!
//! For a **game** all of that is the difference between working and not. For
//! a business application it is machinery that can only hurt. So the
//! launcher picks per program, and [`tahoe_format::pe::ProgramKind`] is how
//! it knows which is which.
//!
//! # The verbs, exactly
//!
//! Read from the `proton` script: `run`, `waitforexitandrun`,
//! `runinprefix`, `destroyprefix`, `getcompatpath`, `getnativepath`. There is
//! no `--help` and no `getpwfmode` — that verb does not exist on any branch.
//! Anything else exits 1 with "Need a verb."
//!
//! # The environment, exactly
//!
//! | Variable | Status |
//! |---|---|
//! | `STEAM_COMPAT_DATA_PATH` | **required**; absent means `exit 1` |
//! | `STEAM_COMPAT_CLIENT_INSTALL_PATH` | **effectively required**: read unguarded, so absent means a `KeyError` traceback |
//! | `WINEPREFIX` | **overwritten**; the real prefix is `<DATA_PATH>/pfx` |
//! | `PROTONPATH`, `GAMEID`, `PROTON_VERB` | consumed by **umu-launcher**, not by Proton |
//! | `STEAM_COMPAT_MOUNTS`, `PROTON_PATH`, `PROTONFIXES_*` | not read at all |
//!
//! `STEAM_COMPAT_CLIENT_INSTALL_PATH` does not have to be a working Steam
//! install: the video paths and the `legacycompat` copies are both guarded
//! by existence checks, so any existing directory satisfies it.
//!
//! # The supported way to run it outside Steam
//!
//! Upstream supports none. The de-facto supported route is **umu-launcher**,
//! which is what Lutris and Bottles moved to, and which is packaged for
//! CachyOS. Where `umu-run` exists this module prefers it.

use std::path::{Path, PathBuf};

use crate::wine::sanitize_component;

/// Proton's verbs, exactly as the script dispatches them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Set up the drives and run. Does not wait for a previous session.
    Run,
    /// `wineserver -w`, then the same as [`Verb::Run`].
    WaitForExitAndRun,
    /// Run a program inside an existing prefix without initializing it.
    RunInPrefix,
    /// Remove the files Proton has tracked in the compat data directory.
    DestroyPrefix,
    /// Unix path to Windows path.
    GetCompatPath,
    /// Windows path to Unix path.
    GetNativePath,
}

impl Verb {
    /// The literal argument Proton expects.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::WaitForExitAndRun => "waitforexitandrun",
            Self::RunInPrefix => "runinprefix",
            Self::DestroyPrefix => "destroyprefix",
            Self::GetCompatPath => "getcompatpath",
            Self::GetNativePath => "getnativepath",
        }
    }
}

/// Where a Proton build came from, which decides how to treat it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtonFlavour {
    /// Valve's own build, as shipped by Steam.
    Valve,
    /// CachyOS's rebuild: Proton plus native Wayland and PipeWire drivers.
    CachyOs,
    /// GE-Proton: upstream experimental Wine plus staging plus codecs.
    GloriousEggroll,
    /// Something else with the right shape.
    Unknown,
}

impl ProtonFlavour {
    /// Guess from a path, since the build carries no reliable marker.
    #[must_use]
    pub fn from_path(path: &Path) -> Self {
        let text = path.to_string_lossy().to_ascii_lowercase();
        if text.contains("cachyos") {
            Self::CachyOs
        } else if text.contains("ge-proton") {
            Self::GloriousEggroll
        } else if text.contains("/steamapps/common/proton") {
            Self::Valve
        } else {
            Self::Unknown
        }
    }

    /// What this flavour adds, for a confidence caveat.
    #[must_use]
    pub fn note(self) -> &'static str {
        match self {
            Self::Valve => "Valve's build, targeted at the Steam Linux Runtime",
            Self::CachyOs => "CachyOS's rebuild: Proton plus native Wayland and PipeWire drivers",
            Self::GloriousEggroll => {
                "GE-Proton: upstream experimental Wine plus staging and codecs"
            }
            Self::Unknown => "an unlabelled Proton build",
        }
    }
}

/// A Proton installation on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtonInstall {
    /// The directory containing the `proton` script.
    pub root: PathBuf,
    /// Which flavour it looks like.
    pub flavour: ProtonFlavour,
    /// `umu-run`, when it is installed. The supported launcher.
    pub umu: Option<PathBuf>,
}

/// An umu invocation: the program to exec, its environment, and its argv.
///
/// A named type rather than a three-element tuple, because a caller that
/// swaps two of those silently launches the wrong thing.
pub type UmuInvocation = (
    PathBuf,
    Vec<(std::ffi::OsString, std::ffi::OsString)>,
    Vec<std::ffi::OsString>,
);

/// Why a Proton invocation could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProtonError {
    /// The `proton` script is not where it should be.
    ScriptMissing(PathBuf),
    /// `STEAM_COMPAT_CLIENT_INSTALL_PATH` was not set and none could be
    /// derived.
    NoClientInstallPath,
}

impl std::fmt::Display for ProtonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ScriptMissing(p) => write!(f, "no proton script at {}", p.display()),
            Self::NoClientInstallPath => write!(
                f,
                "STEAM_COMPAT_CLIENT_INSTALL_PATH is not set and no Steam installation was found. \
                 Proton reads it unguarded, so it must point at an existing directory"
            ),
        }
    }
}

impl std::error::Error for ProtonError {}

impl ProtonInstall {
    /// Look for Proton builds in the places they actually live.
    ///
    /// Steam's `compatibilitytools.d` is where every third-party build
    /// installs itself, including CachyOS's package; `steamapps/common` is
    /// where Valve's own builds go. Standalone extractions are found through
    /// `TAHOE_PROTON_PATH`, which also makes this testable.
    #[must_use]
    pub fn discover() -> Vec<Self> {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let mut roots: Vec<PathBuf> = Vec::new();

        if let Some(explicit) = std::env::var_os("TAHOE_PROTON_PATH") {
            roots.push(PathBuf::from(explicit));
        }
        for base in [
            home.join(".local/share/Steam"),
            home.join(".steam/steam"),
            home.join(".steam/root"),
            home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
        ] {
            roots.push(base.join("compatibilitytools.d"));
            roots.push(base.join("steamapps/common"));
        }
        Self::discover_in(&roots, which("umu-run"))
    }

    /// Discovery with the search roots supplied rather than read from the
    /// environment.
    ///
    /// This exists because tests that mutate `TAHOE_PROTON_PATH` race each
    /// other when they run in parallel, and a test that passes only when it
    /// is not racing is worse than no test. Everything is a parameter.
    #[must_use]
    pub fn discover_in(roots: &[PathBuf], umu: Option<PathBuf>) -> Vec<Self> {
        let mut found = Vec::new();
        for root in roots {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let dir = entry.path();
                // A build is a directory with an executable `proton` in it.
                if dir.join("proton").is_file() && !found.iter().any(|f: &Self| f.root == dir) {
                    found.push(Self {
                        flavour: ProtonFlavour::from_path(&dir),
                        root: dir,
                        umu: umu.clone(),
                    });
                }
            }
        }
        // Prefer the flavour that suits this distribution, then Valve's.
        found.sort_by_key(|f| match f.flavour {
            ProtonFlavour::CachyOs => 0,
            ProtonFlavour::Valve => 1,
            ProtonFlavour::GloriousEggroll => 2,
            ProtonFlavour::Unknown => 3,
        });
        found
    }

    /// The `proton` script.
    #[must_use]
    pub fn script(&self) -> PathBuf {
        self.root.join("proton")
    }

    /// The prefix directory Proton will actually use.
    ///
    /// **Not** `<data_path>`: Proton appends `pfx`, and it overwrites
    /// `WINEPREFIX` with that value, so a caller that passes a prefix path
    /// gets it ignored.
    #[must_use]
    pub fn prefix_of(data_path: &Path) -> PathBuf {
        data_path.join("pfx")
    }

    /// Derive `STEAM_COMPAT_CLIENT_INSTALL_PATH`.
    ///
    /// Only has to exist: Proton's uses of it are both guarded by
    /// existence checks, so a real Steam install is not required.
    #[must_use]
    pub fn client_install_path(data_path: &Path) -> PathBuf {
        if let Some(existing) = std::env::var_os("STEAM_COMPAT_CLIENT_INSTALL_PATH") {
            return PathBuf::from(existing);
        }
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        Self::client_install_path_in(
            data_path,
            &[home.join(".local/share/Steam"), home.join(".steam/steam")],
        )
    }

    /// The candidate search, with the candidates supplied.
    #[must_use]
    pub fn client_install_path_in(data_path: &Path, candidates: &[PathBuf]) -> PathBuf {
        for candidate in candidates {
            if candidate.is_dir() {
                return candidate.clone();
            }
        }
        // Proton only checks that it exists, so return something that does
        // rather than let it raise a KeyError on a machine without Steam.
        data_path.to_path_buf()
    }

    /// The environment Proton needs, given a compat-data directory.
    ///
    /// Deliberately minimal: the game-oriented variables (`SteamAppId`,
    /// `PROTON_*` toggles, `SteamGameId`) are left unset so Proton behaves
    /// as plainly as it can.
    #[must_use]
    pub fn environment(data_path: &Path) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        vec![
            (
                "STEAM_COMPAT_DATA_PATH".into(),
                data_path.as_os_str().to_owned(),
            ),
            (
                "STEAM_COMPAT_CLIENT_INSTALL_PATH".into(),
                Self::client_install_path(data_path).into_os_string(),
            ),
            // `protonfixes` reads `PWD` unguarded and parses `/steamapps` out
            // of it, so the supervisor's environment whitelist is not enough
            // on its own — this has to be set here, and to a path that
            // matches. Found by running Proton, not by reading it.
            ("PWD".into(), data_path.as_os_str().to_owned()),
        ]
    }

    /// The command to run a program through Proton, without `umu`.
    ///
    /// # Errors
    ///
    /// Returns [`ProtonError::ScriptMissing`] when the build is incomplete.
    pub fn argv(
        &self,
        verb: Verb,
        program: &Path,
        args: &[std::ffi::OsString],
    ) -> Result<Vec<std::ffi::OsString>, ProtonError> {
        let script = self.script();
        if !script.is_file() {
            return Err(ProtonError::ScriptMissing(script));
        }
        let mut argv: Vec<std::ffi::OsString> = vec![
            script.into_os_string(),
            verb.as_str().into(),
            program.as_os_str().to_owned(),
        ];
        argv.extend(args.iter().cloned());
        Ok(argv)
    }

    /// The command to run a program through **umu**, the supported route.
    ///
    /// umu recreates the Steam container environment, which is what Proton
    /// expects and what a direct invocation only approximates. Its
    /// variables are its own: `PROTONPATH`, `GAMEID`, `PROTON_VERB`.
    ///
    /// # Errors
    ///
    /// Returns [`ProtonError::NoClientInstallPath`] when neither the
    /// environment nor a Steam installation supplies one.
    pub fn umu_argv(
        &self,
        data_path: &Path,
        program: &Path,
        args: &[std::ffi::OsString],
    ) -> Result<UmuInvocation, ProtonError> {
        let Some(umu) = &self.umu else {
            return Err(ProtonError::ScriptMissing(self.script()));
        };
        let mut env = Self::environment(data_path);
        env.push(("PROTONPATH".into(), self.root.as_os_str().to_owned()));
        // umu requires a game id; a stable placeholder keeps Proton off its
        // per-appid game table, which is what we want for a non-Steam target.
        env.push(("GAMEID".into(), "tahoe-default".into()));
        env.push((
            "PROTON_VERB".into(),
            Verb::WaitForExitAndRun.as_str().into(),
        ));
        let mut argv: Vec<std::ffi::OsString> = vec![program.as_os_str().to_owned()];
        argv.extend(args.iter().cloned());
        Ok((umu.clone(), env, argv))
    }

    /// A compat-data directory for a named target, under Tahoe's own tree.
    ///
    /// Per-target rather than per-user, for the same reason Wine prefixes
    /// are: a broken prefix should cost one application, not all of them.
    ///
    /// **The final path component is a decimal number, and that is not
    /// cosmetic.** `proton-cachyos` ships `protonfixes`, whose
    /// `get_game_id()` is:
    ///
    /// ```python
    /// return re.findall(r'\d+', os.environ['STEAM_COMPAT_DATA_PATH'])[-1]
    /// ```
    ///
    /// A path with no digits raises `IndexError` before the program starts.
    /// Real launchers satisfy this by using Steam's app id; Tahoe has no app
    /// id, so it derives a stable one from the target's name. Stable matters:
    /// the same program must always land in the same prefix.
    #[must_use]
    pub fn data_path_for(name: &str) -> PathBuf {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        // Shaped like Steam's own tree, and that is required rather than
        // tidy. `protonfixes` (shipped by Proton-CachyOS and GE-Proton)
        // does:
        //
        //     re.findall(r'.*/steamapps', os.environ['PWD'])[-1]
        //
        // so `PWD` must contain `/steamapps`, and `get_game_id` reads digits
        // out of the data path. A plain path fails both. Every launcher that
        // runs Proton outside Steam builds this shape for the same reason.
        home.join(".local/share/tahoe/proton/steamapps/compatdata")
            .join(format!("{}-{}", sanitize_component(name), stable_id(name)))
    }
}

/// A stable decimal id for a target name.
///
/// FNV-1a over the name, reduced to a nine-digit number. Not a hash for
/// security — it exists because `protonfixes` requires digits in the compat
/// data path, and the same name must always produce the same id so a program
/// keeps its prefix across runs.
#[must_use]
pub fn stable_id(name: &str) -> u32 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    // Keep it in the range a Steam app id would occupy, and never zero.
    u32::try_from(hash % 900_000_000 + 100_000_000).unwrap_or(100_000_000)
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests;
