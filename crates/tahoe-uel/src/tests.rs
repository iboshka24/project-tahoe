// SPDX-License-Identifier: MIT OR Apache-2.0
//! Decision-table tests for [`crate::decide`]. Every branch that matters to
//! a user double-clicking a file is represented here.

use crate::{Confidence, ContainerKind, HostFacts, Policy, RejectionReason, Strategy, decide};
use std::path::{Path, PathBuf};
use tahoe_core::{Arch, ObjectFormat, OsAbi};
use tahoe_format::{FormatDetail, ProbeReport};

fn report(format: ObjectFormat, arch: Arch) -> ProbeReport {
    ProbeReport {
        format,
        arch,
        bits: None,
        endian: None,
        os_abi: OsAbi::Unspecified,
        detail: FormatDetail::None,
    }
}

fn host() -> HostFacts {
    HostFacts {
        host_arch: Arch::X86_64,
        wine_available: true,
        darling_available: true,
        emulator_available: false,
        flatpak_available: true,
        // No Proton, so the PE tests exercise the Wine path. The Proton
        // path has its own tests below.
        proton_build: None,
        umu_available: None,
    }
}

fn src() -> PathBuf {
    Path::new("setup.exe").to_path_buf()
}

#[test]
fn native_elf_is_verified() {
    let plan = decide(
        &report(ObjectFormat::Elf, Arch::X86_64),
        &host(),
        &Policy::default(),
        &src(),
    );
    assert_eq!(plan.strategy, Strategy::Native);
    assert_eq!(plan.confidence, Confidence::Verified);
}

#[test]
fn unknown_arch_elf_runs_native() {
    // Truncated headers still identify ELF; arch unknown means "try native".
    let plan = decide(
        &report(ObjectFormat::Elf, Arch::Unknown),
        &host(),
        &Policy::default(),
        &src(),
    );
    assert_eq!(plan.strategy, Strategy::Native);
}

#[test]
fn foreign_elf_without_emulator_is_refused() {
    let plan = decide(
        &report(ObjectFormat::Elf, Arch::Aarch64),
        &host(),
        &Policy::default(),
        &src(),
    );
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::ArchMismatchNoEmulator {
                needed: Arch::Aarch64
            }
        }
    );
    assert_eq!(plan.confidence, Confidence::Refused);
}

#[test]
fn foreign_elf_with_emulator_but_cautious_policy() {
    let h = HostFacts {
        emulator_available: true,
        ..host()
    };
    let plan = decide(
        &report(ObjectFormat::Elf, Arch::Aarch64),
        &h,
        &Policy::default(),
        &src(),
    );
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::SpeculativeDisabled
        }
    );
}

#[test]
fn foreign_elf_with_emulator_and_opt_in() {
    let h = HostFacts {
        emulator_available: true,
        ..host()
    };
    let p = Policy {
        allow_speculative: true,
        ..Policy::default()
    };
    let plan = decide(&report(ObjectFormat::Elf, Arch::Aarch64), &h, &p, &src());
    assert!(matches!(plan.strategy, Strategy::Emulator { .. }));
    assert_eq!(plan.confidence, Confidence::Speculative);
}

#[test]
fn pe_via_wine_with_caveat() {
    let plan = decide(
        &report(ObjectFormat::Pe, Arch::X86_64),
        &host(),
        &Policy::default(),
        &src(),
    );
    assert!(matches!(plan.strategy, Strategy::Wine { .. }));
    assert!(matches!(
        plan.confidence,
        Confidence::Layer { layer: "wine", .. }
    ));
    assert!(plan.summary.contains("wine"));
}

#[test]
fn pe32_on_x86_64_is_still_wine() {
    // New WoW64 handles 32-bit PE in-process: no emulator needed.
    let plan = decide(
        &report(ObjectFormat::Pe, Arch::X86),
        &host(),
        &Policy::default(),
        &src(),
    );
    assert!(matches!(plan.strategy, Strategy::Wine { .. }));
}

#[test]
fn pe_without_wine_names_the_layer() {
    let h = HostFacts {
        wine_available: false,
        ..host()
    };
    let plan = decide(
        &report(ObjectFormat::Pe, Arch::X86_64),
        &h,
        &Policy::default(),
        &src(),
    );
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::LayerMissing { layer: "wine" }
        }
    );
}

#[test]
fn pe_cross_os_disabled() {
    let p = Policy {
        allow_cross_os: false,
        ..Policy::default()
    };
    let plan = decide(&report(ObjectFormat::Pe, Arch::X86_64), &host(), &p, &src());
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::CrossOsDisabled
        }
    );
}

