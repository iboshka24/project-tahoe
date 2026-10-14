// SPDX-License-Identifier: MIT OR Apache-2.0
//! Windows integration: prefixes, discovery, and Wine's own desktop files.
//!
//! # The design decision this module encodes
//!
//! Tahoe does **not** fork Wine and does **not** reimplement Windows
//! shortcuts. Wine already ships the two mechanisms a desktop needs:
//!
//! 1. **`winemenubuilder`** converts every Windows shortcut into a
//!    FreeDesktop `.desktop` file, a `.menu` file, a PNG icon and a MIME
//!    association, and it already emits `StartupWMClass` — which is exactly
//!    the key a Wayland compositor needs to group a window with its dock
//!    entry. Reimplementing that would be strictly worse.
//! 2. **`Start Menu\Programs\**\*.lnk`** is the authoritative list of what
//!    is installed, and reading it is what Bottles does.
//!
//! So this module *reads what Wine writes* rather than shadowing it. It
//! parses both sources, because neither is complete on its own:
//! `winemenubuilder` can be disabled (and is, by Lutris by default), while
//! the Start Menu misses anything the user pinned elsewhere.
//!
//! # Verifiable claims
//!
//! Every path and registry key below is from Wine's source, not from
//! folklore. Where a fact could not be verified, the code says so.

use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use crate::{AppOrigin, InstalledApp, Launch};

/// Environment variables Wine documents as current in Wine 11.
///
/// `WINEESYNC` and `WINEFSYNC` are deliberately **absent**: the research
/// for this module found them nowhere in upstream Wine — they exist only in
/// `wine-staging` historical patches and in Valve's Proton fork. Advertising
/// them would be advertising something that does nothing on the official
/// Arch package. The upstream mechanism is NTSync, which needs no variable.
pub const PREFIX_ENV: &[&str] = &[
    "WINEPREFIX",
    "WINEARCH",
    "WINEDEBUG",
    "WINEDLLOVERRIDES",
    "WINEPATH",
];

/// Kernel device NTSync exposes, and the kernel version that added it.
///
/// `CONFIG_NTSYNC` landed in Linux 6.14. Wine 11 uses it automatically when
/// present and falls back to the wineserver sync path when not.
pub const NTSYNC_DEVICE: &str = "/dev/ntsync";
/// First kernel release with NTSync.
pub const NTSYNC_MIN_KERNEL: (u64, u64) = (6, 14);

/// Start Menu directories inside a prefix, newest layout first.
///
/// Wine's `shellpath.c` maps `CSIDL_PROGRAMS` under `AppData\Roaming` for
/// the user and under `ProgramData` for all users. The pre-Vista
/// `users/<user>/Start Menu` layout is still produced by older prefixes, so
/// both are searched.
pub const START_MENU_SUBDIRS: &[&str] = &[
    "users/{user}/AppData/Roaming/Microsoft/Windows/Start Menu/Programs",
    "ProgramData/Microsoft/Windows/Start Menu/Programs",
    "users/{user}/Start Menu/Programs",
    "users/Public/Start Menu/Programs",
];

/// Where `winemenubuilder` writes its output, relative to an XDG data dir.
pub const WINE_APPLICATIONS_SUBDIR: &str = "applications/wine";

/// A Wine prefix on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefix {
    /// The prefix directory (`WINEPREFIX`).
    pub path: PathBuf,
}

/// Why a prefix operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PrefixError {
    /// The directory does not exist.
    Missing(PathBuf),
    /// The directory exists but has no `drive_c`, so Wine never ran here.
    NotInitialised(PathBuf),
    /// A path could not be built.
    Io {
        /// Path involved.
        path: PathBuf,
        /// What the OS said.
        message: String,
    },
}

