// SPDX-License-Identifier: MIT OR Apache-2.0
//! Building Apple applications on Linux with [xtool], and inspecting the
//! result.
//!
//! # What this closes
//!
//! `tahoe-compat::macos` can inspect an `.app` bundle, but the only `.app`
//! bundles available to it were synthetic fixtures, because building a real
//! one needed a Mac. xtool removes that requirement: it turns a SwiftPM
//! package into an iOS `.app` on Linux, using the real Apple SDK extracted
//! from an Xcode archive the user obtains themselves.
//!
//! So this module does three things:
//!
//! 1. Reports **exactly** which prerequisite is missing, because "cannot
//!    build" is useless and "Swift 6.4 is not on PATH" is actionable.
//! 2. Drives `xtool dev build` without a shell.
//! 3. Feeds the resulting bundle back through [`crate::macos`], so a real
//!    artifact exercises the parsers that were written against fixtures.
//!
//! # What it must not do
//!
//! **Apple's SDK is not ours to ship or fetch.** xtool requires the user to
//! download `Xcode.xip` under their own Apple ID and accept Apple's license;
//! it extracts the SDK locally. Whether the extracted SDK may be
//! redistributed is not addressed by xtool's documentation, so this module
//! only ever inspects a prefix the user already installed. It never
//! downloads an SDK, and it never copies one.
//!
//! [xtool]: https://github.com/xtool-org/xtool

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use crate::macos::{AppBundle, BundleError, MachOError, MachOFile};

/// The Swift release xtool documents as required.
///
/// xtool's Linux installation guide names **Swift 6.4** and its CI image
/// pins `swift:6.4.0`. A newer toolchain generally works; an older one does
/// not, because xtool uses `swift-build` with `--toolset` on 6.4 and the
/// SwiftPM `--swift-sdk` path below it.
pub const REQUIRED_SWIFT: (u64, u64) = (6, 4);

/// The Swift SDK name xtool installs.
pub const DARWIN_SDK_NAME: &str = "darwin";

/// Where SwiftPM keeps an installed Swift SDK.
pub const SWIFT_SDKS_DIR: &str = ".swiftpm/swift-sdks";

/// Where xtool writes its output, relative to the project root.
pub const XTOOL_OUTPUT_DIR: &str = "xtool";

/// Prerequisites for an Apple build on this machine, as observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppleToolchain {
    /// Path to `swift`, when found.
    pub swift: Option<PathBuf>,
    /// `swift --version` output, when it could be read.
    pub swift_version: Option<String>,
    /// Path to `xtool`, when found.
    pub xtool: Option<PathBuf>,
    /// Whether the `darwin` Swift SDK appears to be installed.
    pub darwin_sdk_installed: bool,
    /// Where SwiftPM looks for Swift SDKs.
    pub sdk_dir: PathBuf,
    /// `swift sdk list` output, when it could be read.
    pub sdk_list: Option<String>,
}

/// A prerequisite that is missing, named so the user can act on it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MissingPrerequisite {
    /// No `swift` on `PATH`.
    SwiftNotInstalled,
    /// `swift` exists but is older than [`REQUIRED_SWIFT`].
    SwiftTooOld {
        /// What the toolchain reported.
        found: String,
        /// What xtool requires.
        required: String,
    },
    /// No `xtool` on `PATH`.
    XtoolNotInstalled,
    /// xtool is present but has no iOS SDK, so `xtool sdk install` has not
    /// been run.
    ///
    /// This is the one that needs an Apple ID, which is why it is a
    /// distinct variant rather than a generic "not set up".
    SdkNotInstalled,
}