#[test]
fn pe_arm64_on_x86_64_needs_emulator() {
    let h = HostFacts {
        emulator_available: true,
        ..host()
    };
    let p = Policy {
        allow_speculative: true,
        ..Policy::default()
    };
    let plan = decide(&report(ObjectFormat::Pe, Arch::Aarch64), &h, &p, &src());
    assert!(matches!(plan.strategy, Strategy::Emulator { .. }));
    assert_eq!(plan.confidence, Confidence::Speculative);
}

#[test]
fn macho_via_darling_with_caveat() {
    let r = report(ObjectFormat::MachO, Arch::X86_64);
    let plan = decide(&r, &host(), &Policy::default(), &src());
    assert!(matches!(plan.strategy, Strategy::Darling { .. }));
    assert!(matches!(
        plan.confidence,
        Confidence::Layer {
            layer: "darling",
            ..
        }
    ));
}

#[test]
fn macho_without_darling_names_the_layer() {
    let h = HostFacts {
        darling_available: false,
        ..host()
    };
    let plan = decide(
        &report(ObjectFormat::MachO, Arch::X86_64),
        &h,
        &Policy::default(),
        &src(),
    );
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::LayerMissing { layer: "darling" }
        }
    );
}

#[test]
fn macho_cross_os_disabled() {
    let p = Policy {
        allow_cross_os: false,
        ..Policy::default()
    };
    let plan = decide(
        &report(ObjectFormat::MachO, Arch::X86_64),
        &host(),
        &p,
        &src(),
    );
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::CrossOsDisabled
        }
    );
}

#[test]
fn macho_arch_mismatch_refused() {
    let r = report(ObjectFormat::MachO, Arch::Aarch64);
    let plan = decide(&r, &host(), &Policy::default(), &src());
    assert!(matches!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::ArchMismatchNoEmulator { .. }
        }
    ));
}

#[test]
fn dmg_is_inspected_never_executed() {
    let r = ProbeReport {
        format: ObjectFormat::Dmg,
        detail: FormatDetail::Dmg {
            confident: true,
            trailer: None,
            compression: vec![],
        },
        ..report(ObjectFormat::Dmg, Arch::Unknown)
    };
    let plan = decide(&r, &host(), &Policy::default(), &src());
    match plan.strategy {
        Strategy::Inspect { kind, hint } => {
            assert_eq!(kind, ContainerKind::Dmg);
            assert!(
                hint.contains("mount"),
                "the hint must say what to do instead: {hint}"
            );
        }
        other => panic!("a disk image must be Inspect, not {other:?}"),
    }
    // Verified, not refused: we know exactly what this is.
    assert_eq!(plan.confidence, Confidence::Verified);
    assert!(plan.summary.contains("never execute") || plan.summary.contains("read-only"));
}

#[test]
fn a_dmg_hint_reports_split_segments_and_compression() {
    let trailer = tahoe_format::DmgTrailer {
        version: 4,
        header_size: 512,
        flags: 0,
        data_fork_offset: 0,
        data_fork_length: 0,
        resource_fork_offset: 0,
        resource_fork_length: 0,
        segment_number: 2,
        segment_count: 3,
        xml_offset: 0,
        xml_length: 0,
        blob_offset: 0,
        blob_length: 0,
        image_variant: 0,
        sector_count: 2 * 1024 * 1024,
    };
    let r = ProbeReport {
        format: ObjectFormat::Dmg,
        detail: FormatDetail::Dmg {
            confident: true,
            trailer: Some(Box::new(trailer)),
            compression: vec![
                tahoe_format::DmgCompression::Zlib,
                tahoe_format::DmgCompression::Lzfse,
            ],
        },
        ..report(ObjectFormat::Dmg, Arch::Unknown)
    };
    let plan = decide(&r, &host(), &Policy::default(), &src());
    match plan.strategy {
        Strategy::Inspect { hint, .. } => {
            assert!(hint.contains("segment 2 of 3"), "{hint}");
            assert!(hint.contains("compression: zlib, lzfse"), "{hint}");
            assert!(
                hint.contains("LZFSE"),
                "the LZFSE caveat must be surfaced: {hint}"
            );
        }
        other => panic!("expected Inspect, got {other:?}"),
    }
}

#[test]
fn xar_and_plist_are_inspected() {
    for (format, kind) in [
        (ObjectFormat::Xar, ContainerKind::Xar),
        (ObjectFormat::Plist, ContainerKind::Plist),
    ] {
        let plan = decide(
            &report(format.clone(), Arch::Unknown),
            &host(),
            &Policy::default(),
            &src(),
        );
        match plan.strategy {
            Strategy::Inspect { kind: k, .. } => assert_eq!(k, kind),
            other => panic!("{format} must be Inspect, got {other:?}"),
        }
    }
}