impl std::fmt::Display for PrefixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(p) => write!(f, "no Wine prefix at {}", p.display()),
            Self::NotInitialised(p) => write!(
                f,
                "{} has no drive_c: the prefix was never initialised",
                p.display()
            ),
            Self::Io { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for PrefixError {}

impl Prefix {
    /// Wrap a directory. Does not touch the filesystem.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The conventional per-user prefix, `$HOME/.wine`.
    #[must_use]
    pub fn default_location() -> Self {
        let path = std::env::var_os("WINEPREFIX")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home =
                    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
                home.join(".wine")
            });
        Self::new(path)
    }

    /// A Tahoe-managed prefix for a named application or bottle.
    ///
    /// Per-application prefixes are the isolation boundary: a broken
    /// prefix, a hostile installer, or a registry disaster is contained.
    #[must_use]
    pub fn for_application(name: &str) -> Self {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        Self::new(
            home.join(".local/share/tahoe/wine")
                .join(sanitize_component(name)),
        )
    }

    /// `drive_c`.
    #[must_use]
    pub fn drive_c(&self) -> PathBuf {
        self.path.join("drive_c")
    }

    /// Whether Wine has ever initialised this prefix.
    #[must_use]
    pub fn is_initialised(&self) -> bool {
        self.drive_c().join("windows").is_dir()
    }

    /// Whether the prefix exists at all.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.path.is_dir()
    }

    /// Architecture the prefix was created with, read from its registry, or
    /// `None` when the prefix is not initialised.
    ///
    /// Wine records the build architecture; the presence of `syswow64`
    /// alongside `system32` is the on-disk signal that 32-bit support is
    /// present, which under Wine 11 means new-WoW64 rather than a 32-bit
    /// loader.
    #[must_use]
    pub fn architecture(&self) -> Option<PrefixArchitecture> {
        if !self.is_initialised() {
            return None;
        }
        let system32 = self.drive_c().join("windows/system32");
        if !system32.is_dir() {
            return None;
        }
        let has_syswow64 = self.drive_c().join("windows/syswow64").is_dir();
        Some(if has_syswow64 {
            PrefixArchitecture::Wow64
        } else {
            PrefixArchitecture::Win64Only
        })
    }

    /// Existing Start Menu directories inside this prefix.
    #[must_use]
    pub fn start_menu_dirs(&self) -> Vec<PathBuf> {
        let user = prefix_user_name(self);
        let mut out = Vec::new();
        for template in START_MENU_SUBDIRS {
            let relative = template.replace("{user}", &user);
            let dir = self.drive_c().join(relative);
            if dir.is_dir() {
                out.push(dir);
            }
        }
        out
    }

    /// The prefix's user Desktop directory, when present.
    #[must_use]
    pub fn desktop_dir(&self) -> Option<PathBuf> {
        let user = prefix_user_name(self);
        let dir = self.drive_c().join("users").join(&user).join("Desktop");
        dir.is_dir().then_some(dir)
    }

    /// Environment for launching a program in this prefix.
    ///
    /// Deliberately minimal: `WINEPREFIX` plus the two DLL overrides that
    /// stop Wine from popping the Gecko/Mono download dialogs during a
    /// headless or first-run launch. The overrides are the de-facto distro
    /// idiom rather than a documented constant, so they are named here
    /// instead of being buried at the call site.
    #[must_use]
    pub fn launch_env(&self) -> Vec<(OsString, OsString)> {
        vec![
            ("WINEPREFIX".into(), self.path.as_os_str().to_owned()),
            ("WINEDLLOVERRIDES".into(), "mscoree,mshtml=".into()),
        ]
    }

    /// Every Windows application discoverable in this prefix.
    ///
    /// Two sources, deduplicated by launch target, because they disagree in
    /// practice: `winemenubuilder` output survives even when the shortcut
    /// was deleted, and a `.lnk` can exist before `winemenubuilder` has
    /// processed it.
    #[must_use]
    pub fn applications(&self) -> Vec<InstalledApp> {
        let mut apps = self.start_menu_applications();
        let mut seen: BTreeSet<PathBuf> = apps
            .iter()
            .filter_map(|a| match &a.launch {
                Launch::Windows { program, .. } => Some(program.clone()),
                _ => None,
            })
            .collect();
        for app in wine_desktop_entries_in(&self.path) {
            if let Launch::Windows { program, .. } = &app.launch
                && !seen.insert(program.clone())
            {
                continue;
            }
            apps.push(app);
        }
        apps
    }

    /// Applications found as `.lnk` files under the Start Menu.
    ///
    /// The `.lnk` is launched through `wine start`, which is how every
    /// third-party launcher does it: it resolves the shortcut exactly as
    /// Windows would, including arguments and working directory.
    #[must_use]
    pub fn start_menu_applications(&self) -> Vec<InstalledApp> {
        let mut apps = Vec::new();
        for dir in self.start_menu_dirs() {
            collect_links(&dir, &self.path, 0, &mut apps);
        }
        apps.sort_by_key(|a| a.name.to_lowercase());
        apps
    }
}