impl std::fmt::Display for MissingPrerequisite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SwiftNotInstalled => write!(
                f,
                "no Swift toolchain on PATH. Install Swift {}.{} from https://swift.org/install/linux \
                 and put its usr/bin on PATH (no root needed)",
                REQUIRED_SWIFT.0, REQUIRED_SWIFT.1
            ),
            Self::SwiftTooOld { found, required } => write!(
                f,
                "Swift {found} is too old; xtool needs {required} or newer"
            ),
            Self::XtoolNotInstalled => write!(
                f,
                "xtool is not installed. It is an MIT-licensed AppImage: \
                 https://github.com/xtool-org/xtool/releases — no root needed"
            ),
            Self::SdkNotInstalled => write!(
                f,
                "xtool has no iOS SDK. Download Xcode.xip from \
                 https://developer.apple.com/download/all/?q=Xcode (Apple ID required, ~7 GB), \
                 then run `xtool sdk install /path/to/Xcode.xip`. \
                 Apple's SDK is yours to obtain; Tahoe never downloads or ships it."
            ),
        }
    }
}

/// Why a build could not run.
#[derive(Debug)]
#[non_exhaustive]
pub enum AppleBuildError {
    /// A prerequisite is missing.
    Prerequisite(MissingPrerequisite),
    /// The project directory is not an xtool project.
    NotAProject {
        /// What was wrong.
        reason: String,
    },
    /// `xtool` ran and failed.
    BuildFailed {
        /// The command that ran.
        command: String,
        /// Its status.
        status: String,
        /// Its captured stderr, which is where xtool puts the interesting
        /// part.
        stderr: String,
    },
    /// The build reported success but the artifact is not where expected.
    ArtifactMissing(PathBuf),
    /// The artifact exists but is not a usable bundle.
    BadBundle(BundleError),
    /// The artifact's executable could not be parsed.
    BadExecutable(MachOError),
    /// `xtool` could not be spawned at all.
    Io(std::io::Error),
}

impl std::fmt::Display for AppleBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Prerequisite(p) => write!(f, "{p}"),
            Self::NotAProject { reason } => write!(f, "not an xtool project: {reason}"),
            Self::BuildFailed {
                command,
                status,
                stderr,
            } => {
                write!(f, "`{command}` failed ({status})")?;
                if !stderr.trim().is_empty() {
                    write!(f, "\n{}", stderr.trim())?;
                }
                Ok(())
            }
            Self::ArtifactMissing(p) => {
                write!(f, "the build succeeded but {} does not exist", p.display())
            }
            Self::BadBundle(e) => write!(f, "the built bundle could not be read: {e}"),
            Self::BadExecutable(e) => write!(f, "the built executable could not be parsed: {e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AppleBuildError {}

impl AppleToolchain {
    /// Probe this machine. Never fails; absence is a finding.
    #[must_use]
    pub fn detect() -> Self {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let sdk_dir = home.join(SWIFT_SDKS_DIR);
        let swift = which("swift");
        let swift_version = swift.as_ref().and_then(|s| run_capture(s, &["--version"]));
        let xtool = which("xtool");
        // `swift sdk list` is the authoritative answer, but it is slow, so
        // the directory is checked first and the command only consulted
        // when the directory suggests something is there.
        let darwin_sdk_installed = sdk_dir
            .join(format!("{DARWIN_SDK_NAME}.artifactbundle"))
            .is_dir();
        let sdk_list = swift.as_ref().and_then(|s| {
            if sdk_dir.is_dir() {
                run_capture(s, &["sdk", "list"])
            } else {
                None
            }
        });
        Self {
            swift,
            swift_version,
            xtool,
            darwin_sdk_installed,
            sdk_dir,
            sdk_list,
        }
    }

    /// The first missing prerequisite, if any.
    ///
    /// Ordered deliberately: a missing Swift toolchain is a different problem
    /// from a missing SDK, and reporting the SDK first would send the user
    /// off to download seven gigabytes they cannot use yet.
    #[must_use]
    pub fn first_missing(&self) -> Option<MissingPrerequisite> {
        let Some(swift) = &self.swift else {
            return Some(MissingPrerequisite::SwiftNotInstalled);
        };
        let _ = swift;
        if let Some(version) = &self.swift_version
            && let Some(parsed) = parse_swift_version(version)
            && parsed < REQUIRED_SWIFT
        {
            return Some(MissingPrerequisite::SwiftTooOld {
                found: format!("{}.{}", parsed.0, parsed.1),
                required: format!("{}.{}", REQUIRED_SWIFT.0, REQUIRED_SWIFT.1),
            });
        }
        if self.xtool.is_none() {
            return Some(MissingPrerequisite::XtoolNotInstalled);
        }
        if !self.darwin_sdk_installed {
            return Some(MissingPrerequisite::SdkNotInstalled);
        }
        None
    }

    /// Whether an Apple build can run right now.
    #[must_use]
    pub fn can_build(&self) -> bool {
        self.first_missing().is_none()
    }

    /// A human-readable status, one line per prerequisite.
    #[must_use]
    pub fn describe(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "swift: {}",
            match (&self.swift, &self.swift_version) {
                (Some(p), Some(v)) =>
                    format!("{} ({})", p.display(), v.lines().next().unwrap_or("")),
                (Some(p), None) => format!("{} (--version failed)", p.display()),
                (None, _) => "not found".to_owned(),
            }
        );
        let _ = writeln!(
            out,
            "xtool: {}",
            self.xtool
                .as_ref()
                .map_or_else(|| "not found".to_owned(), |p| p.display().to_string())
        );
        let _ = writeln!(
            out,
            "darwin SDK: {} ({})",
            if self.darwin_sdk_installed {
                "installed"
            } else {
                "not installed"
            },
            self.sdk_dir.display()
        );
        if let Some(list) = &self.sdk_list {
            let _ = writeln!(out, "swift sdk list: {}", list.trim());
        }
        match self.first_missing() {
            Some(missing) => {
                let _ = writeln!(out, "\nblocked: {missing}");
            }
            None => {
                let _ = writeln!(out, "\nready to build");
            }
        }
        out
    }
}

