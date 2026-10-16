// SPDX-License-Identifier: MIT OR Apache-2.0
//! Proton integration tests.
//!
//! Two things are worth testing here and one is worth *not*:
//!
//! - The **verb set** is pinned. Getting a verb wrong is a silent failure
//!   ("Need a verb." and exit 1) that a launcher would report as a crash.
//! - The **env and argv shapes** are pinned, because Proton reads
//!   `STEAM_COMPAT_CLIENT_INSTALL_PATH` unguarded and raises a `KeyError`
//!   rather than printing anything useful.
//! - What is *not* tested is whether Proton runs a program well. That is
//!   Proton's compatibility, and it is measured elsewhere.

use super::*;
use std::ffi::OsString;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("tahoe-proton-tests")
        .join(format!("{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

/// A directory shaped like a real Proton build: a `proton` script at the root.
fn fake_install(root: &Path, name: &str) -> ProtonInstall {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("install dir");
    std::fs::write(dir.join("proton"), b"#!/usr/bin/env python3\n").expect("proton script");
    let flavour = ProtonFlavour::from_path(&dir);
    ProtonInstall {
        root: dir,
        flavour,
        umu: None,
    }
}

// ---------------------------------------------------------------------------
// The verb set — exact
// ---------------------------------------------------------------------------

#[test]
fn the_verb_set_is_exactly_upstreams() {
    // Six verbs. Anything else exits 1 with "Need a verb.", so a launcher
    // that invents one gets a failure it will misreport.
    let all = [
        Verb::Run,
        Verb::WaitForExitAndRun,
        Verb::RunInPrefix,
        Verb::DestroyPrefix,
        Verb::GetCompatPath,
        Verb::GetNativePath,
    ];
    let rendered: Vec<&str> = all.iter().map(|v| v.as_str()).collect();
    assert_eq!(
        rendered,
        vec![
            "run",
            "waitforexitandrun",
            "runinprefix",
            "destroyprefix",
            "getcompatpath",
            "getnativepath"
        ]
    );
}
// NOTE: `getpwfmode` is not among them. It does not exist on any Proton
// branch, and several guides mention it. The absence is deliberate.

#[test]
fn the_verb_used_for_a_launch_is_the_waiting_one() {
    // `run` does not wait for a previous session to finish; a launcher that
    // wants an exit code needs `waitforexitandrun`.
    assert_eq!(Verb::WaitForExitAndRun.as_str(), "waitforexitandrun");
}

// ---------------------------------------------------------------------------
// argv
// ---------------------------------------------------------------------------

#[test]
fn the_argv_is_script_verb_program_args() {
    let root = scratch("argv");
    let install = fake_install(&root, "GE-Proton11-9");
    let args: Vec<OsString> = ["--silent", "/install"].iter().map(Into::into).collect();
    let argv = install
        .argv(Verb::WaitForExitAndRun, Path::new("/games/app.exe"), &args)
        .expect("argv");
    let rendered: Vec<String> = argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(rendered[0], install.script().to_string_lossy());
    assert_eq!(rendered[1], "waitforexitandrun");
    assert_eq!(rendered[2], "/games/app.exe");
    assert_eq!(rendered[3], "--silent");
    assert_eq!(rendered[4], "/install");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn no_double_dash_is_inserted() {
    // The Python script appends `sys.argv[2:]` directly and never reads a
    // `--`, so inserting one would pass a literal `--` to the program.
    let root = scratch("nodashes");
    let install = fake_install(&root, "Proton - Experimental");
    let argv = install
        .argv(Verb::Run, Path::new("/a.exe"), &[])
        .expect("argv");
    assert!(!argv.iter().any(|a| a == "--"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_missing_script_is_a_typed_error() {
    // A half-extracted build must not produce an argv that fails later with
    // something unhelpful.
    let install = ProtonInstall {
        root: PathBuf::from("/definitely/not/a/proton/build"),
        flavour: ProtonFlavour::Unknown,
        umu: None,
    };
    match install.argv(Verb::Run, Path::new("/a.exe"), &[]) {
        Err(ProtonError::ScriptMissing(p)) => assert!(p.ends_with("proton")),
        other => panic!("expected ScriptMissing, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

#[test]
fn both_required_variables_are_set() {
    // `STEAM_COMPAT_DATA_PATH` absent means `exit 1`.
    // `STEAM_COMPAT_CLIENT_INSTALL_PATH` absent means a KeyError traceback.
    let data = PathBuf::from("/tmp/tahoe-compat-data");
    let env = ProtonInstall::environment(&data);
    let keys: Vec<String> = env
        .iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    assert!(
        keys.contains(&"STEAM_COMPAT_DATA_PATH".to_owned()),
        "{keys:?}"
    );
    assert!(
        keys.contains(&"STEAM_COMPAT_CLIENT_INSTALL_PATH".to_owned()),
        "{keys:?}"
    );
}

#[test]
fn the_data_path_is_passed_through_unchanged() {
    let data = PathBuf::from("/tmp/tahoe-compat-data");
    let env = ProtonInstall::environment(&data);
    let value = env
        .iter()
        .find(|(k, _)| k == "STEAM_COMPAT_DATA_PATH")
        .map(|(_, v)| v.clone())
        .expect("data path");
    assert_eq!(PathBuf::from(value), data);
}

#[test]
fn pwd_is_set_and_shaped_like_steams_tree() {
    // `protonfixes` does `re.findall(r'.*/steamapps', os.environ['PWD'])[-1]`,
    // which raises IndexError when PWD has no `/steamapps` in it. Both the
    // prefix's shape and this variable exist for that reason.
    let data = ProtonInstall::data_path_for("Some Game");
    assert!(
        data.to_string_lossy().contains("/steamapps/"),
        "the data path must be shaped like Steam's tree: {}",
        data.display()
    );
    let env = ProtonInstall::environment(&data);
    let pwd = env
        .iter()
        .find(|(k, _)| k == "PWD")
        .map(|(_, v)| PathBuf::from(v))
        .expect("PWD must be set; protonfixes reads it unguarded");
    let text = pwd.to_string_lossy().into_owned();
    assert!(
        text.contains("/steamapps"),
        "PWD must contain /steamapps: {text}"
    );
    assert!(
        text.chars().any(|c| c.is_ascii_digit()),
        "and the digits: {text}"
    );
}

#[test]
fn winprefix_is_never_set() {
    // Proton overwrites it with `<data_path>/pfx` unconditionally. Setting it
    // would be misleading rather than harmful, which is worse.
    let env = ProtonInstall::environment(Path::new("/tmp/x"));
    assert!(!env.iter().any(|(k, _)| k == "WINEPREFIX"));
}

#[test]
fn the_real_prefix_has_the_pfx_suffix() {
    // Not `STEAM_COMPAT_DATA_PATH` itself: Proton appends `pfx`, and a
    // launcher that assumed otherwise would inspect an empty directory.
    assert_eq!(
        ProtonInstall::prefix_of(Path::new("/data/bottle")),
        PathBuf::from("/data/bottle/pfx")
    );
}

#[test]
fn the_client_install_path_always_resolves_to_something() {
    // Proton only checks that it exists, so returning a non-existent path
    // would produce the KeyError this function exists to prevent.
    let data = scratch("clientpath");
    let missing = data.join("no-steam-here");
    let resolved = ProtonInstall::client_install_path_in(&data, &[missing]);
    assert!(
        resolved.is_dir(),
        "resolved to {} which does not exist",
        resolved.display()
    );
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn no_game_specific_variables_are_set() {
    // Setting SteamAppId would put Proton into its per-appid workaround
    // table. A non-Steam target must not be given a game's identity.
    let env = ProtonInstall::environment(Path::new("/tmp/x"));
    for forbidden in ["SteamAppId", "SteamGameId", "STEAM_COMPAT_APP_ID"] {
        assert!(
            !env.iter().any(|(k, _)| k == forbidden),
            "{forbidden} must not be set"
        );
    }
}

// ---------------------------------------------------------------------------
// umu, the supported route
// ---------------------------------------------------------------------------

#[test]
fn umu_uses_its_own_variable_names() {
    let root = scratch("umu");
    let mut install = fake_install(&root, "proton-cachyos-11.0");
    install.umu = Some(PathBuf::from("/usr/bin/umu-run"));
    let (program, env, argv) = install
        .umu_argv(Path::new("/data"), Path::new("/games/app.exe"), &[])
        .expect("umu argv");
    assert_eq!(program, PathBuf::from("/usr/bin/umu-run"));
    let keys: Vec<String> = env
        .iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    // umu consumes PROTONPATH and GAMEID; Proton does not read either.
    assert!(keys.contains(&"PROTONPATH".to_owned()), "{keys:?}");
    assert!(keys.contains(&"GAMEID".to_owned()), "{keys:?}");
    assert!(keys.contains(&"PROTON_VERB".to_owned()), "{keys:?}");
    assert!(
        keys.contains(&"STEAM_COMPAT_DATA_PATH".to_owned()),
        "{keys:?}"
    );
    // umu takes the program directly; the verb is an environment variable.
    assert_eq!(argv[0], OsString::from("/games/app.exe"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn protonpath_points_at_the_build() {
    let root = scratch("protonpath");
    let mut install = fake_install(&root, "proton-cachyos-11.0");
    install.umu = Some(PathBuf::from("/usr/bin/umu-run"));
    let (_, env, _) = install
        .umu_argv(Path::new("/data"), Path::new("/a.exe"), &[])
        .expect("umu argv");
    let value = env
        .iter()
        .find(|(k, _)| k == "PROTONPATH")
        .map(|(_, v)| PathBuf::from(v))
        .expect("PROTONPATH");
    assert_eq!(value, install.root);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn umu_without_umu_is_an_error_not_a_silent_fallback() {
    // Falling back to a direct invocation would be a different, unsupported
    // execution path chosen without telling anyone.
    let root = scratch("noumu");
    let install = fake_install(&root, "Proton");
    assert!(
        install
            .umu_argv(Path::new("/d"), Path::new("/a.exe"), &[])
            .is_err()
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// Discovery and flavour
// ---------------------------------------------------------------------------

#[test]
fn flavours_are_recognised_from_their_paths() {
    let cases = [
        (
            "/home/u/.local/share/Steam/compatibilitytools.d/GE-Proton11-9",
            ProtonFlavour::GloriousEggroll,
        ),
        (
            "/usr/share/steam/compatibilitytools.d/proton-cachyos-slr",
            ProtonFlavour::CachyOs,
        ),
        (
            "/home/u/.steam/steam/steamapps/common/Proton - Experimental",
            ProtonFlavour::Valve,
        ),
        ("/opt/something-else", ProtonFlavour::Unknown),
    ];
    for (path, expected) in cases {
        assert_eq!(
            ProtonFlavour::from_path(Path::new(path)),
            expected,
            "{path}"
        );
    }
}

#[test]
fn flavour_detection_is_case_insensitive() {
    assert_eq!(
        ProtonFlavour::from_path(Path::new("/x/PROTON-CACHYOS-slr")),
        ProtonFlavour::CachyOs
    );
    assert_eq!(
        ProtonFlavour::from_path(Path::new("/x/ge-proton10-1")),
        ProtonFlavour::GloriousEggroll
    );
}

#[test]
fn every_flavour_explains_itself() {
    // The note ends up in a user-visible caveat, so it must say something.
    for flavour in [
        ProtonFlavour::Valve,
        ProtonFlavour::CachyOs,
        ProtonFlavour::GloriousEggroll,
        ProtonFlavour::Unknown,
    ] {
        let note = flavour.note();
        assert!(note.len() > 20, "{flavour:?} note is uninformative: {note}");
    }
}

#[test]
fn discovery_honours_the_explicit_override() {
    // The override is what makes this testable and what lets a user point
    // Tahoe at a build it would not otherwise find.
    let root = scratch("discover");
    fake_install(&root, "proton-cachyos-11.0");
    let found = ProtonInstall::discover_in(std::slice::from_ref(&root), None);
    assert!(
        found
            .iter()
            .any(|i| i.root == root.join("proton-cachyos-11.0")),
        "the override directory was not searched: {found:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn discovery_prefers_the_distributions_own_build() {
    let root = scratch("prefer");
    fake_install(&root, "GE-Proton11-9");
    fake_install(&root, "proton-cachyos-slr");
    fake_install(&root, "Proton - Experimental");
    let found = ProtonInstall::discover_in(std::slice::from_ref(&root), None);
    let first = found.first().expect("at least one build");
    assert_eq!(
        first.flavour,
        ProtonFlavour::CachyOs,
        "the distro's own build should win; got {first:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn discovery_ignores_directories_without_a_proton_script() {
    let root = scratch("noscript");
    std::fs::create_dir_all(root.join("not-a-proton")).expect("dir");
    let found = ProtonInstall::discover_in(std::slice::from_ref(&root), None);
    assert!(
        !found.iter().any(|i| i.root.ends_with("not-a-proton")),
        "a directory with no proton script is not a build"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_stable_id_is_always_a_number_in_a_plausible_range() {
    for name in [
        "",
        "a",
        "My Game",
        "../../etc",
        "ünïcödé",
        "x".repeat(1000).as_str(),
    ] {
        let id = stable_id(name);
        assert!(
            (100_000_000..=1_000_000_000).contains(&id),
            "{name:?} -> {id}"
        );
    }
    // Different names should not collide in practice.
    assert_ne!(stable_id("game-a"), stable_id("game-b"));
}

#[test]
fn per_target_data_paths_are_isolated_and_sanitised() {
    let a = ProtonInstall::data_path_for("My Game 2.0");
    let b = ProtonInstall::data_path_for("my-game-2.0");
    assert_ne!(
        a.file_name(),
        b.file_name(),
        "different labels must not collide"
    );
    for p in [&a, &b] {
        let component = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(!component.contains('/'), "{component}");
    }
    // The digits are load-bearing: protonfixes reads them out of the path
    // with `re.findall(r'\d+', ...)` and raises IndexError without them.
    // This was found by running Proton, not by reading it.
    for p in [&a, &b] {
        let component = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            component.chars().any(|c| c.is_ascii_digit()),
            "{component} has no digits; protonfixes would raise IndexError"
        );
    }
    assert_eq!(
        stable_id("My Game 2.0"),
        stable_id("My Game 2.0"),
        "must be stable"
    );
    // A traversal attempt is refused rather than cleaned, the same rule the
    // Wine prefix uses. The numeric suffix is appended to the sanitised
    // name, so the component starts with `app-` rather than being `app`.
    let hostile = ProtonInstall::data_path_for("../../etc");
    let component = hostile.file_name().unwrap().to_string_lossy().into_owned();
    assert!(component.starts_with("app-"), "{component}");
    assert!(!component.contains(".."), "{component}");
    assert!(!component.contains('/'), "{component}");
}

#[test]
fn discovery_on_a_machine_with_nothing_installed_returns_empty() {
    // Must not panic or invent a build.
    let empty = scratch("empty");
    let found = ProtonInstall::discover_in(std::slice::from_ref(&empty), None);
    assert!(found.is_empty(), "{found:?}");
    let _ = std::fs::remove_dir_all(&empty);
}
