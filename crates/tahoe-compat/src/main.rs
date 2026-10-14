// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-compat` CLI.
//!
//! Diagnostics first: the point of this binary is to let a human find out
//! what this machine can actually run, and why, without reading source.
//!
//! ```text
//! tahoe-compat runtime                       what Wine/macOS support exists here
//! tahoe-compat wine apps [--prefix P]        Windows apps discoverable in a prefix
//! tahoe-compat wine prefix <path>            report one prefix's state
//! tahoe-compat mac bundle <Foo.app>          resolve a bundle to its executable
//! tahoe-compat mac macho <file>              load commands and dependencies
//! tahoe-compat binfmt rules                  rules Tahoe would register
//! tahoe-compat binfmt status                 the dispatcher as this process sees it
//! tahoe-compat binfmt self-test              register + dispatch, in a throwaway namespace
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tahoe_compat::{
    binfmt::{self, Dispatcher},
    macos::{AppBundle, MachOFile},
    wine::{NTSYNC_DEVICE, Prefix, WineRuntime},
    xtool::{
        AppleBuildError, AppleProject, AppleToolchain, BuildRequest, Configuration, inspect_bundle,
    },
};

const USAGE: &str = "\
tahoe-compat — compatibility services for Tahoe

USAGE:
    tahoe-compat runtime
    tahoe-compat wine apps [--prefix <path>] [--all]
    tahoe-compat wine prefix <path>
    tahoe-compat mac bundle <Foo.app>
    tahoe-compat mac macho <file>
    tahoe-compat binfmt rules [--interpreter-dir <dir>]
    tahoe-compat binfmt status
    tahoe-compat binfmt self-test
    tahoe-compat apple toolchain
    tahoe-compat apple project <dir>
    tahoe-compat apple build <dir> [--release] [--ipa] [--dry-run]
    tahoe-compat apple inspect <Foo.app>

NOTES:
    Nothing here executes a Windows or macOS program. `binfmt self-test`
    registers a rule inside a throwaway user namespace and dispatches one
    file through the kernel; it needs no root and changes nothing outside
    that namespace.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("runtime") => cmd_runtime(),
        Some("wine") => cmd_wine(&args[1..]),
        Some("mac") => cmd_mac(&args[1..]),
        Some("binfmt") => cmd_binfmt(&args[1..]),
        Some("apple") => cmd_apple(&args[1..]),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("tahoe-compat: {message}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_runtime() -> Result<(), String> {
    let wine = WineRuntime::detect();
    println!("== Wine ==");
    print!("{}", wine.describe());
    println!("  loader: {:?}", wine.loader);
    println!("  version: {:?}", wine.version);
    println!("  ntsync: {} ({})", wine.ntsync_available, NTSYNC_DEVICE);
    println!(
        "  kernel supports ntsync: {}",
        wine.kernel_supports_ntsync()
    );

    println!("\n== binfmt_misc ==");
    let dispatcher = Dispatcher::inspect();
    println!("  mounted: {}", dispatcher.mounted);
    println!("  enabled: {:?}", dispatcher.enabled);
    println!("  entries: {:?}", dispatcher.entries);

    println!("\n== macOS ==");
    let darling = which("darling");
    println!("  darling: {darling:?}");
    println!("  dmgs can be inspected; running Mach-O needs Darling, which is not bundled");
    Ok(())
}