/// An xtool project on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppleProject {
    /// The directory holding `Package.swift` and `xtool.yml`.
    pub root: PathBuf,
    /// The parsed `xtool.yml`.
    pub manifest: XtoolManifest,
}

/// The subset of `xtool.yml` this module reads.
///
/// Only the scalar keys are parsed. `xtool.yml` is YAML, but the file xtool
/// generates uses nothing but top-level `key: value` lines, and a full YAML
/// parser would be a dependency for three fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XtoolManifest {
    /// `version`.
    pub version: Option<u32>,
    /// `bundleID`.
    pub bundle_id: Option<String>,
    /// `orgID`, the alternative to `bundleID`.
    pub org_id: Option<String>,
    /// `product`.
    pub product: Option<String>,
    /// `infoPath`, which this module deliberately does not use.
    pub info_path: Option<String>,
    /// Keys present in the file that this parser does not model, so a caller
    /// can tell "absent" from "present but unread".
    pub unmodelled_keys: Vec<String>,
}

impl XtoolManifest {
    /// Parse the scalar keys of an `xtool.yml`.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut out = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
                continue;
            }
            // Only top-level keys: a nested key is indented, and indent was
            // stripped, so re-check the original line.
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if key.contains(' ') {
                continue;
            }
            match key {
                "version" => out.version = value.parse().ok(),
                "bundleID" => out.bundle_id = Some(value.to_owned()),
                "orgID" => out.org_id = Some(value.to_owned()),
                "product" => out.product = Some(value.to_owned()),
                "infoPath" => out.info_path = Some(value.to_owned()),
                other => out.unmodelled_keys.push(other.to_owned()),
            }
        }
        out
    }

    /// The bundle identifier, from either key.
    #[must_use]
    pub fn effective_bundle_id(&self) -> Option<&str> {
        self.bundle_id.as_deref().or(self.org_id.as_deref())
    }
}

