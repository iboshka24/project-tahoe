// SPDX-License-Identifier: MIT OR Apache-2.0
//! xtool integration tests.
//!
//! No Swift toolchain or iOS SDK exists on the machines this was developed
//! on, so the *build* path cannot be exercised here. What can be, and is:
//!
//! - prerequisite detection and the order in which problems are reported;
//! - `xtool.yml` parsing, against the real file committed in `apple/`;
//! - the exact `argv` a build would use;
//! - the project loader, against the real `apple/` project;
//! - inspection of a **real-shaped** iOS bundle built from the keys xtool's
//!   `Packer.swift` writes.
//!
//! The last one matters most. The bundle fixture is modelled on xtool's
//! source rather than captured from a run, and the test says so.

use super::*;
use crate::macos::BundleLayout;
use std::fs;

/// A scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join("tahoe-xtool-tests").join(format!(
            "{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        fs::write(&path, bytes).expect("write");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The real project committed in this repository.
fn real_project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apple")
}

// ---------------------------------------------------------------------------
// Version parsing
// ---------------------------------------------------------------------------

#[test]
fn parses_the_swift_version_banner() {
    assert_eq!(
        parse_swift_version("Swift version 6.4 (swift-6.4-RELEASE)"),
        Some((6, 4))
    );
    assert_eq!(
        parse_swift_version("Apple Swift version 5.9.2 (swiftlang)"),
        Some((5, 9))
    );
    assert_eq!(
        parse_swift_version("swift-driver version: 1.100.1 Swift version 6.0.3"),
        Some((6, 0))
    );
    assert_eq!(
        parse_swift_version("Target: x86_64-unknown-linux-gnu"),
        None
    );
    assert_eq!(parse_swift_version(""), None);
}

#[test]
fn version_parsing_does_not_trip_on_a_long_number() {
    // A build id like 20240101 must not be read as version 20240101.1.
    assert_eq!(
        parse_swift_version("Swift version 6.4.0-dev build 20240101"),
        Some((6, 4))
    );
}

// ---------------------------------------------------------------------------
// Toolchain detection
// ---------------------------------------------------------------------------

#[test]
fn detection_never_panics_and_is_self_consistent() {
    let t = AppleToolchain::detect();
    if let Some(swift) = &t.swift {
        assert!(swift.is_file());
        assert!(
            t.swift_version.is_some(),
            "a swift binary should answer --version"
        );
    }
    if t.xtool.is_some() {
        assert!(t.xtool.as_ref().is_some_and(|p| p.is_file()));
    }
    assert!(t.sdk_dir.is_absolute());
    assert!(!t.describe().is_empty());
}

#[test]
fn a_missing_swift_toolchain_is_reported_before_the_sdk() {
    // Someone with no Swift at all must not be told to download a 7 GB
    // Xcode archive. Ordering is the whole point of this check.
    let t = AppleToolchain {
        swift: None,
        swift_version: None,
        xtool: None,
        darwin_sdk_installed: false,
        sdk_dir: PathBuf::from("/home/u/.swiftpm/swift-sdks"),
        sdk_list: None,
    };
    assert_eq!(
        t.first_missing(),
        Some(MissingPrerequisite::SwiftNotInstalled)
    );
    let text = t.first_missing().expect("missing").to_string();
    assert!(
        text.contains("swift.org"),
        "the message must say where to get it: {text}"
    );
    assert!(
        !text.contains("Xcode.xip"),
        "and must not ask for the SDK first"
    );
}

#[test]
fn a_too_old_swift_is_named_with_both_versions() {
    let t = AppleToolchain {
        swift: Some(PathBuf::from("/usr/bin/swift")),
        swift_version: Some("Swift version 5.10 (swift-5.10-RELEASE)".to_owned()),
        xtool: Some(PathBuf::from("/usr/local/bin/xtool")),
        darwin_sdk_installed: true,
        sdk_dir: PathBuf::from("/h/.swiftpm/swift-sdks"),
        sdk_list: None,
    };
    match t.first_missing() {
        Some(MissingPrerequisite::SwiftTooOld { found, required }) => {
            assert_eq!(found, "5.10");
            assert_eq!(required, "6.4");
        }
        other => panic!("expected SwiftTooOld, got {other:?}"),
    }
}