fn cmd_wine(rest: &[String]) -> Result<(), String> {
    match rest.first().map(String::as_str) {
        Some("apps") => {
            let mut prefix_path: Option<PathBuf> = None;
            let mut all = false;
            let mut it = rest[1..].iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--prefix" => {
                        prefix_path =
                            Some(PathBuf::from(it.next().ok_or("--prefix needs a value")?))
                    }
                    "--all" => all = true,
                    other => return Err(format!("unknown flag: {other}")),
                }
            }
            let prefixes = if all {
                discover_prefixes()
            } else {
                vec![prefix_path.map_or_else(Prefix::default_location, Prefix::new)]
            };
            let mut total = 0;
            for prefix in prefixes {
                println!(
                    "prefix {} ({}):",
                    prefix.path.display(),
                    if prefix.is_initialised() {
                        "initialised"
                    } else {
                        "not initialised"
                    }
                );
                let apps = prefix.applications();
                total += apps.len();
                for app in &apps {
                    let program = match &app.launch {
                        tahoe_compat::Launch::Windows { program, .. } => {
                            program.display().to_string()
                        }
                        other => format!("{other:?}"),
                    };
                    println!("  {} [{}] -> {program}", app.name, app.origin.label());
                    for caveat in &app.caveats {
                        println!("      note: {caveat}");
                    }
                }
                if apps.is_empty() {
                    println!("  (none)");
                }
            }
            println!("\n{total} application(s)");
            Ok(())
        }
        Some("prefix") => {
            let path = rest.get(1).ok_or("prefix needs a path")?;
            let prefix = Prefix::new(path);
            println!("path: {}", prefix.path.display());
            println!("exists: {}", prefix.exists());
            println!("initialised: {}", prefix.is_initialised());
            println!("architecture: {:?}", prefix.architecture());
            println!("drive_c: {}", prefix.drive_c().display());
            println!("start menu dirs:");
            for dir in prefix.start_menu_dirs() {
                println!("  {}", dir.display());
            }
            println!("launch env:");
            for (k, v) in prefix.launch_env() {
                println!("  {}={}", k.to_string_lossy(), v.to_string_lossy());
            }
            Ok(())
        }
        _ => Err(format!("unknown `wine` subcommand\n\n{USAGE}")),
    }
}

fn cmd_mac(rest: &[String]) -> Result<(), String> {
    match rest.first().map(String::as_str) {
        Some("bundle") => {
            let path = PathBuf::from(rest.get(1).ok_or("bundle needs a path")?);
            let bundle = AppBundle::resolve(&path).map_err(|e| e.to_string())?;
            println!("root: {}", bundle.root.display());
            println!("executable: {}", bundle.executable.display());
            println!("executable_name: {}", bundle.executable_name);
            println!("identifier: {:?}", bundle.identifier);
            println!(
                "minimum_system_version: {:?}",
                bundle.minimum_system_version
            );
            println!("icon_file: {:?}", bundle.icon_file);
            println!("is_agent: {}", bundle.is_agent);
            println!("package_type: {:?}", bundle.package_type);
            println!("architectures: {:?}", bundle.architectures);
            Ok(())
        }
        Some("macho") => {
            let path = PathBuf::from(rest.get(1).ok_or("macho needs a path")?);
            let (image, slice) =
                MachOFile::from_path(&path, &["x86_64", "arm64"]).map_err(|e| e.to_string())?;
            if let Some(slice) = slice {
                println!(
                    "universal binary, selected slice: {} ({})",
                    slice.cpu_name(),
                    slice.is_64_bit()
                );
            }
            println!(
                "cputype: {} ({:#x})",
                tahoe_compat::macos::cpu_name_for(image.cputype),
                image.cputype
            );
            println!(
                "filetype: {:#x} executable={}",
                image.filetype,
                image.is_executable()
            );
            println!("pie: {}", image.pie);
            println!("uuid: {:?}", image.uuid);
            println!("dylinker: {:?}", image.dylinker);
            println!(
                "min macos: {:?} (platform {:?})",
                image.min_macos_version(),
                image.platform()
            );
            println!("code signature: {}", image.code_signature);
            println!("entry offset: {:?}", image.entry_offset);
            println!("rpaths: {:?}", image.rpaths);
            println!("dependencies ({}):", image.dependencies.len());
            for dep in &image.dependencies {
                let tag = if dep.weak {
                    "weak"
                } else if dep.reexported {
                    "reexport"
                } else {
                    "required"
                };
                println!("  [{tag}] {}", dep.install_name);
            }
            let missing = tahoe_compat::macos::unresolved_dependencies(
                &image,
                path.parent().unwrap_or(Path::new("/")),
                path.parent().unwrap_or(Path::new("/")),
            );
            println!("unresolved here ({}):", missing.len());
            for m in &missing {
                println!("  {m}");
            }
            println!(
                "\nNOTE: resolution is checked against this Linux filesystem. On a macOS\n\
                 system those paths live in the dyld shared cache, so `unresolved`\n\
                 here does not mean the app is broken — it means a Darwin runtime\n\
                 must supply them."
            );
            Ok(())
        }
        _ => Err(format!("unknown `mac` subcommand\n\n{USAGE}")),
    }
}