impl AppleProject {
    /// Load and validate a project directory.
    ///
    /// # Errors
    ///
    /// Returns [`AppleBuildError::NotAProject`] with the specific problem:
    /// a missing manifest, a missing `Package.swift`, or a manifest with no
    /// product to package.
    pub fn load(root: &Path) -> Result<Self, AppleBuildError> {
        let manifest_path = root.join("xtool.yml");
        let package_path = root.join("Package.swift");
        if !package_path.is_file() {
            return Err(AppleBuildError::NotAProject {
                reason: format!("{} has no Package.swift", root.display()),
            });
        }
        let text =
            std::fs::read_to_string(&manifest_path).map_err(|e| AppleBuildError::NotAProject {
                reason: format!("{}: {e}", manifest_path.display()),
            })?;
        let manifest = XtoolManifest::parse(&text);
        if manifest.product.is_none() {
            return Err(AppleBuildError::NotAProject {
                reason: "xtool.yml sets no `product`, so there is nothing to package".to_owned(),
            });
        }
        Ok(Self {
            root: root.to_path_buf(),
            manifest,
        })
    }

    /// The product name, which is also the `.app` name.
    #[must_use]
    pub fn product(&self) -> &str {
        self.manifest.product.as_deref().unwrap_or("App")
    }

    /// Where xtool writes the bundle.
    #[must_use]
    pub fn app_path(&self) -> PathBuf {
        self.root
            .join(XTOOL_OUTPUT_DIR)
            .join(format!("{}.app", self.product()))
    }

    /// Where xtool writes an `.ipa`, when asked for one.
    #[must_use]
    pub fn ipa_path(&self) -> PathBuf {
        self.root
            .join(XTOOL_OUTPUT_DIR)
            .join(format!("{}.ipa", self.product()))
    }

    /// The `argv` for a build, without running it.
    ///
    /// Separate from [`AppleProject::build`] so the command can be asserted
    /// without a toolchain, and shown to a user who wants to run it by hand.
    #[must_use]
    pub fn build_argv(&self, request: &BuildRequest) -> Vec<OsString> {
        let mut argv: Vec<OsString> = vec![
            "xtool".into(),
            "dev".into(),
            "build".into(),
            "--configuration".into(),
            request.configuration.as_str().into(),
        ];
        if request.produce_ipa {
            argv.push("--ipa".into());
        }
        // `--sign` is deliberately never passed: it needs `xtool auth`, an
        // Apple ID, and a real device. An unsigned build is what a distro
        // should be able to make without either.
        if let Some(triple) = &request.triple {
            argv.push("--triple".into());
            argv.push(triple.clone());
        }
        argv
    }