#[test]
fn a_sufficient_swift_passes_but_a_missing_xtool_does_not() {
    let mk = |xtool: Option<PathBuf>, sdk: bool| AppleToolchain {
        swift: Some(PathBuf::from("/usr/bin/swift")),
        swift_version: Some("Swift version 6.4 (swift-6.4-RELEASE)".to_owned()),
        xtool,
        darwin_sdk_installed: sdk,
        sdk_dir: PathBuf::from("/h/.swiftpm/swift-sdks"),
        sdk_list: None,
    };
    assert_eq!(
        mk(None, true).first_missing(),
        Some(MissingPrerequisite::XtoolNotInstalled)
    );
    assert_eq!(
        mk(Some(PathBuf::from("/x")), false).first_missing(),
        Some(MissingPrerequisite::SdkNotInstalled)
    );
    assert_eq!(mk(Some(PathBuf::from("/x")), true).first_missing(), None);
    assert!(mk(Some(PathBuf::from("/x")), true).can_build());
}

#[test]
fn the_sdk_message_names_the_apple_id_requirement() {
    // This is the one prerequisite Tahoe cannot fetch, so the message has
    // to be explicit about why.
    let text = MissingPrerequisite::SdkNotInstalled.to_string();
    assert!(text.contains("Xcode.xip"), "{text}");
    assert!(text.contains("Apple ID"), "{text}");
    assert!(text.contains("xtool sdk install"), "{text}");
    assert!(text.contains("never downloads"), "{text}");
}

#[test]
fn a_newer_swift_is_accepted() {
    let t = AppleToolchain {
        swift: Some(PathBuf::from("/usr/bin/swift")),
        swift_version: Some("Swift version 6.6 (swift-6.6-RELEASE)".to_owned()),
        xtool: Some(PathBuf::from("/x")),
        darwin_sdk_installed: true,
        sdk_dir: PathBuf::from("/h"),
        sdk_list: None,
    };
    assert!(t.can_build(), "a newer toolchain must not be refused");
}

#[test]
fn an_unreadable_version_does_not_block_the_build() {
    // If `swift --version` fails we do not know the version; refusing on an
    // unknown is worse than trying, because xtool reports the real problem.
    let t = AppleToolchain {
        swift: Some(PathBuf::from("/usr/bin/swift")),
        swift_version: None,
        xtool: Some(PathBuf::from("/x")),
        darwin_sdk_installed: true,
        sdk_dir: PathBuf::from("/h"),
        sdk_list: None,
    };
    assert!(t.can_build());
}

// ---------------------------------------------------------------------------
// xtool.yml
// ---------------------------------------------------------------------------

#[test]
fn parses_the_committed_manifest() {
    let path = real_project_root().join("xtool.yml");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let manifest = XtoolManifest::parse(&text);
    assert_eq!(manifest.version, Some(1));
    assert_eq!(manifest.bundle_id.as_deref(), Some("org.tahoe.Gallery"));
    assert_eq!(manifest.product.as_deref(), Some("TahoeGallery"));
    assert_eq!(manifest.effective_bundle_id(), Some("org.tahoe.Gallery"));
    assert!(
        manifest.info_path.is_none(),
        "the manifest must not set infoPath: its merge behaviour is undocumented"
    );
}

#[test]
fn parses_a_minimal_manifest() {
    let m = XtoolManifest::parse("version: 1\norgID: com.example\nproduct: App\n");
    assert_eq!(m.version, Some(1));
    assert_eq!(m.org_id.as_deref(), Some("com.example"));
    assert_eq!(
        m.effective_bundle_id(),
        Some("com.example"),
        "orgID is the alternative to bundleID"
    );
    assert_eq!(m.product.as_deref(), Some("App"));
}

#[test]
fn bundle_id_wins_over_org_id() {
    let m = XtoolManifest::parse("bundleID: a.b\norgID: c.d\n");
    assert_eq!(m.effective_bundle_id(), Some("a.b"));
}

#[test]
fn unmodelled_keys_are_recorded_not_dropped() {
    let m = XtoolManifest::parse("product: A\niconPath: i.png\nentitlementsPath: e.plist\n");
    assert_eq!(m.product.as_deref(), Some("A"));
    assert!(m.unmodelled_keys.contains(&"iconPath".to_owned()));
    assert!(m.unmodelled_keys.contains(&"entitlementsPath".to_owned()));
}