#[test]
fn an_msi_runs_through_msiexec_when_wine_is_present() {
    let r = report(ObjectFormat::Ole2, Arch::Unknown);
    let plan = decide(&r, &host(), &Policy::default(), &src());
    match plan.strategy {
        Strategy::WineInstaller { package, .. } => assert_eq!(package, src()),
        other => panic!("expected WineInstaller, got {other:?}"),
    }
    assert!(matches!(
        plan.confidence,
        Confidence::Layer { layer: "wine", .. }
    ));
}

#[test]
fn an_msi_without_wine_is_refused_by_name() {
    let h = HostFacts {
        wine_available: false,
        ..host()
    };
    let plan = decide(
        &report(ObjectFormat::Ole2, Arch::Unknown),
        &h,
        &Policy::default(),
        &src(),
    );
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::LayerMissing { layer: "wine" }
        }
    );
}

#[test]
fn a_resolved_app_bundle_runs_its_inner_executable() {
    let r = ProbeReport {
        format: ObjectFormat::AppBundle,
        detail: FormatDetail::AppBundle {
            executable: Some("/Applications/Widget.app/Contents/MacOS/Widget".to_owned()),
            bundle_id: Some("com.example.widget".to_owned()),
            minimum_system_version: Some("11.0".to_owned()),
        },
        ..report(ObjectFormat::AppBundle, Arch::X86_64)
    };
    let plan = decide(&r, &host(), &Policy::default(), &src());
    match plan.strategy {
        Strategy::MacBundle { executable, .. } => {
            assert_eq!(
                executable,
                PathBuf::from("/Applications/Widget.app/Contents/MacOS/Widget"),
                "the inner executable is what gets run, not the bundle directory"
            );
        }
        other => panic!("expected MacBundle, got {other:?}"),
    }
    assert!(
        plan.summary.contains("minimum macOS 11.0"),
        "{}",
        plan.summary
    );
}

#[test]
fn an_app_bundle_without_an_executable_key_is_refused() {
    let r = ProbeReport {
        format: ObjectFormat::AppBundle,
        detail: FormatDetail::AppBundle {
            executable: None,
            bundle_id: None,
            minimum_system_version: None,
        },
        ..report(ObjectFormat::AppBundle, Arch::Unknown)
    };
    let plan = decide(&r, &host(), &Policy::default(), &src());
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::UnknownFormat
        }
    );
}

#[test]
fn an_app_bundle_without_darling_is_refused_by_name() {
    let h = HostFacts {
        darling_available: false,
        ..host()
    };
    let r = ProbeReport {
        format: ObjectFormat::AppBundle,
        detail: FormatDetail::AppBundle {
            executable: Some("/A/Contents/MacOS/A".to_owned()),
            bundle_id: None,
            minimum_system_version: None,
        },
        ..report(ObjectFormat::AppBundle, Arch::X86_64)
    };
    let plan = decide(&r, &h, &Policy::default(), &src());
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::LayerMissing { layer: "darling" }
        }
    );
}

#[test]
fn an_arm64_only_bundle_is_refused_because_darling_is_x86_64_only() {
    let r = ProbeReport {
        format: ObjectFormat::AppBundle,
        detail: FormatDetail::AppBundle {
            executable: Some("/A.app/Contents/MacOS/A".to_owned()),
            bundle_id: None,
            minimum_system_version: None,
        },
        ..report(ObjectFormat::AppBundle, Arch::Aarch64)
    };
    let plan = decide(&r, &host(), &Policy::default(), &src());
    match plan.strategy {
        Strategy::Rejected {
            reason: RejectionReason::ArchMismatchNoEmulator { needed },
        } => {
            assert_eq!(needed, Arch::Aarch64);
        }
        other => panic!("expected an arch mismatch refusal, got {other:?}"),
    }
    assert!(
        plan.summary.contains("darling only supports"),
        "{}",
        plan.summary
    );
}

#[test]
fn ambiguous_is_never_guessed() {
    let r = report(
        ObjectFormat::Ambiguous {
            candidates: &["java-class", "fat-macho"],
        },
        Arch::Unknown,
    );
    let plan = decide(&r, &host(), &Policy::default(), &src());
    assert!(matches!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::AmbiguousFormat { .. }
        }
    ));
}

#[test]
fn unknown_format_refused() {
    let plan = decide(
        &report(ObjectFormat::Unknown, Arch::Unknown),
        &host(),
        &Policy::default(),
        &src(),
    );
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::UnknownFormat
        }
    );
}