fn cmd_binfmt(rest: &[String]) -> Result<(), String> {
    match rest.first().map(String::as_str) {
        Some("rules") => {
            let mut dir = PathBuf::from("/usr/lib/tahoe/compat");
            let mut it = rest[1..].iter();
            while let Some(arg) = it.next() {
                if arg == "--interpreter-dir" {
                    dir = PathBuf::from(it.next().ok_or("--interpreter-dir needs a value")?);
                } else {
                    return Err(format!("unknown flag: {arg}"));
                }
            }
            for rule in binfmt::shipped_rules(&dir) {
                println!("# {}", rule.name);
                println!("{}", rule.to_register_string());
            }
            Ok(())
        }
        Some("status") => {
            let d = Dispatcher::inspect();
            println!("mounted: {}", d.mounted);
            println!("enabled: {:?}", d.enabled);
            println!("entries: {:?}", d.entries);
            for name in &d.entries {
                if let Ok(entry) = Dispatcher::read_entry(name) {
                    println!(
                        "  {name}: interpreter={} flags={:?}",
                        entry.reported_interpreter, entry.reported_flags
                    );
                }
            }
            Ok(())
        }
        Some("self-test") => {
            // End-to-end: register a rule in a scratch namespace and let the
            // kernel dispatch a real file. No root, no host change.
            let dir = std::env::temp_dir().join("tahoe-compat-selftest");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let target = dir.join("payload.bin");
            let mut bytes = vec![0xcf, 0xfa, 0xed, 0xfe];
            bytes.resize(64, 0);
            std::fs::write(&target, &bytes).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| e.to_string())?;
            }

            let rule = binfmt::Rule::new("tahoe-selftest", &[0xcf, 0xfa, 0xed, 0xfe], "/bin/echo");
            println!("registering in a throwaway user namespace:");
            println!("  {}", rule.to_register_string());
            let out = binfmt::register_isolated(&[rule], &[target.display().to_string()])
                .map_err(|e| e.to_string())?;
            println!("exit status: {}", out.status);
            println!("stdout: {}", String::from_utf8_lossy(&out.stdout).trim());
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.trim().is_empty() {
                println!("stderr: {}", stderr.trim());
            }
            let _ = std::fs::remove_file(&target);
            if out.status.success() {
                println!("\nOK: the kernel dispatched the file to our interpreter.");
                Ok(())
            } else {
                Err("the namespace run failed".to_owned())
            }
        }
        _ => Err(format!("unknown `binfmt` subcommand\n\n{USAGE}")),
    }
}

