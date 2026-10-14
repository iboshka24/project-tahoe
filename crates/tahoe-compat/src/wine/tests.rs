// SPDX-License-Identifier: MIT OR Apache-2.0
//! Wine integration tests.
//!
//! Everything runs against synthetic prefix trees built in a scratch
//! directory, so the tests exercise the real discovery code without Wine
//! being installed. The fixtures reproduce the on-disk shapes that Wine
//! and `winemenubuilder` actually produce, as documented in the module.

use super::*;
use std::fs;

/// A scratch directory removed when the test ends.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join("tahoe-wine-tests").join(format!(
            "{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch");
        Self { path }
    }

    fn file(&self, relative: &str, contents: &[u8]) -> PathBuf {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        fs::write(&path, contents).expect("write");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A minimal prefix with `drive_c`, as Wine creates it.
fn initialised_prefix(s: &Scratch, with_wow64: bool) -> Prefix {
    fs::create_dir_all(s.path.join("drive_c/windows/system32")).expect("system32");
    if with_wow64 {
        fs::create_dir_all(s.path.join("drive_c/windows/syswow64")).expect("syswow64");
    }
    fs::create_dir_all(s.path.join("drive_c/users/tester")).expect("users");
    Prefix::new(&s.path)
}

// ---------------------------------------------------------------------------
// Prefix shape
// ---------------------------------------------------------------------------

#[test]
fn an_uninitialised_prefix_is_reported_as_such() {
    let s = Scratch::new("uninit");
    let prefix = Prefix::new(&s.path);
    assert!(prefix.exists());
    assert!(!prefix.is_initialised());
    assert_eq!(
        prefix.architecture(),
        None,
        "a prefix with no drive_c has no architecture"
    );
    assert!(prefix.start_menu_dirs().is_empty());
    assert!(prefix.desktop_dir().is_none());
}

#[test]
fn a_missing_prefix_is_not_an_error_to_describe() {
    let prefix = Prefix::new("/definitely/not/a/prefix");
    assert!(!prefix.exists());
    assert!(!prefix.is_initialised());
    assert!(
        prefix.applications().is_empty(),
        "discovery on a missing prefix yields nothing"
    );
}

#[test]
fn wow64_prefixes_are_distinguished_from_64_bit_only_ones() {
    let s = Scratch::new("arch");
    let wow64 = initialised_prefix(&s, true);
    assert_eq!(wow64.architecture(), Some(PrefixArchitecture::Wow64));

    let s2 = Scratch::new("arch64");
    let only64 = initialised_prefix(&s2, false);
    assert_eq!(only64.architecture(), Some(PrefixArchitecture::Win64Only));
}

#[test]
fn start_menu_dirs_covers_both_the_modern_and_legacy_layouts() {
    let s = Scratch::new("startmenu");
    let prefix = initialised_prefix(&s, true);
    fs::create_dir_all(
        s.path
            .join("drive_c/users/tester/AppData/Roaming/Microsoft/Windows/Start Menu/Programs"),
    )
    .expect("modern");
    fs::create_dir_all(
        s.path
            .join("drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs"),
    )
    .expect("all users");
    fs::create_dir_all(s.path.join("drive_c/users/tester/Start Menu/Programs")).expect("legacy");

    let dirs = prefix.start_menu_dirs();
    assert_eq!(dirs.len(), 3, "all three layouts are found: {dirs:?}");
    assert!(
        dirs.iter()
            .any(|d| d.to_string_lossy().contains("AppData/Roaming"))
    );
    assert!(
        dirs.iter()
            .any(|d| d.to_string_lossy().contains("ProgramData"))
    );
}

#[test]
fn launch_env_sets_the_prefix_and_suppresses_addon_downloads() {
    let prefix = Prefix::new("/home/u/.local/share/tahoe/wine/app");
    let env: Vec<(String, String)> = prefix
        .launch_env()
        .into_iter()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert!(env.contains(&(
        "WINEPREFIX".to_owned(),
        "/home/u/.local/share/tahoe/wine/app".to_owned()
    )));
    // Without this, a first launch pops the Gecko/Mono download dialog.
    assert!(env.contains(&("WINEDLLOVERRIDES".to_owned(), "mscoree,mshtml=".to_owned())));
}

#[test]
fn per_application_prefixes_are_isolated_and_sanitised() {
    let a = Prefix::for_application("My App 2.0");
    let b = Prefix::for_application("my-app-2.0");
    assert_ne!(
        a.path.file_name(),
        b.path.file_name(),
        "different labels must not collide on disk"
    );
    for p in [&a, &b] {
        let component = p.path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(!component.contains('/'), "{component}");
        assert!(!component.contains(".."), "{component}");
    }
}

#[test]
fn sanitize_component_refuses_traversal_rather_than_laundering_it() {
    // Anything that looks like a path is refused outright, so a
    // half-cleaned value can never later be mistaken for a path.
    for hostile in [
        "../../etc/passwd",
        "..",
        "a/b",
        "a\\b",
        "..\\..\\windows",
        "/absolute",
    ] {
        assert_eq!(
            sanitize_component(hostile),
            "app",
            "hostile input: {hostile:?}"
        );
    }
    // Ordinary labels survive as one component with no separators.
    assert_eq!(sanitize_component("Normal Name"), "Normal-Name");
    assert_eq!(sanitize_component("keep-me_1.2"), "keep-me_1.2");
    assert_eq!(sanitize_component(""), "app");
    assert_eq!(sanitize_component("---"), "app");
}

// ---------------------------------------------------------------------------
// Start Menu discovery
// ---------------------------------------------------------------------------

#[test]
fn discovers_applications_from_the_modern_start_menu() {
    let s = Scratch::new("discover");
    let prefix = initialised_prefix(&s, true);
    s.file(
        "drive_c/users/tester/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Widget Studio.lnk",
        b"not a real lnk",
    );
    s.file(
        "drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs/All Users Tool.lnk",
        b"nope",
    );
    // Nested vendor folder, as installers create.
    s.file(
        "drive_c/users/tester/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Vendor/Product/Deep.lnk",
        b"x",
    );

    let apps = prefix.applications();
    let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"Widget Studio"), "{names:?}");
    assert!(names.contains(&"All Users Tool"), "{names:?}");
    assert!(
        names.contains(&"Deep"),
        "nested menus are walked: {names:?}"
    );
    assert_eq!(apps.len(), 3);
}