#[test]
fn java_class_refused() {
    let plan = decide(
        &report(ObjectFormat::JavaClass, Arch::Unknown),
        &host(),
        &Policy::default(),
        &src(),
    );
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::UnknownFormat
        }
    );
}

#[test]
fn script_runs_via_interpreter() {
    let r = ProbeReport {
        format: ObjectFormat::Script,
        detail: FormatDetail::Script {
            interpreter: "/bin/sh".to_owned(),
        },
        ..report(ObjectFormat::Script, Arch::Unknown)
    };
    let plan = decide(&r, &host(), &Policy::default(), &src());
    assert_eq!(
        plan.strategy,
        Strategy::Script {
            interpreter: PathBuf::from("/bin/sh"),
            interpreter_args: Vec::new()
        }
    );
    assert_eq!(plan.confidence, Confidence::Plausible);
}

#[test]
fn script_without_interpreter_refused() {
    let r = ProbeReport {
        format: ObjectFormat::Script,
        detail: FormatDetail::Script {
            interpreter: String::new(),
        },
        ..report(ObjectFormat::Script, Arch::Unknown)
    };
    let plan = decide(&r, &host(), &Policy::default(), &src());
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::UnknownFormat
        }
    );
}

#[test]
fn appimage_default_policy_refuses() {
    let plan = decide(
        &report(ObjectFormat::AppImage, Arch::X86_64),
        &host(),
        &Policy::default(),
        &src(),
    );
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::SpeculativeDisabled
        }
    );
    assert!(plan.summary.contains("sandbox"));
}

#[test]
fn appimage_opt_in_runs_with_warning() {
    let p = Policy {
        allow_speculative: true,
        ..Policy::default()
    };
    let plan = decide(
        &report(ObjectFormat::AppImage, Arch::X86_64),
        &host(),
        &p,
        &src(),
    );
    assert_eq!(plan.strategy, Strategy::Native);
    assert_eq!(plan.confidence, Confidence::Speculative);
    assert!(plan.summary.contains("unsandboxed"));
}

/// Compile a real PE importing the given DLLs, or `None` when clang or
/// lld-link is missing.
///
/// The import table is what the Wine-versus-Proton decision reads, so the
/// decision tests need a real one. A hand-built header would test the
/// heuristic against my understanding of the format rather than against a
/// linker's output.
fn pe_fixture(imports: &[&str]) -> Option<PathBuf> {
    use std::process::Command;
    let dir = std::env::temp_dir().join("tahoe-uel-pe");
    std::fs::create_dir_all(&dir).ok()?;
    let mut hash = 0u64;
    for name in imports {
        for byte in name.bytes() {
            hash = hash
                .wrapping_mul(1099511628211)
                .wrapping_add(u64::from(byte));
        }
    }
    let tag = format!("{}-{}", std::process::id(), hash);
    let ok = |c: &mut Command| c.status().ok().is_some_and(|s| s.success());

    let decls: String = imports
        .iter()
        .map(|d| {
            format!(
                "__declspec(dllimport) int f_{}(void);\n",
                d.replace('.', "_")
            )
        })
        .collect();
    let calls: String = imports
        .iter()
        .map(|d| format!("    f_{}();\n", d.replace('.', "_")))
        .collect();
    let c_path = dir.join(format!("{tag}.c"));
    std::fs::write(
        &c_path,
        format!("{decls}void mainCRTStartup(void) {{\n{calls}}}\n"),
    )
    .ok()?;

    let mut libs = Vec::new();
    for (i, dll) in imports.iter().enumerate() {
        let def = dir.join(format!("{tag}-{i}.def"));
        std::fs::write(
            &def,
            format!("LIBRARY {dll}\nEXPORTS\n  f_{}\n", dll.replace('.', "_")),
        )
        .ok()?;
        let lib = dir.join(format!("{tag}-{i}.lib"));
        if !ok(Command::new("lld-link")
            .arg("/lib")
            .arg(format!("/def:{}", def.display()))
            .arg(format!("/out:{}", lib.display()))
            .arg("/machine:x64"))
        {
            return None;
        }
        libs.push(lib);
    }
    let obj = dir.join(format!("{tag}.obj"));
    let exe = dir.join(format!("{tag}.exe"));
    if !ok(Command::new("clang")
        .args([
            "--target=x86_64-pc-windows-msvc",
            "-ffreestanding",
            "-fno-stack-protector",
            "-c",
        ])
        .arg(&c_path)
        .arg("-o")
        .arg(&obj))
    {
        return None;
    }
    let mut link = Command::new("lld-link");
    link.args([
        "/entry:mainCRTStartup",
        "/subsystem:console",
        "/nodefaultlib",
    ])
    .arg(format!("/out:{}", exe.display()))
    .arg(&obj);
    for lib in &libs {
        link.arg(lib);
    }
    if !ok(&mut link) {
        return None;
    }
    let _ = std::fs::remove_file(c_path);
    let _ = std::fs::remove_file(obj);
    for lib in libs {
        let _ = std::fs::remove_file(lib);
    }
    exe.is_file().then_some(exe)
}

