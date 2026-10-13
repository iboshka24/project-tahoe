// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-uel`: the Universal Execution Layer (decision stage).
//!
//! Given a [`ProbeReport`] plus [`HostFacts`], [`decide`] returns an
//! [`ExecutionPlan`]: a [`Strategy`] paired with a [`Confidence`].
//!
//! This crate **decides**; it does not spawn anything yet (that is roadmap
//! stage 2, the supervisor). Keeping decision logic pure means every branch
//! is unit-testable without Wine, Darling, or root.

use std::path::{Path, PathBuf};
use tahoe_core::{Arch, ObjectFormat};
use tahoe_format::{FormatDetail, ProbeReport, probe_file};

/// What the host can actually run, as observed (never assumed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFacts {
    /// CPU we are running on.
    pub host_arch: Arch,
    /// `wine` found in PATH.
    pub wine_available: bool,
    /// `darling` found in PATH.
    pub darling_available: bool,
    /// x86-64 emulator for foreign-arch PE (FEX-Emu or box64) found.
    pub emulator_available: bool,
    /// Flatpak available for sandboxed installs.
    pub flatpak_available: bool,
}

impl HostFacts {
    /// Probe the current machine. Read-only: checks PATH and nothing else.
    #[must_use]
    pub fn probe_current() -> Self {
        fn in_path(name: &str) -> bool {
            std::env::var_os("PATH").is_some_and(|paths| {
                std::env::split_paths(&paths).any(|dir| dir.join(name).is_file())
            })
        }
        Self {
            host_arch: Arch::host(),
            wine_available: in_path("wine"),
            darling_available: in_path("darling"),
            emulator_available: in_path("fex-emu") || in_path("box64") || in_path("qemu-x86_64"),
            flatpak_available: in_path("flatpak"),
        }
    }
}

/// Policy knobs for [`decide`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// Allow cross-OS strategies (Wine/Darling). Default true.
    pub allow_cross_os: bool,
    /// Allow speculative strategies (emulator wrapping, AppImage).
    /// Default false: the user must opt in.
    pub allow_speculative: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            allow_cross_os: true,
            allow_speculative: false,
        }
    }
}

/// How the UEL proposes to run a file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Strategy {
    /// ELF matching the host arch: direct `exec`.
    Native,
    /// Windows PE via Wine in its own prefix (separate process, ADR-0002).
    Wine {
        /// Prefix to use (derived, not created here).
        prefix: PathBuf,
    },
    /// Mach-O / `.app` via Darling in its own DPREFIX (separate process).
    Darling {
        /// DPREFIX to use (derived, not created here).
        prefix: PathBuf,
    },
    /// Foreign-arch binary wrapped in a user-mode emulator.
    Emulator {
        /// Which emulator backend to invoke.
        backend: EmulatorBackend,
    },
    /// Script run via its `#!` interpreter.
    Script {
        /// Interpreter path as written in the file.
        interpreter: PathBuf,
    },
    /// Refuse, with a reason a human can act on.
    Rejected {
        /// Why this file will not be executed.
        reason: RejectionReason,
    },
}

/// User-mode emulator backends the UEL knows about (all MIT-licensed,
///
/// all separate processes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulatorBackend {
    /// FEX-Emu (x86-64 on AArch64).
    Fex,
    /// box64 (x86-64 on AArch64/RISC-V/LoongArch).
    Box64,
    /// qemu-user (any-to-any, slowest, most general).
    QemuUser,
}

/// Why the UEL refuses to execute something.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RejectionReason {
    /// DMG is a disk image: mount it, do not execute it (ADR-0004).
    DmgIsNotExecutable,
    /// The bytes match several formats; guessing risks wrong dispatch.
    AmbiguousFormat {
        /// Candidates, as reported by the parser.
        candidates: &'static [&'static str],
    },
    /// Format not recognised at all.
    UnknownFormat,
    /// Cross-OS execution disabled by policy.
    CrossOsDisabled,
    /// The needed compatibility layer is not installed.
    LayerMissing {
        /// `wine`, `darling`, …
        layer: &'static str,
    },
    /// The plan would be speculative and the policy forbids it.
    SpeculativeDisabled,
    /// Foreign architecture with no emulator available.
    ArchMismatchNoEmulator {
        /// Architecture the binary needs.
        needed: Arch,
    },
}

/// How much the UEL believes its own plan (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Confidence {
    /// Same OS family, same arch, everything checks out.
    Verified,
    /// Same OS family and arch, some dependencies unverified.
    Plausible,
    /// Cross-OS translation layer: works for a *subset* of software.
    Layer {
        /// `wine`, `darling`, …
        layer: &'static str,
        /// The documented caveat, shown to the user.
        caveat: &'static str,
    },
    /// Attemptable, but nobody has measured this combination.
    Speculative,
    /// Do not attempt.
    Refused,
}