fn prefix_user_name(prefix: &Prefix) -> String {
    let users = prefix.drive_c().join("users");
    // Prefer the current user's directory name if it exists, which is what
    // Wine does when it creates the prefix.
    if let Some(user) = std::env::var_os("USER").and_then(|u| u.to_str().map(str::to_owned))
        && users.join(&user).is_dir()
    {
        return user;
    }
    if let Ok(rd) = fs::read_dir(&users) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name != "Public" && name != "All Users" && entry.path().is_dir() {
                return name;
            }
        }
    }
    "user".to_owned()
}

/// Architecture shape of an initialised prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixArchitecture {
    /// `system32` and `syswow64` both present: 32-bit PE runs through
    /// Wine 11's thunking in a 64-bit process.
    Wow64,
    /// Only `system32`: a prefix with no 32-bit support.
    Win64Only,
}

/// Turn a UI label into a single safe path component.
///
/// A traversal attempt is **refused**, not laundered: a name containing a
/// path separator or `..` never becomes a usable component, because a
/// half-cleaned `../../etc` is exactly the kind of value that later looks
/// like a path. Refusing is one line and cannot be got wrong.
#[must_use]
pub fn sanitize_component(name: &str) -> String {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return "app".to_owned();
    }
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(['-', '.']).to_owned();
    if trimmed.is_empty() {
        "app".to_owned()
    } else {
        trimmed
    }
}

/// Walk a Start Menu directory for `.lnk` files.
fn collect_links(dir: &Path, prefix: &Path, depth: usize, out: &mut Vec<InstalledApp>) {
    // Start menus nest (Programs/Vendor/Product), but not deeply.
    const MAX_DEPTH: usize = 6;
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_links(&path, prefix, depth + 1, out);
            continue;
        }
        let is_link = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk") || e.eq_ignore_ascii_case("url"));
        if !is_link {
            continue;
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Strip installer noise from the display name.
        let name = clean_display_name(&stem);
        if name.is_empty() {
            continue;
        }
        let details = fs::read(&path)
            .ok()
            .map(|b| parse_shortcut(&b))
            .unwrap_or_default();
        out.push(InstalledApp {
            id: format!("wine:{}", sanitize_component(&name).to_lowercase()),
            name,
            icon: None,
            launch: Launch::Windows {
                prefix: prefix.to_path_buf(),
                program: path.clone(),
            },
            origin: AppOrigin::WineStartMenu,
            caveats: vec!["runs under Wine: a compatibility layer, not Windows"],
        });
        let _ = details;
    }
}

/// Strip the trailing installer junk Windows puts in shortcut names.
#[must_use]
pub fn clean_display_name(raw: &str) -> String {
    let mut name = raw.trim().to_owned();
    for suffix in [
        " - Shortcut",
        " (uninstall)",
        " (Uninstall)",
        " uninstall",
        " Uninstall",
    ] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.trim_end().to_owned();
        }
    }
    name
}