fn cmd_apple(rest: &[String]) -> Result<(), String> {
    match rest.first().map(String::as_str) {
        Some("toolchain") => {
            let toolchain = AppleToolchain::detect();
            print!("{}", toolchain.describe());
            Ok(())
        }
        Some("project") => {
            let dir = PathBuf::from(rest.get(1).ok_or("project needs a directory")?);
            let project = AppleProject::load(&dir).map_err(|e| e.to_string())?;
            println!("root: {}", project.root.display());
            println!("product: {}", project.product());
            println!("bundle id: {:?}", project.manifest.effective_bundle_id());
            println!("manifest version: {:?}", project.manifest.version);
            if !project.manifest.unmodelled_keys.is_empty() {
                println!(
                    "keys this parser does not model: {:?}",
                    project.manifest.unmodelled_keys
                );
            }
            println!("app output: {}", project.app_path().display());
            println!("ipa output: {}", project.ipa_path().display());
            Ok(())
        }
        Some("build") => {
            let mut dir: Option<PathBuf> = None;
            let mut request = BuildRequest::default();
            let mut dry_run = false;
            let mut it = rest[1..].iter();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--release" => request.configuration = Configuration::Release,
                    "--ipa" => request.produce_ipa = true,
                    "--dry-run" => dry_run = true,
                    "--triple" => {
                        request.triple = Some(it.next().ok_or("--triple needs a value")?.into())
                    }
                    other if other.starts_with('-') => {
                        return Err(format!("unknown flag: {other}"));
                    }
                    other => dir = Some(PathBuf::from(other)),
                }
            }
            let dir = dir.ok_or("build needs a project directory")?;
            let project = AppleProject::load(&dir).map_err(|e| e.to_string())?;
            let argv: Vec<String> = project
                .build_argv(&request)
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            println!("would run, in {}:", project.root.display());
            println!("  {}", argv.join(" "));
            if dry_run {
                println!(
                    "
--dry-run: nothing was executed."
                );
                return Ok(());
            }
            let toolchain = AppleToolchain::detect();
            if let Some(missing) = toolchain.first_missing() {
                // Reported as a normal outcome, not an error: on a machine
                // without Swift this is expected, and the message says what
                // to install.
                println!(
                    "
cannot build: {missing}"
                );
                return Ok(());
            }
            match project.build_and_inspect(&toolchain, &request) {
                Ok(artifact) => {
                    println!(
                        "
built: {}",
                        artifact.app.display()
                    );
                    println!("  {}", artifact.summary());
                    if let Some(arch) = &artifact.architecture {
                        println!("  architecture: {arch}");
                    }
                    if let Some(min) = &artifact.bundle.minimum_system_version {
                        println!("  minimum OS: {min}");
                    }
                    Ok(())
                }
                Err(AppleBuildError::Prerequisite(missing)) => {
                    println!(
                        "
cannot build: {missing}"
                    );
                    Ok(())
                }
                Err(e) => Err(e.to_string()),
            }
        }
        Some("inspect") => {
            let app = PathBuf::from(rest.get(1).ok_or("inspect needs an .app path")?);
            let artifact = inspect_bundle(&app).map_err(|e| e.to_string())?;
            println!("app: {}", artifact.app.display());
            println!("layout: {:?}", artifact.bundle.layout);
            println!("executable: {}", artifact.bundle.executable.display());
            println!("bundle id: {:?}", artifact.bundle.identifier);
            println!("minimum OS: {:?}", artifact.bundle.minimum_system_version);
            println!("package type: {:?}", artifact.bundle.package_type);
            println!("is agent: {}", artifact.bundle.is_agent);
            println!(
                "architectures in bundle: {:?}",
                artifact.bundle.architectures
            );
            println!("selected architecture: {:?}", artifact.architecture);
            println!("is 64-bit: {}", artifact.is_64_bit);
            println!("signed: {}", artifact.is_signed);
            println!("platform: {:?}", artifact.platform);
            println!("minimum macOS: {:?}", artifact.minimum_macos);
            println!("dependencies ({}):", artifact.dependencies.len());
            for dep in &artifact.dependencies {
                println!("  {dep}");
            }
            println!("\nNOTE: this is an inspection. An iOS bundle links UIKit, which no");
            println!("Linux runtime implements, so it cannot be executed here.");
            Ok(())
        }
        _ => Err(format!("unknown `apple` subcommand\n\n{USAGE}")),
    }
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

/// Prefixes Tahoe manages, plus the conventional one.
fn discover_prefixes() -> Vec<Prefix> {
    let mut found = Vec::new();
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    let managed = home.join(".local/share/tahoe/wine");
    if let Ok(rd) = std::fs::read_dir(&managed) {
        for entry in rd.flatten() {
            if entry.path().is_dir() {
                found.push(Prefix::new(entry.path()));
            }
        }
    }
    let default = Prefix::default_location();
    if default.exists() && !found.iter().any(|p| p.path == default.path) {
        found.push(default);
    }
    found
}