/// A strategy paired with the UEL's confidence in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    /// What to do.
    pub strategy: Strategy,
    /// How much we believe it.
    pub confidence: Confidence,
    /// One-line human summary, suitable for logging and UI.
    pub summary: String,
}

/// Decide how to run a probed file on a host under a policy.
///
/// Pure function: no filesystem access, no process spawning.
#[must_use]
pub fn decide(
    report: &ProbeReport,
    host: &HostFacts,
    policy: &Policy,
    source: &Path,
) -> ExecutionPlan {
    let plan = |strategy: Strategy, confidence: Confidence, summary: String| ExecutionPlan {
        strategy,
        confidence,
        summary,
    };
    let refused = |reason: RejectionReason, summary: String| {
        plan(Strategy::Rejected { reason }, Confidence::Refused, summary)
    };

    // DMG is data. Always. (ADR-0004)
    if report.format == ObjectFormat::Dmg {
        return refused(
            RejectionReason::DmgIsNotExecutable,
            format!(
                "{} is a disk image: mount it read-only and probe the .app inside; never execute it",
                source.display()
            ),
        );
    }

    if let ObjectFormat::Ambiguous { candidates } = &report.format {
        return refused(
            RejectionReason::AmbiguousFormat { candidates },
            format!(
                "{} matches several formats ({}); refusing to guess",
                source.display(),
                candidates.join(", ")
            ),
        );
    }

    match &report.format {
        ObjectFormat::Elf => {
            if report.arch == Arch::Unknown || report.arch == host.host_arch {
                plan(
                    Strategy::Native,
                    Confidence::Verified,
                    format!(
                        "{}: native ELF for {}, direct exec",
                        source.display(),
                        host.host_arch
                    ),
                )
            } else if host.emulator_available && policy.allow_speculative {
                plan(
                    Strategy::Emulator {
                        backend: EmulatorBackend::QemuUser,
                    },
                    Confidence::Speculative,
                    format!(
                        "{}: foreign-arch ELF ({}) via qemu-user; unmeasured combination",
                        source.display(),
                        report.arch
                    ),
                )
            } else if host.emulator_available {
                refused(
                    RejectionReason::SpeculativeDisabled,
                    format!(
                        "{}: foreign-arch ELF ({}) needs an emulator; enable speculative policy to attempt",
                        source.display(),
                        report.arch
                    ),
                )
            } else {
                refused(
                    RejectionReason::ArchMismatchNoEmulator {
                        needed: report.arch,
                    },
                    format!(
                        "{}: foreign-arch ELF ({}) with no emulator installed",
                        source.display(),
                        report.arch
                    ),
                )
            }
        }
        ObjectFormat::Pe => decide_pe(report, host, policy, source),
        ObjectFormat::MachO | ObjectFormat::FatMachO => decide_macho(report, host, policy, source),
        ObjectFormat::Script => match &report.detail {
            FormatDetail::Script { interpreter } if !interpreter.is_empty() => plan(
                Strategy::Script {
                    interpreter: PathBuf::from(interpreter),
                },
                Confidence::Plausible,
                format!("{}: script via {interpreter}", source.display()),
            ),
            _ => refused(
                RejectionReason::UnknownFormat,
                format!(
                    "{}: script with no usable interpreter line",
                    source.display()
                ),
            ),
        },
        ObjectFormat::AppImage => {
            if policy.allow_speculative {
                plan(
                    Strategy::Native,
                    Confidence::Speculative,
                    format!(
                        "{}: AppImage is unsandboxed upstream; running natively with a warning",
                        source.display()
                    ),
                )
            } else {
                refused(
                    RejectionReason::SpeculativeDisabled,
                    format!(
                        "{}: AppImage is not a sandbox; enable speculative policy to run it",
                        source.display()
                    ),
                )
            }
        }
        ObjectFormat::JavaClass | ObjectFormat::Unknown => refused(
            RejectionReason::UnknownFormat,
            format!(
                "{}: UEL does not execute this format ({})",
                source.display(),
                report.format
            ),
        ),
        // Covered above; the wildcard is required because ObjectFormat is
        // non-exhaustive across crates, and doubles as a tripwire: a new
        // format variant refuses loudly instead of falling through.
        _ => refused(RejectionReason::UnknownFormat, "unreachable".to_owned()),
    }
}

fn wine_prefix() -> PathBuf {
    std::env::var_os("WINEPREFIX")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".wine")))
        .unwrap_or_else(|| PathBuf::from("/tmp/tahoe-wineprefix"))
}