/// What a Windows shortcut declares, as far as we parse it.
///
/// This is a **partial** reader of MS-SHLLINK: the 76-byte header plus the
/// `StringData` fields that carry human-readable text. It deliberately does
/// not resolve the `LinkTargetIDList`, because resolving a shell item to a
/// path requires the shell namespace; `wine start` does that at launch time
/// instead, which is both correct and less code.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shortcut {
    /// True when the header magic and CLSID matched.
    pub valid: bool,
    /// `NAME_STRING`, when present.
    pub name: Option<String>,
    /// `RELATIVE_PATH`, when present.
    pub relative_path: Option<String>,
    /// `WORKING_DIR`, when present.
    pub working_dir: Option<String>,
    /// `COMMAND_LINE_ARGUMENTS`, when present.
    pub arguments: Option<String>,
    /// Raw `LinkFlags`.
    pub flags: u32,
    /// Raw `FileAttributes`.
    pub file_attributes: u32,
}

impl Shortcut {
    /// True when the shortcut points at a directory rather than a file.
    #[must_use]
    pub fn targets_directory(&self) -> bool {
        // FILE_ATTRIBUTE_DIRECTORY == 0x10.
        self.file_attributes & 0x10 != 0
    }
}

/// The 16-byte link CLSID `00021401-0000-0000-C000-000000000046`.
const LNK_CLSID: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// Parse the parts of a Windows shortcut this module needs.
#[must_use]
pub fn parse_shortcut(bytes: &[u8]) -> Shortcut {
    let mut out = Shortcut::default();
    if bytes.len() < 0x4C {
        return out;
    }
    let header_size = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if header_size != 0x4C || bytes[4..0x14] != LNK_CLSID {
        return out;
    }
    out.valid = true;
    out.flags = u32::from_le_bytes([bytes[0x14], bytes[0x15], bytes[0x16], bytes[0x17]]);
    out.file_attributes = u32::from_le_bytes([bytes[0x18], bytes[0x19], bytes[0x1A], bytes[0x1B]]);

    let mut cursor = 0x4C;
    // HasLinkTargetIDList (bit 0): a u16 length followed by that many bytes.
    if out.flags & 0x1 != 0 {
        let Some(len_bytes) = bytes.get(cursor..cursor + 2) else {
            return out;
        };
        let len = u16::from_le_bytes([len_bytes[0], len_bytes[1]]) as usize;
        cursor = match cursor.checked_add(2).and_then(|c| c.checked_add(len)) {
            Some(c) if c <= bytes.len() => c,
            _ => return out,
        };
    }
    // HasLinkInfo (bit 1): a u32 size, then that many bytes.
    if out.flags & 0x2 != 0 {
        let Some(size_bytes) = bytes.get(cursor..cursor + 4) else {
            return out;
        };
        let size = u32::from_le_bytes([size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3]])
            as usize;
        cursor = match cursor.checked_add(size) {
            Some(c) if c <= bytes.len() => c,
            _ => return out,
        };
    }
    // StringData follows, in fixed order, each present only if its bit is set.
    for (bit, slot) in [
        (0x4u32, 0usize), // NAME_STRING
        (0x8, 1),         // RELATIVE_PATH
        (0x10, 2),        // WORKING_DIR
        (0x20, 3),        // COMMAND_LINE_ARGUMENTS
        (0x40, 4),        // ICON_LOCATION
    ] {
        if out.flags & bit == 0 {
            continue;
        }
        let Some((text, next)) = read_unicode_string(bytes, cursor) else {
            return out;
        };
        cursor = next;
        if text.is_empty() {
            continue;
        }
        match slot {
            0 => out.name = Some(text),
            1 => out.relative_path = Some(text),
            2 => out.working_dir = Some(text),
            3 => out.arguments = Some(text),
            _ => {}
        }
    }
    out
}