#[test]
fn ignores_non_shortcut_files_in_the_start_menu() {
    let s = Scratch::new("nonshortcut");
    let prefix = initialised_prefix(&s, true);
    let menu = "drive_c/users/tester/AppData/Roaming/Microsoft/Windows/Start Menu/Programs";
    s.file(&format!("{menu}/app.lnk"), b"x");
    s.file(&format!("{menu}/readme.txt"), b"x");
    s.file(&format!("{menu}/notes.ini"), b"x");
    s.file(&format!("{menu}/website.url"), b"x");
    let apps = prefix.applications();
    let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"app"));
    assert!(names.contains(&"website"), ".url shortcuts are apps too");
    assert!(!names.contains(&"readme"));
    assert!(!names.contains(&"notes"));
}

#[test]
fn strips_installer_suffixes_from_display_names() {
    let s = Scratch::new("suffix");
    let prefix = initialised_prefix(&s, true);
    let menu = "drive_c/users/tester/AppData/Roaming/Microsoft/Windows/Start Menu/Programs";
    s.file(&format!("{menu}/Widget - Shortcut.lnk"), b"x");
    s.file(&format!("{menu}/Widget Uninstall.lnk"), b"x");
    let names: Vec<String> = prefix.applications().into_iter().map(|a| a.name).collect();
    assert!(names.contains(&"Widget".to_owned()), "{names:?}");
    assert!(
        names.contains(&"Widget".to_owned()) && !names.iter().any(|n| n.contains("Uninstall")),
        "{names:?}"
    );
}

#[test]
fn clean_display_name_handles_the_known_suffixes() {
    assert_eq!(clean_display_name("Game - Shortcut"), "Game");
    assert_eq!(clean_display_name("Game (uninstall)"), "Game");
    assert_eq!(clean_display_name("Game Uninstall"), "Game");
    assert_eq!(clean_display_name("  Spaced  "), "Spaced");
    assert_eq!(clean_display_name("Plain"), "Plain");
}

#[test]
fn discovered_apps_are_marked_as_a_compatibility_layer() {
    let s = Scratch::new("caveat");
    let prefix = initialised_prefix(&s, true);
    s.file(
        "drive_c/users/tester/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Thing.lnk",
        b"x",
    );
    let apps = prefix.applications();
    assert_eq!(apps.len(), 1);
    assert!(
        apps[0].caveats.iter().any(|c| c.contains("not Windows")),
        "every Wine app must carry the honesty caveat: {:?}",
        apps[0].caveats
    );
    assert_eq!(apps[0].origin, AppOrigin::WineStartMenu);
    assert!(matches!(apps[0].launch, Launch::Windows { .. }));
}