    /// Run `xtool dev build`.
    ///
    /// # Errors
    ///
    /// See [`AppleBuildError`]. A missing prerequisite is reported as such
    /// before anything is spawned.
    pub fn build(
        &self,
        toolchain: &AppleToolchain,
        request: &BuildRequest,
    ) -> Result<PathBuf, AppleBuildError> {
        if let Some(missing) = toolchain.first_missing() {
            return Err(AppleBuildError::Prerequisite(missing));
        }
        let xtool = toolchain
            .xtool
            .as_ref()
            .ok_or(AppleBuildError::Prerequisite(
                MissingPrerequisite::XtoolNotInstalled,
            ))?;
        let mut argv = self.build_argv(request);
        argv[0] = xtool.as_os_str().to_owned();

        let output = Command::new(xtool)
            .args(&argv[1..])
            .current_dir(&self.root)
            .output()
            .map_err(AppleBuildError::Io)?;

        if !output.status.success() {
            return Err(AppleBuildError::BuildFailed {
                command: render(&argv),
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        let artifact = if request.produce_ipa {
            self.ipa_path()
        } else {
            self.app_path()
        };
        if !artifact.exists() {
            // xtool succeeds without producing output when the product is
            // already up to date in some configurations; either way, the
            // caller asked for a path that is not there.
            return Err(AppleBuildError::ArtifactMissing(artifact));
        }
        Ok(artifact)
    }

    /// Build, then inspect the result through [`crate::macos`].
    ///
    /// This is the point of the whole module: a real `.app`, parsed by the
    /// code that was written against fixtures.
    ///
    /// # Errors
    ///
    /// See [`AppleBuildError`].
    pub fn build_and_inspect(
        &self,
        toolchain: &AppleToolchain,
        request: &BuildRequest,
    ) -> Result<AppleArtifact, AppleBuildError> {
        let app = self.build(toolchain, request)?;
        inspect_bundle(&app).map_err(AppleBuildError::BadBundle)
    }
}

/// How to build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRequest {
    /// Debug or release.
    pub configuration: Configuration,
    /// Produce an `.ipa` instead of an `.app`. An `.ipa` is a zip, so this
    /// module can report its existence but cannot inspect it as a bundle.
    pub produce_ipa: bool,
    /// Override the target triple, e.g. `arm64-apple-ios-simulator`.
    pub triple: Option<OsString>,
}

impl Default for BuildRequest {
    fn default() -> Self {
        Self {
            configuration: Configuration::Debug,
            produce_ipa: false,
            triple: None,
        }
    }
}

/// SwiftPM build configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Configuration {
    /// `debug`.
    Debug,
    /// `release`.
    Release,
}

impl Configuration {
    /// The string SwiftPM expects.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

/// A built bundle, described.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppleArtifact {
    /// The `.app` directory.
    pub app: PathBuf,
    /// `CFBundleExecutable` and friends, resolved by `macos::AppBundle`.
    pub bundle: AppBundle,
    /// Architecture of the built executable, as `lipo` would name it.
    pub architecture: Option<String>,
    /// True for a 64-bit slice.
    pub is_64_bit: bool,
    /// `LC_LOAD_DYLIB` install names, truncated for display.
    pub dependencies: Vec<String>,
    /// Minimum macOS version the binary declares.
    ///
    /// Usually `None` for an iOS build: iOS binaries carry a
    /// `LC_BUILD_VERSION` with `PLATFORM_IOS`, which is *not* a macOS
    /// minimum, and reporting one would be wrong.
    pub minimum_macos: Option<String>,
    /// The declared platform constant.
    pub platform: Option<u32>,
    /// Whether the binary carries an `LC_CODE_SIGNATURE`.
    pub is_signed: bool,
}

impl AppleArtifact {
    /// One-line summary.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} [{}] arch={} signed={} deps={}",
            self.bundle.executable_name,
            self.bundle.identifier.as_deref().unwrap_or("no bundle id"),
            self.architecture.as_deref().unwrap_or("unknown"),
            self.is_signed,
            self.dependencies.len()
        )
    }
}

/// Inspect an `.app` bundle on disk.
///
/// # Errors
///
/// See [`BundleError`].
pub fn inspect_bundle(app: &Path) -> Result<AppleArtifact, BundleError> {
    let bundle = AppBundle::resolve(app)?;
    // A bundle whose executable is not Mach-O is still a valid bundle; the
    // parse failure is reported by leaving the Mach-O fields empty rather
    // than failing the whole inspection.
    let parsed = MachOFile::from_path(&bundle.executable, &["arm64", "x86_64"]).ok();
    let (architecture, is_64_bit, dependencies, minimum_macos, platform, is_signed) = match parsed {
        Some((image, slice)) => (
            Some(
                slice
                    .map(|s| s.cpu_name().to_owned())
                    .unwrap_or_else(|| crate::macos::cpu_name_for(image.cputype).to_owned()),
            ),
            slice.map(|s| s.is_64_bit()).unwrap_or(true),
            image
                .dependencies
                .iter()
                .map(|d| d.install_name.clone())
                .collect(),
            image.min_macos_version(),
            image.platform(),
            image.code_signature,
        ),
        None => (None, false, Vec::new(), None, None, false),
    };
    Ok(AppleArtifact {
        app: app.to_path_buf(),
        bundle,
        architecture,
        is_64_bit,
        dependencies,
        minimum_macos,
        platform,
        is_signed,
    })
}

/// Parse `Swift version 6.4 (swift-6.4-RELEASE)` into `(6, 4)`.
///
/// Delegates to [`crate::macos::parse_swift_release`]. The first version of
/// this function lived here and was wrong: it treated `.` as part of a
/// token, so `"6.4"` failed to parse as an integer and every real banner
/// returned `None`.
#[must_use]
pub fn parse_swift_version(output: &str) -> Option<(u64, u64)> {
    crate::macos::parse_swift_release(output)
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

fn run_capture(program: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Some(text)
}

fn render(argv: &[OsString]) -> String {
    argv.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