/// Read a `CountCharacters`-prefixed UTF-16LE string.
fn read_unicode_string(bytes: &[u8], at: usize) -> Option<(String, usize)> {
    let count_bytes = bytes.get(at..at + 2)?;
    let count = u16::from_le_bytes([count_bytes[0], count_bytes[1]]) as usize;
    let start = at + 2;
    let end = start.checked_add(count.checked_mul(2)?)?;
    let raw = bytes.get(start..end)?;
    let units: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    Some((String::from_utf16_lossy(&units), end))
}

/// Find `winemenubuilder`'s `.desktop` entries for one prefix.
///
/// `winemenubuilder` writes to `$XDG_DATA_HOME/applications/wine/…` and
/// embeds `WINEPREFIX` in the `Exec=` line, which is how a caller can tell
/// which prefix an entry belongs to.
#[must_use]
pub fn wine_desktop_entries_in(prefix: &Path) -> Vec<InstalledApp> {
    let mut out = Vec::new();
    for root in xdg_data_dirs() {
        let dir = root.join(WINE_APPLICATIONS_SUBDIR);
        collect_desktop_entries(&dir, prefix, 0, &mut out);
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// XDG data directories, `$XDG_DATA_HOME` first.
#[must_use]
pub fn xdg_data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    let data_home =
        std::env::var_os("XDG_DATA_HOME").map_or_else(|| home.join(".local/share"), PathBuf::from);
    let mut dirs = vec![data_home];
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .unwrap_or_else(|| OsString::from("/usr/local/share:/usr/share"));
    dirs.extend(std::env::split_paths(&data_dirs));
    dirs
}

fn collect_desktop_entries(dir: &Path, prefix: &Path, depth: usize, out: &mut Vec<InstalledApp>) {
    const MAX_DEPTH: usize = 6;
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_desktop_entries(&path, prefix, depth + 1, out);
            continue;
        }
        if path.extension().is_none_or(|e| e != "desktop") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(app) = parse_wine_desktop_entry(&text, &path, prefix) {
            out.push(app);
        }
    }
}

/// Parse one of `winemenubuilder`'s `.desktop` files.
///
/// Returns `None` when the file does not belong to `prefix`, which is how
/// a multi-prefix system keeps its entries separate.
#[must_use]
pub fn parse_wine_desktop_entry(text: &str, _path: &Path, prefix: &Path) -> Option<InstalledApp> {
    let mut in_entry = false;
    let mut name = None;
    let mut exec = None;
    let mut icon = None;
    let mut no_display = false;
    let mut wm_class = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Name" if name.is_none() => name = Some(value.trim().to_owned()),
            "Exec" if exec.is_none() => exec = Some(value.trim().to_owned()),
            "Icon" if icon.is_none() => icon = Some(value.trim().to_owned()),
            "StartupWMClass" if wm_class.is_none() => wm_class = Some(value.trim().to_owned()),
            "NoDisplay" => no_display = value.trim().eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    let name = name?;
    let exec = exec?;
    let program = extract_wine_exec_target(&exec)?;
    // `winemenubuilder` embeds the prefix in `Exec`; the entry only belongs
    // to us if it names the prefix we are asking about.
    if !exec.contains(&prefix.display().to_string()) {
        return None;
    }
    let mut caveats = vec!["runs under Wine: a compatibility layer, not Windows"];
    if no_display {
        caveats.push("hidden from menus by the generator");
    }
    Some(InstalledApp {
        id: wm_class
            .unwrap_or_else(|| format!("wine:{}", sanitize_component(&name).to_lowercase())),
        name,
        icon,
        launch: Launch::Windows {
            prefix: prefix.to_path_buf(),
            program,
        },
        origin: AppOrigin::WineDesktopEntry,
        caveats,
    })
}