#[test]
fn launch_targets_the_shortcut_so_wine_resolves_it() {
    let s = Scratch::new("target");
    let prefix = initialised_prefix(&s, true);
    let lnk = s.file(
        "drive_c/users/tester/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Thing.lnk",
        b"x",
    );
    let apps = prefix.applications();
    match &apps[0].launch {
        Launch::Windows { prefix: p, program } => {
            assert_eq!(p, &prefix.path);
            assert_eq!(
                program, &lnk,
                "the .lnk itself is the target: `wine start` resolves it correctly"
            );
        }
        other => panic!("expected a Windows launch, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Shortcut parsing
// ---------------------------------------------------------------------------

/// Build a `.lnk` with the given string-data fields.
fn build_lnk(flags: u32, file_attributes: u32, strings: &[&str]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&0x4Cu32.to_le_bytes());
    b.extend_from_slice(&LNK_CLSID);
    b.extend_from_slice(&flags.to_le_bytes());
    b.extend_from_slice(&file_attributes.to_le_bytes());
    b.extend_from_slice(&[0u8; 0x4C - 0x1C]); // timestamps, size, icon, show, hotkey
    assert_eq!(b.len(), 0x4C);
    for s in strings {
        let utf16: Vec<u16> = s.encode_utf16().collect();
        b.extend_from_slice(&(utf16.len() as u16).to_le_bytes());
        for unit in utf16 {
            b.extend_from_slice(&unit.to_le_bytes());
        }
    }
    b
}

#[test]
fn parses_a_shortcuts_string_data() {
    // NAME_STRING | RELATIVE_PATH | WORKING_DIR == 0x4 | 0x8 | 0x10
    let lnk = build_lnk(
        0x1C,
        0,
        &[
            "Widget Studio",
            ".\\widget.exe",
            "C:\\Program Files\\Widget",
        ],
    );
    let parsed = parse_shortcut(&lnk);
    assert!(parsed.valid);
    assert_eq!(parsed.name.as_deref(), Some("Widget Studio"));
    assert_eq!(parsed.relative_path.as_deref(), Some(".\\widget.exe"));
    assert_eq!(
        parsed.working_dir.as_deref(),
        Some("C:\\Program Files\\Widget")
    );
    assert!(!parsed.targets_directory());
}

#[test]
fn detects_a_shortcut_that_points_at_a_directory() {
    let lnk = build_lnk(0x4, 0x10, &["Some Folder"]);
    assert!(parse_shortcut(&lnk).targets_directory());
}

#[test]
fn skips_a_link_target_id_list_and_link_info() {
    // HasLinkTargetIDList | HasLinkInfo | NAME_STRING
    let mut b = Vec::new();
    b.extend_from_slice(&0x4Cu32.to_le_bytes());
    b.extend_from_slice(&LNK_CLSID);
    b.extend_from_slice(&(0x1u32 | 0x2 | 0x4).to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&[0u8; 0x4C - 0x1C]);
    // IDList: 4 bytes of payload.
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&[0xAA; 4]);
    // LinkInfo: 8 bytes total.
    b.extend_from_slice(&8u32.to_le_bytes());
    b.extend_from_slice(&[0xBB; 4]);
    // NAME_STRING "Hi"
    let utf16: Vec<u16> = "Hi".encode_utf16().collect();
    b.extend_from_slice(&(utf16.len() as u16).to_le_bytes());
    for unit in utf16 {
        b.extend_from_slice(&unit.to_le_bytes());
    }

    let parsed = parse_shortcut(&b);
    assert!(
        parsed.valid,
        "the walk must skip both variable-length blocks"
    );
    assert_eq!(parsed.name.as_deref(), Some("Hi"));
}

#[test]
fn rejects_a_file_that_is_not_a_shortcut() {
    assert!(!parse_shortcut(b"").valid);
    assert!(
        !parse_shortcut(&[0u8; 200]).valid,
        "zeroed header is not a shortcut"
    );
    let mut wrong_size = build_lnk(0x4, 0, &["x"]);
    wrong_size[0] = 0x00;
    assert!(
        !parse_shortcut(&wrong_size).valid,
        "a wrong header size must be refused"
    );
}

#[test]
fn survives_a_truncated_shortcut_without_panicking() {
    let full = build_lnk(0x1C, 0, &["Name", "Path", "Dir"]);
    for cut in 0..full.len() {
        let _ = parse_shortcut(&full[..cut]);
    }
}

#[test]
fn a_hostile_id_list_length_does_not_run_off_the_end() {
    let mut b = Vec::new();
    b.extend_from_slice(&0x4Cu32.to_le_bytes());
    b.extend_from_slice(&LNK_CLSID);
    b.extend_from_slice(&0x7u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&[0u8; 0x4C - 0x1C]);
    b.extend_from_slice(&0xFFFFu16.to_le_bytes()); // claims 65535 bytes
    b.extend_from_slice(&[0xAA; 8]);
    let parsed = parse_shortcut(&b);
    // The walk gives up rather than reading past the buffer.
    assert!(parsed.name.is_none());
}

// ---------------------------------------------------------------------------
// winemenubuilder .desktop output
// ---------------------------------------------------------------------------

const WINEMENUBUILDER_ENTRY: &str = r#"
[Desktop Entry]
Name=Widget Studio
Exec=env "WINEPREFIX=/home/u/.wine" wine "C:\\Program Files\\Widget\\widget.exe"
Type=Application
StartupNotify=true
Icon=1234_widget.0
StartupWMClass=widget.exe
"#;

#[test]
fn parses_a_winemenubuilder_desktop_entry() {
    let prefix = Path::new("/home/u/.wine");
    let app = parse_wine_desktop_entry(WINEMENUBUILDER_ENTRY, Path::new("/x.desktop"), prefix)
        .expect("entry belongs to this prefix");
    assert_eq!(app.name, "Widget Studio");
    assert_eq!(app.icon.as_deref(), Some("1234_widget.0"));
    assert_eq!(
        app.id, "widget.exe",
        "StartupWMClass makes the dock grouping work"
    );
    assert_eq!(app.origin, AppOrigin::WineDesktopEntry);
    match &app.launch {
        Launch::Windows { program, prefix: p } => {
            assert_eq!(p, prefix);
            assert_eq!(
                program.to_string_lossy(),
                "C:\\Program Files\\Widget\\widget.exe"
            );
        }
        other => panic!("expected Windows launch, got {other:?}"),
    }
}

#[test]
fn an_entry_for_a_different_prefix_is_refused() {
    let app = parse_wine_desktop_entry(
        WINEMENUBUILDER_ENTRY,
        Path::new("/x.desktop"),
        Path::new("/home/u/.local/share/tahoe/wine/other"),
    );
    assert!(
        app.is_none(),
        "a multi-prefix system must not cross-match entries"
    );
}

#[test]
fn an_entry_without_an_exec_line_is_refused() {
    let text = "[Desktop Entry]\nName=Broken\nType=Application\n";
    assert!(parse_wine_desktop_entry(text, Path::new("/x"), Path::new("/home/u/.wine")).is_none());
}

#[test]
fn parses_quoted_exec_lines_per_the_desktop_entry_spec() {
    // The Desktop Entry spec escapes a literal backslash as `\\` inside a
    // quoted argument, and that is the form `winemenubuilder` writes to
    // disk. A naive whitespace split gets this wrong in both directions: it
    // breaks on the spaces AND on the escapes.
    let words = shell_words(
        "env \"WINEPREFIX=/home/u/My Wine\" wine \"C:\\\\Program Files\\\\App\\\\a.exe\" --flag",
    );
    assert_eq!(words[0], "env");
    assert_eq!(words[1], "WINEPREFIX=/home/u/My Wine");
    assert_eq!(words[2], "wine");
    assert_eq!(
        words[3], r"C:\Program Files\App\a.exe",
        "the doubled backslashes must collapse to the real Windows path"
    );
    assert_eq!(words[4], "--flag");
}

#[test]
fn shell_words_never_evaluates_a_shell_metacharacter() {
    // A .desktop line is data. `$(...)` must survive as literal text.
    let words = shell_words("wine \"$(touch /tmp/pwned)\"");
    assert_eq!(
        words,
        vec!["wine".to_owned(), "$(touch /tmp/pwned)".to_owned()]
    );
    assert!(!Path::new("/tmp/pwned").exists());
}

#[test]
fn extracts_the_wine_target_from_a_generated_exec_line() {
    assert_eq!(
        extract_wine_exec_target("env \"WINEPREFIX=/p\" wine \"C:\\\\a\\\\b.exe\""),
        Some(PathBuf::from(r"C:\a\b.exe"))
    );
    assert_eq!(
        extract_wine_exec_target(r#""/usr/bin/wine" "/p/c.exe""#),
        Some(PathBuf::from("/p/c.exe"))
    );
    // Nothing to launch: refuse rather than guess.
    assert_eq!(extract_wine_exec_target("env foo bar"), None);
    assert_eq!(extract_wine_exec_target("wine --version"), None);
    assert_eq!(extract_wine_exec_target(""), None);
}

#[test]
fn two_sources_deduplicate_on_the_launch_target() {
    let s = Scratch::new("dedup");
    let _prefix = initialised_prefix(&s, true);
    let lnk = s.file(
        "drive_c/users/tester/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Thing.lnk",
        b"x",
    );
    // A `.desktop` entry pointing at the same shortcut must not double up.
    let text = format!(
        "[Desktop Entry]\nName=Thing\nExec=env \"WINEPREFIX={}\" wine \"{}\"\nType=Application\n",
        s.path.display(),
        lnk.display()
    );
    let prefix = Prefix::new(&s.path);
    let mut combined = prefix.start_menu_applications();
    let seen: BTreeSet<PathBuf> = combined
        .iter()
        .filter_map(|a| match &a.launch {
            Launch::Windows { program, .. } => Some(program.clone()),
            _ => None,
        })
        .collect();
    if let Some(app) = parse_wine_desktop_entry(&text, Path::new("/x"), &s.path)
        && let Launch::Windows { program, .. } = &app.launch
        && !seen.contains(program)
    {
        combined.push(app);
    }
    assert_eq!(
        combined.len(),
        1,
        "the same target appears once: {combined:?}"
    );
}

// ---------------------------------------------------------------------------
// Runtime detection
// ---------------------------------------------------------------------------

#[test]
fn runtime_detection_never_panics_and_is_self_consistent() {
    let rt = WineRuntime::detect();
    if rt.is_available() {
        assert!(rt.loader.as_ref().is_some_and(|p| p.is_file()));
    }
    if let Some(v) = &rt.version {
        assert!(v.contains("wine"), "unexpected version string: {v}");
    }
    assert!(!rt.describe().is_empty());
}

#[test]
fn ntsync_support_is_judged_from_the_kernel_version() {
    // Wine 11 uses NTSync automatically when the device exists; the kernel
    // check only tells a user whether upgrading could help.
    let mk = |release: &str| WineRuntime {
        loader: None,
        version: None,
        ntsync_available: false,
        kernel: release.to_owned(),
    };
    assert!(mk("6.14.0-arch1-1").kernel_supports_ntsync());
    assert!(mk("6.18.55-1-cachyos-lts").kernel_supports_ntsync());
    assert!(mk("7.2.0").kernel_supports_ntsync());
    assert!(!mk("6.13.9").kernel_supports_ntsync());
    assert!(!mk("5.15.216").kernel_supports_ntsync());
    assert!(!mk("").kernel_supports_ntsync());
}

#[test]
fn describe_explains_a_missing_ntsync_device_separately_from_an_old_kernel() {
    let new_kernel_no_device = WineRuntime {
        loader: None,
        version: None,
        ntsync_available: false,
        kernel: "6.18.0".to_owned(),
    };
    let text = new_kernel_no_device.describe();
    assert!(text.contains("load the ntsync module"), "{text}");

    let old_kernel = WineRuntime {
        loader: None,
        version: None,
        ntsync_available: false,
        kernel: "6.6.0".to_owned(),
    };
    let text = old_kernel.describe();
    assert!(text.contains("needs kernel >="), "{text}");
}

#[test]
fn prefix_env_list_covers_the_documented_variables_only() {
    for expected in ["WINEPREFIX", "WINEARCH", "WINEDEBUG", "WINEDLLOVERRIDES"] {
        assert!(
            PREFIX_ENV.contains(&expected),
            "{expected} should be documented"
        );
    }
    // These are Proton/wine-staging only and must not be advertised as
    // upstream Wine variables.
    assert!(!PREFIX_ENV.contains(&"WINEESYNC"));
    assert!(!PREFIX_ENV.contains(&"WINEFSYNC"));
}

#[test]
fn desktop_entry_scanning_uses_the_xdg_data_dirs() {
    let dirs = xdg_data_dirs();
    assert!(!dirs.is_empty());
    assert!(
        dirs[0].is_absolute(),
        "the first entry must be XDG_DATA_HOME: {dirs:?}"
    );
    assert!(dirs.iter().all(|d| d.is_absolute()));
}