#[test]
fn comments_blanks_and_oddities_are_ignored() {
    let m = XtoolManifest::parse("\n# a comment\n\n  \nproduct: Ok\nnot a mapping\n: no key\n");
    assert_eq!(m.product.as_deref(), Some("Ok"));
}

#[test]
fn quotes_are_stripped_from_values() {
    let m = XtoolManifest::parse("product: \"Quoted App\"\nbundleID: 'single.quoted'\n");
    assert_eq!(m.product.as_deref(), Some("Quoted App"));
    assert_eq!(m.bundle_id.as_deref(), Some("single.quoted"));
}

// ---------------------------------------------------------------------------
// Project loading
// ---------------------------------------------------------------------------

#[test]
fn loads_the_committed_project() {
    let root = real_project_root();
    if !root.is_dir() {
        eprintln!("note: {} is absent; skipping", root.display());
        return;
    }
    let project = AppleProject::load(&root).expect("load the committed project");
    assert_eq!(project.product(), "TahoeGallery");
    assert_eq!(project.app_path(), root.join("xtool/TahoeGallery.app"));
    assert_eq!(project.ipa_path(), root.join("xtool/TahoeGallery.ipa"));
}

#[test]
fn the_committed_package_declares_the_targets_the_project_needs() {
    let path = real_project_root().join("Package.swift");
    let text = fs::read_to_string(path).expect("read Package.swift");
    // xtool packages a *library* product, so the manifest must not declare
    // an executable.
    assert!(
        text.contains(".library(name: \"TahoeGallery\""),
        "xtool needs a library product"
    );
    assert!(
        !text.contains(".executableTarget"),
        "an executable target would not be packaged"
    );
    assert!(
        text.contains("\"TahoeDesign\""),
        "the token target must be declared"
    );
    assert!(text.contains("swift-tools-version: 6.0"));
    assert!(
        text.contains(".iOS(.v17)"),
        "the deployment target becomes MinimumOSVersion"
    );
}

#[test]
fn a_directory_without_a_manifest_is_not_a_project() {
    let s = Scratch::new("nomanifest");
    s.write("Package.swift", b"// swift-tools-version: 6.0\n");
    match AppleProject::load(&s.0) {
        Err(AppleBuildError::NotAProject { reason }) => {
            assert!(reason.contains("xtool.yml"), "{reason}")
        }
        other => panic!("expected NotAProject, got {other:?}"),
    }
}

#[test]
fn a_directory_without_a_package_is_not_a_project() {
    let s = Scratch::new("nopackage");
    s.write("xtool.yml", b"version: 1\nproduct: App\n");
    match AppleProject::load(&s.0) {
        Err(AppleBuildError::NotAProject { reason }) => {
            assert!(reason.contains("Package.swift"), "{reason}")
        }
        other => panic!("expected NotAProject, got {other:?}"),
    }
}