fn darling_prefix() -> PathBuf {
    std::env::var_os("DPREFIX")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".darling")))
        .unwrap_or_else(|| PathBuf::from("/tmp/tahoe-dprefix"))
}

fn decide_pe(
    report: &ProbeReport,
    host: &HostFacts,
    policy: &Policy,
    source: &Path,
) -> ExecutionPlan {
    if !policy.allow_cross_os {
        return ExecutionPlan {
            strategy: Strategy::Rejected {
                reason: RejectionReason::CrossOsDisabled,
            },
            confidence: Confidence::Refused,
            summary: format!(
                "{}: Windows PE but cross-OS execution is disabled",
                source.display()
            ),
        };
    }
    if !host.wine_available {
        return ExecutionPlan {
            strategy: Strategy::Rejected {
                reason: RejectionReason::LayerMissing { layer: "wine" },
            },
            confidence: Confidence::Refused,
            summary: format!("{}: Windows PE but wine is not installed", source.display()),
        };
    }
    // Wine 11's new WoW64 runs 32-bit PE inside a 64-bit process, so x86 PE
    // on an x86-64 host is routine. Any other arch mismatch needs emulation.
    let needs_emu = !matches!(
        (report.arch, host.host_arch),
        (Arch::X86_64, Arch::X86_64)
            | (Arch::X86, Arch::X86)
            | (Arch::X86, Arch::X86_64)
            | (Arch::Unknown, _)
    );
    if needs_emu {
        if host.emulator_available && policy.allow_speculative {
            return ExecutionPlan {
                strategy: Strategy::Emulator {
                    backend: EmulatorBackend::Fex,
                },
                confidence: Confidence::Speculative,
                summary: format!(
                    "{}: {} PE on {} via emulator + wine; unmeasured",
                    source.display(),
                    report.arch,
                    host.host_arch
                ),
            };
        }
        return ExecutionPlan {
            strategy: Strategy::Rejected {
                reason: RejectionReason::ArchMismatchNoEmulator {
                    needed: report.arch,
                },
            },
            confidence: Confidence::Refused,
            summary: format!(
                "{}: {} PE on {} needs an emulator (none available or speculative disabled)",
                source.display(),
                report.arch,
                host.host_arch
            ),
        };
    }
    ExecutionPlan {
        strategy: Strategy::Wine {
            prefix: wine_prefix(),
        },
        confidence: Confidence::Layer {
            layer: "wine",
            caveat: "Wine runs a subset of Windows software; 32-bit OpenGL has known WoW64 slowdowns",
        },
        summary: format!("{}: Windows PE via wine", source.display()),
    }
}

fn decide_macho(
    report: &ProbeReport,
    host: &HostFacts,
    policy: &Policy,
    source: &Path,
) -> ExecutionPlan {
    if !policy.allow_cross_os {
        return ExecutionPlan {
            strategy: Strategy::Rejected {
                reason: RejectionReason::CrossOsDisabled,
            },
            confidence: Confidence::Refused,
            summary: format!(
                "{}: Mach-O but cross-OS execution is disabled",
                source.display()
            ),
        };
    }
    if !host.darling_available {
        return ExecutionPlan {
            strategy: Strategy::Rejected {
                reason: RejectionReason::LayerMissing { layer: "darling" },
            },
            confidence: Confidence::Refused,
            summary: format!("{}: Mach-O but darling is not installed", source.display()),
        };
    }
    if report.arch != Arch::Unknown && report.arch != host.host_arch {
        return ExecutionPlan {
            strategy: Strategy::Rejected {
                reason: RejectionReason::ArchMismatchNoEmulator {
                    needed: report.arch,
                },
            },
            confidence: Confidence::Refused,
            summary: format!(
                "{}: {} Mach-O on {} (darling is {}-only upstream)",
                source.display(),
                report.arch,
                host.host_arch,
                host.host_arch
            ),
        };
    }
    ExecutionPlan {
        strategy: Strategy::Darling {
            prefix: darling_prefix(),
        },
        confidence: Confidence::Layer {
            layer: "darling",
            caveat: "Darling runs CLI Mach-O well; GUI AppKit support is minimal upstream",
        },
        summary: format!("{}: Mach-O via darling", source.display()),
    }
}

/// Probe a path and decide, in one call (used by the CLI).
///
/// # Errors
///
/// Propagates [`probe_file`] I/O errors.
pub fn probe_and_decide(
    path: &Path,
    host: &HostFacts,
    policy: &Policy,
) -> std::io::Result<(ProbeReport, ExecutionPlan)> {
    let report = probe_file(path)?;
    let plan = decide(&report, host, policy, path);
    Ok((report, plan))
}

#[cfg(test)]
mod tests;