// ---------------------------------------------------------------------------
// Wine or Proton, decided from the file
// ---------------------------------------------------------------------------

/// A host with Proton available.
fn host_with_proton() -> HostFacts {
    HostFacts {
        proton_build: Some(PathBuf::from(
            "/usr/share/steam/compatibilitytools.d/proton-cachyos-slr",
        )),
        ..host()
    }
}

#[test]
fn a_game_is_offered_proton_when_it_is_installed() {
    // The classification comes from the import table of the file on disk,
    // so this needs a real PE. Skipped when clang is unavailable.
    let Some(pe) = pe_fixture(&["KERNEL32.dll", "d3d11.dll"]) else {
        eprintln!("note: clang/lld-link missing; skipping");
        return;
    };
    let plan = decide(
        &report(ObjectFormat::Pe, Arch::X86_64),
        &host_with_proton(),
        &Policy::default(),
        &pe,
    );
    match plan.strategy {
        Strategy::Proton { .. } => {}
        other => panic!("a d3d11-importing PE should route to Proton, got {other:?}"),
    }
    assert!(matches!(
        plan.confidence,
        Confidence::Layer {
            layer: "proton",
            ..
        }
    ));
    assert!(plan.summary.contains("game"), "{}", plan.summary);
    assert!(plan.summary.contains("Proton"), "{}", plan.summary);
    let _ = std::fs::remove_file(&pe);
}

#[test]
fn a_plain_desktop_program_stays_on_wine_even_with_proton_installed() {
    // This is the point of the whole classification: Proton does not even
    // execute the program directly, it runs a steam.exe stub, and for a
    // business application that is machinery that can only hurt.
    let Some(pe) = pe_fixture(&["KERNEL32.dll", "SHELL32.dll", "COMCTL32.dll"]) else {
        eprintln!("note: clang/lld-link missing; skipping");
        return;
    };
    let plan = decide(
        &report(ObjectFormat::Pe, Arch::X86_64),
        &host_with_proton(),
        &Policy::default(),
        &pe,
    );
    assert!(
        matches!(plan.strategy, Strategy::Wine { .. }),
        "a desktop program must stay on Wine, got {:?}",
        plan.strategy
    );
    let _ = std::fs::remove_file(&pe);
}

#[test]
fn a_game_without_proton_is_run_by_wine_and_says_why_it_could_be_better() {
    let Some(pe) = pe_fixture(&["KERNEL32.dll", "d3d11.dll"]) else {
        eprintln!("note: clang/lld-link missing; skipping");
        return;
    };
    let plan = decide(
        &report(ObjectFormat::Pe, Arch::X86_64),
        &host(),
        &Policy::default(),
        &pe,
    );
    assert!(matches!(plan.strategy, Strategy::Wine { .. }));
    assert!(
        plan.summary
            .contains("Proton would be the better-tested path"),
        "the plan should say what would be better: {}",
        plan.summary
    );
    let _ = std::fs::remove_file(&pe);
}

#[test]
fn a_file_that_cannot_be_read_falls_back_to_wine_rather_than_failing() {
    // The import read is best-effort; a launcher must not refuse to run a
    // program because it could not classify it.
    let plan = decide(
        &report(ObjectFormat::Pe, Arch::X86_64),
        &host_with_proton(),
        &Policy::default(),
        Path::new("/definitely/not/here.exe"),
    );
    assert!(
        matches!(plan.strategy, Strategy::Wine { .. }),
        "{:?}",
        plan.strategy
    );
}

#[test]
fn proton_requires_cross_os_to_be_allowed() {
    let Some(pe) = pe_fixture(&["KERNEL32.dll", "dxgi.dll"]) else {
        eprintln!("note: clang/lld-link missing; skipping");
        return;
    };
    let p = Policy {
        allow_cross_os: false,
        ..Policy::default()
    };
    let plan = decide(
        &report(ObjectFormat::Pe, Arch::X86_64),
        &host_with_proton(),
        &p,
        &pe,
    );
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::CrossOsDisabled
        }
    );
    let _ = std::fs::remove_file(&pe);
}