#[test]
fn a_manifest_with_no_product_is_refused() {
    let s = Scratch::new("noproduct");
    s.write("Package.swift", b"// swift-tools-version: 6.0\n");
    s.write("xtool.yml", b"version: 1\nbundleID: a.b\n");
    match AppleProject::load(&s.0) {
        Err(AppleBuildError::NotAProject { reason }) => {
            assert!(reason.contains("product"), "{reason}")
        }
        other => panic!("expected NotAProject, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Build command
// ---------------------------------------------------------------------------

fn project() -> AppleProject {
    AppleProject {
        root: PathBuf::from("/projects/Gallery"),
        manifest: XtoolManifest::parse(
            "version: 1\nbundleID: org.tahoe.Gallery\nproduct: TahoeGallery\n",
        ),
    }
}

fn render(argv: &[OsString]) -> Vec<String> {
    argv.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn the_default_build_command_is_unsigned_and_debug() {
    let argv = render(&project().build_argv(&BuildRequest::default()));
    assert_eq!(
        argv,
        vec!["xtool", "dev", "build", "--configuration", "debug"]
    );
    assert!(
        !argv.iter().any(|a| a == "--sign"),
        "signing needs an Apple ID and a device; a distro build must not"
    );
    assert!(
        !argv.iter().any(|a| a == "--ipa"),
        "an .app is what we can inspect"
    );
}

#[test]
fn the_release_build_sets_the_configuration() {
    let request = BuildRequest {
        configuration: Configuration::Release,
        ..BuildRequest::default()
    };
    let argv = render(&project().build_argv(&request));
    assert_eq!(argv[4], "release");
}

#[test]
fn an_ipa_build_is_available_but_off_by_default() {
    let request = BuildRequest {
        produce_ipa: true,
        ..BuildRequest::default()
    };
    let argv = render(&project().build_argv(&request));
    assert!(argv.contains(&"--ipa".to_owned()));
    assert_eq!(project().ipa_path().extension().unwrap(), "ipa");
}

#[test]
fn a_triple_override_is_passed_through() {
    let request = BuildRequest {
        triple: Some("arm64-apple-ios-simulator".into()),
        ..BuildRequest::default()
    };
    let argv = render(&project().build_argv(&request));
    let at = argv.iter().position(|a| a == "--triple").expect("--triple");
    assert_eq!(argv[at + 1], "arm64-apple-ios-simulator");
}

#[test]
fn a_build_without_prerequisites_is_refused_before_spawning() {
    let toolchain = AppleToolchain {
        swift: None,
        swift_version: None,
        xtool: None,
        darwin_sdk_installed: false,
        sdk_dir: PathBuf::from("/h"),
        sdk_list: None,
    };
    match project().build(&toolchain, &BuildRequest::default()) {
        Err(AppleBuildError::Prerequisite(MissingPrerequisite::SwiftNotInstalled)) => {}
        other => panic!("expected a prerequisite error, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Inspecting a real-shaped iOS bundle
// ---------------------------------------------------------------------------

/// Build an Info.plist with the keys xtool's `Packer.swift` writes.
///
/// Taken from xtool's source, **not** captured from a run. The test below
/// says so, so nobody mistakes this for end-to-end evidence.
fn xtool_style_plist(product: &str, bundle_id: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleExecutable</key><string>{product}</string>
    <key>CFBundleIdentifier</key><string>{bundle_id}</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>{product}</string>
    <key>CFBundleDisplayName</key><string>{product}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>CFBundleSupportedPlatforms</key><array><string>iPhoneOS</string></array>
    <key>MinimumOSVersion</key><string>17.0</string>
    <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
    <key>UILaunchScreen</key><dict/>
    <key>UIRequiredDeviceCapabilities</key><array><string>arm64</string></array>
    <key>UISupportedInterfaceOrientations</key>
    <array><string>UIInterfaceOrientationPortrait</string></array>
    <key>LSRequiresIPhoneOS</key><true/>
</dict>
</plist>
"#
    )
}

/// A 64-bit arm64 Mach-O with an iOS `LC_BUILD_VERSION` and two dylibs.
fn ios_arm64_macho() -> Vec<u8> {
    fn command(cmd: u32, body: &[u8]) -> Vec<u8> {
        let total = (8 + body.len() + 7) & !7;
        let mut out = Vec::new();
        out.extend_from_slice(&cmd.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(body);
        out.resize(total, 0);
        out
    }
    fn dylib(name: &str) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&24u32.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&0x10000u32.to_le_bytes());
        body.extend_from_slice(&0x10000u32.to_le_bytes());
        body.extend_from_slice(name.as_bytes());
        body.push(0);
        command(0xc, &body)
    }
    // LC_BUILD_VERSION with PLATFORM_IOS (2) and minos 17.0.
    let mut bv = Vec::new();
    bv.extend_from_slice(&2u32.to_le_bytes()); // platform = iOS
    bv.extend_from_slice(&0x0011_0000u32.to_le_bytes()); // 17.0.0
    bv.extend_from_slice(&0x0011_0000u32.to_le_bytes()); // sdk
    bv.extend_from_slice(&0u32.to_le_bytes()); // ntools

    let commands = [
        dylib("/System/Library/Frameworks/SwiftUI.framework/SwiftUI"),
        dylib("/usr/lib/libSystem.B.dylib"),
        command(0x32, &bv),
    ];
    let sizeofcmds: u32 = commands.iter().map(|c| c.len() as u32).sum();
    let mut out = Vec::new();
    out.extend_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]); // MH_MAGIC_64
    out.extend_from_slice(&0x0100_000Cu32.to_le_bytes()); // CPU_TYPE_ARM64
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes()); // MH_EXECUTE
    out.extend_from_slice(&(commands.len() as u32).to_le_bytes());
    out.extend_from_slice(&sizeofcmds.to_le_bytes());
    out.extend_from_slice(&0x20_0000u32.to_le_bytes()); // PIE
    out.extend_from_slice(&0u32.to_le_bytes());
    for c in &commands {
        out.extend_from_slice(c);
    }
    out
}

/// An iOS bundle: flat, with `Info.plist` and the executable at the root.
///
/// Not a `Contents/` tree — that is the macOS layout, and confusing the two
/// is the bug this fixture exists to catch.
fn fake_ios_bundle(s: &Scratch, product: &str, bundle_id: &str) -> PathBuf {
    let root = s.0.join(format!("{product}.app"));
    fs::create_dir_all(&root).expect("dirs");
    fs::write(
        root.join("Info.plist"),
        xtool_style_plist(product, bundle_id),
    )
    .expect("plist");
    fs::write(root.join(product), ios_arm64_macho()).expect("binary");
    root
}

#[test]
fn inspects_a_bundle_with_the_shape_xtool_produces() {
    let s = Scratch::new("inspect");
    let app = fake_ios_bundle(&s, "TahoeGallery", "org.tahoe.Gallery");

    let artifact = inspect_bundle(&app).expect("inspect");
    assert_eq!(artifact.bundle.executable_name, "TahoeGallery");
    assert_eq!(
        artifact.bundle.identifier.as_deref(),
        Some("org.tahoe.Gallery")
    );
    assert_eq!(artifact.bundle.package_type.as_deref(), Some("APPL"));
    assert_eq!(
        artifact.bundle.minimum_system_version.as_deref(),
        Some("17.0"),
        "xtool writes the deployment target here"
    );
    assert!(!artifact.bundle.is_agent, "a SwiftUI app is not an agent");

    assert_eq!(artifact.architecture.as_deref(), Some("arm64"));
    assert!(artifact.is_64_bit);
    assert!(
        !artifact.is_signed,
        "an unsigned build carries no LC_CODE_SIGNATURE"
    );
    assert_eq!(artifact.dependencies.len(), 2);
    assert!(
        artifact.dependencies.iter().any(|d| d.contains("SwiftUI")),
        "a SwiftUI app must link SwiftUI: {:?}",
        artifact.dependencies
    );
    assert!(
        artifact.summary().contains("TahoeGallery"),
        "{}",
        artifact.summary()
    );
}

#[test]
fn an_ios_binary_reports_no_macos_minimum() {
    // This is the correctness detail worth pinning: an iOS build carries
    // LC_BUILD_VERSION with PLATFORM_IOS. Reporting it as a macOS minimum
    // would tell a user their app needs "macOS 17.0", which does not exist.
    let s = Scratch::new("ios-platform");
    let app = fake_ios_bundle(&s, "Gallery", "org.tahoe.Gallery");
    let artifact = inspect_bundle(&app).expect("inspect");
    assert_eq!(artifact.minimum_macos, None);
    assert_eq!(artifact.platform, Some(2), "PLATFORM_IOS");
}

#[test]
fn a_bundle_without_an_executable_key_is_refused_clearly() {
    let s = Scratch::new("noexec");
    let root = s.0.join("Broken.app");
    fs::create_dir_all(&root).expect("dirs");
    fs::write(
        root.join("Info.plist"),
        "<plist version=\"1.0\"><dict/></plist>",
    )
    .expect("plist");
    match inspect_bundle(&root) {
        Err(BundleError::NoExecutableKey(_)) => {}
        other => panic!("expected NoExecutableKey, got {other:?}"),
    }
}

#[test]
fn a_bundle_whose_executable_is_not_macho_still_inspects() {
    // A bundle that exists but has an unreadable executable should report
    // what it could rather than failing outright: the caller may still want
    // the identifier and the icon.
    let s = Scratch::new("notmacho");
    let root = s.0.join("Odd.app");
    fs::create_dir_all(&root).expect("dirs");
    fs::write(root.join("Info.plist"), xtool_style_plist("Odd", "a.b")).expect("plist");
    fs::write(root.join("Odd"), b"\x7fELF not a macho at all").expect("binary");

    let artifact = inspect_bundle(&root).expect("the bundle itself is valid");
    assert_eq!(artifact.bundle.identifier.as_deref(), Some("a.b"));
    assert_eq!(artifact.architecture, None);
    assert!(artifact.dependencies.is_empty());
    assert!(!artifact.is_signed);
}

#[test]
fn a_universal_bundle_reports_the_selected_slice() {
    let s = Scratch::new("universal");
    let root = s.0.join("Fat.app");
    fs::create_dir_all(&root).expect("dirs");
    fs::write(root.join("Info.plist"), xtool_style_plist("Fat", "a.fat")).expect("plist");

    let arm = ios_arm64_macho();
    let mut x86 = ios_arm64_macho();
    x86[4..8].copy_from_slice(&0x0100_0007u32.to_le_bytes()); // CPU_TYPE_X86_64
    let mut fat = vec![0xca, 0xfe, 0xba, 0xbe];
    fat.extend_from_slice(&2u32.to_be_bytes());
    for (cputype, offset) in [(0x0100_0007u32, 0x1000u32), (0x0100_000Cu32, 0x2000)] {
        fat.extend_from_slice(&cputype.to_be_bytes());
        fat.extend_from_slice(&0u32.to_be_bytes());
        fat.extend_from_slice(&offset.to_be_bytes());
        fat.extend_from_slice(&0x400u32.to_be_bytes());
        fat.extend_from_slice(&12u32.to_be_bytes());
    }
    fat.resize(0x2400, 0);
    fat[0x1000..0x1000 + x86.len()].copy_from_slice(&x86);
    fat[0x2000..0x2000 + arm.len()].copy_from_slice(&arm);
    fs::write(root.join("Fat"), &fat).expect("binary");

    let artifact = inspect_bundle(&root).expect("inspect");
    assert_eq!(
        artifact.architecture.as_deref(),
        Some("arm64"),
        "arm64 is preferred for an Apple bundle"
    );
}

#[test]
fn the_two_bundle_layouts_are_distinguished() {
    // macOS: Contents/Info.plist + Contents/MacOS/<exe>.
    // iOS:   Info.plist + <exe>. xtool produces the second.
    let s = Scratch::new("layouts");
    let ios = fake_ios_bundle(&s, "Ios", "a.ios");
    assert_eq!(AppBundle::layout_of(&ios), Some(BundleLayout::Ios));
    let resolved = AppBundle::resolve(&ios).expect("resolve the iOS bundle");
    assert_eq!(resolved.layout, BundleLayout::Ios);
    assert_eq!(resolved.executable, ios.join("Ios"));

    let macos = s.0.join("Mac.app");
    fs::create_dir_all(macos.join("Contents/MacOS")).expect("dirs");
    fs::write(
        macos.join("Contents/Info.plist"),
        xtool_style_plist("Mac", "a.mac"),
    )
    .expect("plist");
    fs::write(macos.join("Contents/MacOS/Mac"), ios_arm64_macho()).expect("binary");
    assert_eq!(AppBundle::layout_of(&macos), Some(BundleLayout::MacOs));
    let resolved = AppBundle::resolve(&macos).expect("resolve the macOS bundle");
    assert_eq!(resolved.layout, BundleLayout::MacOs);
    assert_eq!(resolved.executable, macos.join("Contents/MacOS/Mac"));
}

#[test]
fn an_app_directory_with_no_plist_anywhere_is_not_a_bundle() {
    let s = Scratch::new("noplist");
    let root = s.0.join("Empty.app");
    fs::create_dir_all(root.join("Contents/MacOS")).expect("dirs");
    assert_eq!(AppBundle::layout_of(&root), None);
    assert!(!AppBundle::looks_like_bundle(&root));
}

#[test]
fn inspecting_a_non_bundle_is_a_typed_error() {
    let s = Scratch::new("notabundle");
    fs::create_dir_all(s.0.join("Plain")).expect("dir");
    assert!(matches!(
        inspect_bundle(&s.0.join("Plain")),
        Err(BundleError::NotABundle(_))
    ));
}
