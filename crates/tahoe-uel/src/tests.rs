// SPDX-License-Identifier: MIT OR Apache-2.0
//! Decision-table tests for [`crate::decide`]. Every branch that matters to
//! a user double-clicking a file is represented here.

use crate::{Confidence, HostFacts, Policy, RejectionReason, Strategy, decide};
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
fn dmg_is_never_executed() {
    let r = ProbeReport {
        format: ObjectFormat::Dmg,
        detail: FormatDetail::Dmg { confident: true },
        ..report(ObjectFormat::Dmg, Arch::Unknown)
    };
    let plan = decide(&r, &host(), &Policy::default(), &src());
    assert_eq!(
        plan.strategy,
        Strategy::Rejected {
            reason: RejectionReason::DmgIsNotExecutable
        }
    );
    assert!(plan.summary.contains("never") || plan.summary.contains("mount"));
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
            interpreter: PathBuf::from("/bin/sh")
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