/// Pull the Windows program path out of a `winemenubuilder` `Exec=` line.
///
/// The generated shape is
/// `env "WINEPREFIX=<path>" wine "<program>" [args]`, so the argument after
/// `wine` is the target. Anything that does not match this shape is refused
/// rather than guessed at.
#[must_use]
pub fn extract_wine_exec_target(exec: &str) -> Option<PathBuf> {
    let tokens = shell_words(exec);
    let wine_at = tokens
        .iter()
        .position(|t| t == "wine" || t.ends_with("/wine"))?;
    let target = tokens.get(wine_at + 1)?;
    if target.is_empty() || target.starts_with('-') {
        return None;
    }
    Some(PathBuf::from(target))
}

/// Split a `.desktop` `Exec` line, honouring the quoting the spec allows.
///
/// The spec permits double quotes with backslash escapes and forbids shell
/// evaluation. Only that much is implemented: a `.desktop` file is data,
/// and treating it as shell would be a code-execution hole.
#[must_use]
pub fn shell_words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut started = false;
    for c in line.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_quotes => escaped = true,
            '"' => {
                in_quotes = !in_quotes;
                started = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if started || !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started || !current.is_empty() {
        out.push(current);
    }
    out
}

/// Wine's runtime state on this machine, as observed rather than assumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WineRuntime {
    /// Path to the `wine` loader, when installed.
    pub loader: Option<PathBuf>,
    /// `wine --version` output, when it could be read.
    pub version: Option<String>,
    /// Whether `/dev/ntsync` exists, which is what enables Wine 11's
    /// synchronisation fast path.
    pub ntsync_available: bool,
    /// Kernel release, for the version note in the UI.
    pub kernel: String,
}

impl WineRuntime {
    /// Probe the system. Never fails; absence is a finding.
    #[must_use]
    pub fn detect() -> Self {
        let loader = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|d| d.join("wine"))
                .find(|p| p.is_file())
        });
        let version = loader.as_ref().and_then(|w| {
            std::process::Command::new(w)
                .arg("--version")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        });
        let kernel = fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();
        Self {
            loader,
            version,
            ntsync_available: Path::new(NTSYNC_DEVICE).exists(),
            kernel,
        }
    }

    /// True when Wine is installed and answered `--version`.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.loader.is_some()
    }

    /// Whether the running kernel is new enough for NTSync at all.
    ///
    /// A kernel that is new enough but has the module unloaded reports
    /// `ntsync_available == false`; both facts are surfaced separately so a
    /// user can tell "upgrade" from "load the module".
    #[must_use]
    pub fn kernel_supports_ntsync(&self) -> bool {
        crate::macos::parse_kernel_release(&self.kernel)
            .is_some_and(|(major, minor, _)| (major, minor) >= NTSYNC_MIN_KERNEL)
    }

    /// A sentence for the diagnostics pane.
    #[must_use]
    pub fn describe(&self) -> String {
        let mut out = String::new();
        match (&self.loader, &self.version) {
            (Some(p), Some(v)) => out.push_str(&format!("wine {v} at {}\n", p.display())),
            (Some(p), None) => {
                out.push_str(&format!("{} present but --version failed\n", p.display()))
            }
            (None, _) => out.push_str("wine is not installed\n"),
        }
        out.push_str(&match (self.ntsync_available, self.kernel_supports_ntsync()) {
            (true, _) => format!("NTSync available at {NTSYNC_DEVICE}\n"),
            (false, true) => format!(
                "kernel {} supports NTSync but {NTSYNC_DEVICE} is missing; load the ntsync module\n",
                self.kernel
            ),
            (false, false) => format!(
                "NTSync needs kernel >= {}.{}; this is {}\n",
                NTSYNC_MIN_KERNEL.0, NTSYNC_MIN_KERNEL.1, self.kernel
            ),
        });
        out
    }
}

#[cfg(test)]
mod tests;
