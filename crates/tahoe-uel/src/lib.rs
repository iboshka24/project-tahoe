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

pub mod supervisor;
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
    /// A Windows installer package, run through `wine msiexec /i`.
    ///
    /// Distinct from [`Strategy::Wine`] because it is a *different* program
    /// with different arguments, and because an installer writes to the
    /// prefix: the caller should create the prefix before this runs.
    WineInstaller {
        /// Prefix to install into.
        prefix: PathBuf,
        /// The `.msi` file.
        package: PathBuf,
    },
    /// Mach-O / `.app` via Darling in its own DPREFIX (separate process).
    Darling {
        /// DPREFIX to use (derived, not created here).
        prefix: PathBuf,
    },
    /// A macOS `.app` bundle, resolved to the executable its `Info.plist`
    /// names and run through Darling.
    ///
    /// Distinct from [`Strategy::Darling`] because the caller must run the
    /// *inner* executable, not the bundle directory: `execve` on a
    /// directory fails, and picking the executable by hand would be a guess.
    MacBundle {
        /// DPREFIX to use.
        prefix: PathBuf,
        /// The bundle directory, for display and for `DYLD_ROOT_PATH`.
        bundle: PathBuf,
        /// `Contents/MacOS/<CFBundleExecutable>`.
        executable: PathBuf,
    },
    /// Foreign-arch binary wrapped in a user-mode emulator.
    Emulator {
        /// Which emulator backend to invoke.
        backend: EmulatorBackend,
    },
    /// Script run via its `#!` interpreter.
    ///
    /// The arguments exist because a shebang may carry them: Linux splits
    /// `#!/usr/bin/env python3` into the program `/usr/bin/env` and the
    /// argument `python3`, and a plan that forgot the split would try to
    /// exec a file literally named `/usr/bin/env python3`.
    Script {
        /// Interpreter to execute.
        interpreter: PathBuf,
        /// Arguments the interpreter receives before the script.
        interpreter_args: Vec<String>,
    },
    /// A container that must be **inspected, never executed**: a disk
    /// image or an installer archive.
    ///
    /// This is not a refusal in the sense of "we do not know what this is".
    /// We know exactly what it is, and the correct action is to mount or
    /// unpack it and look inside, and then plan for whatever we find.
    Inspect {
        /// What kind of container.
        kind: ContainerKind,
        /// What the caller should do with it, in one sentence.
        hint: String,
    },
    /// Refuse, with a reason a human can act on.
    Rejected {
        /// Why this file will not be executed.
        reason: RejectionReason,
    },
}

/// Container kinds the UEL can name. Mirrors `tahoe_format::ContainerKind`
/// but is owned here so the UEL's public API does not leak another crate's
/// enum on every match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    /// A UDIF disk image.
    Dmg,
    /// A XAR archive: `.pkg` installers and `.xip` archives.
    Xar,
    /// An OLE2 compound file: `.msi` installers.
    Ole2,
    /// A property list.
    Plist,
}

impl std::fmt::Display for ContainerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dmg => write!(f, "dmg"),
            Self::Xar => write!(f, "xar"),
            Self::Ole2 => write!(f, "ole2"),
            Self::Plist => write!(f, "plist"),
        }
    }
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

impl std::fmt::Display for Confidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Verified => write!(f, "verified"),
            Self::Plausible => write!(f, "plausible"),
            Self::Layer { layer, .. } => write!(f, "layer:{layer}"),
            Self::Speculative => write!(f, "speculative"),
            Self::Refused => write!(f, "refused"),
        }
    }
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

    // Containers are data. A disk image is mounted, an installer package is
    // unpacked or handed to an installer, and a plist is read. None of them
    // is ever handed to execve (ADR-0004). This is a distinct outcome from
    // "we do not know what this is": we know exactly what it is and what to
    // do with it.
    match &report.format {
        ObjectFormat::Dmg => {
            return plan(
                Strategy::Inspect {
                    kind: ContainerKind::Dmg,
                    hint: dmg_hint(report),
                },
                Confidence::Verified,
                format!(
                    "{} is a UDIF disk image: mount it read-only, then probe the .app inside",
                    source.display()
                ),
            );
        }
        ObjectFormat::Xar => {
            return plan(
                Strategy::Inspect {
                    kind: ContainerKind::Xar,
                    hint: "a XAR archive; a .pkg payload is a compressed cpio stream and a .xip holds a pbzx stream".to_owned(),
                },
                Confidence::Verified,
                format!("{} is a XAR archive: unpack it, do not execute it", source.display()),
            );
        }
        ObjectFormat::Plist => {
            return plan(
                Strategy::Inspect {
                    kind: ContainerKind::Plist,
                    hint: "a property list; read it for metadata rather than running it".to_owned(),
                },
                Confidence::Verified,
                format!("{} is a property list", source.display()),
            );
        }
        _ => {}
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
        ObjectFormat::Ole2 => decide_msi(report, host, policy, source),
        ObjectFormat::MachO | ObjectFormat::FatMachO => decide_macho(report, host, policy, source),
        ObjectFormat::AppBundle => decide_app_bundle(report, host, policy, source),
        ObjectFormat::Script => match &report.detail {
            FormatDetail::Script { interpreter } if !interpreter.is_empty() => {
                let (program, args) = tahoe_format::split_shebang(interpreter);
                plan(
                    Strategy::Script {
                        interpreter: PathBuf::from(&program),
                        interpreter_args: args,
                    },
                    Confidence::Plausible,
                    format!("{}: script via {interpreter}", source.display()),
                )
            }
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

/// A one-line summary of what a DMG contains, from its parsed trailer.
///
/// Reports the compression methods we actually found, because LZFSE needs
/// Apple's implementation while zlib and bzip2 do not — a distinction a
/// user should see before being told "mount it".
fn dmg_hint(report: &ProbeReport) -> String {
    let FormatDetail::Dmg {
        trailer,
        compression,
        ..
    } = &report.detail
    else {
        return "a UDIF disk image; mount it read-only and probe what is inside".to_owned();
    };
    let mut hint = String::from("mount read-only and probe the .app inside");
    if let Some(t) = trailer {
        if t.is_split_segment() {
            hint.push_str(&format!(
                "; this is segment {} of {}, so all segments are needed",
                t.segment_number, t.segment_count
            ));
        }
        if t.decoded_size() > 0 {
            hint.push_str(&format!(
                "; {} MiB decoded",
                t.decoded_size() / (1024 * 1024)
            ));
        }
    }
    if !compression.is_empty() {
        let names: Vec<&str> = compression.iter().map(|c| c.label()).collect();
        hint.push_str(&format!("; compression: {}", names.join(", ")));
        if compression.iter().any(|c| !c.is_widely_supported()) {
            hint.push_str(" (LZFSE needs Apple's decompressor; zlib/bzip2/LZMA do not)");
        }
    }
    hint
}

/// A `.msi` is a database an installer program reads, not a program.
fn decide_msi(
    _report: &ProbeReport,
    host: &HostFacts,
    policy: &Policy,
    source: &Path,
) -> ExecutionPlan {
    let refuse = |reason: RejectionReason, summary: String| ExecutionPlan {
        strategy: Strategy::Rejected { reason },
        confidence: Confidence::Refused,
        summary,
    };
    if !policy.allow_cross_os {
        return refuse(
            RejectionReason::CrossOsDisabled,
            format!(
                "{}: a Windows installer but cross-OS execution is disabled",
                source.display()
            ),
        );
    }
    if !host.wine_available {
        return refuse(
            RejectionReason::LayerMissing { layer: "wine" },
            format!(
                "{}: a Windows installer; wine is needed to run msiexec and is not installed",
                source.display()
            ),
        );
    }
    ExecutionPlan {
        strategy: Strategy::WineInstaller {
            prefix: wine_prefix(),
            package: source.to_path_buf(),
        },
        confidence: Confidence::Layer {
            layer: "wine",
            caveat: "Wine runs a subset of Windows installers; the package may fail partway",
        },
        summary: format!("{}: Windows installer via wine msiexec", source.display()),
    }
}

/// A `.app` bundle: run the executable its plist names, through Darling.
fn decide_app_bundle(
    report: &ProbeReport,
    host: &HostFacts,
    policy: &Policy,
    source: &Path,
) -> ExecutionPlan {
    let refuse = |reason: RejectionReason, summary: String| ExecutionPlan {
        strategy: Strategy::Rejected { reason },
        confidence: Confidence::Refused,
        summary,
    };
    let FormatDetail::AppBundle {
        executable,
        minimum_system_version,
        ..
    } = &report.detail
    else {
        return refuse(
            RejectionReason::UnknownFormat,
            format!(
                "{}: an .app bundle whose Info.plist named no executable",
                source.display()
            ),
        );
    };
    let Some(executable) = executable else {
        return refuse(
            RejectionReason::UnknownFormat,
            format!(
                "{}: the bundle declares no CFBundleExecutable, so there is nothing to run",
                source.display()
            ),
        );
    };
    if !policy.allow_cross_os {
        return refuse(
            RejectionReason::CrossOsDisabled,
            format!(
                "{}: a macOS bundle but cross-OS execution is disabled",
                source.display()
            ),
        );
    }
    if !host.darling_available {
        return refuse(
            RejectionReason::LayerMissing { layer: "darling" },
            format!(
                "{}: a macOS bundle; darling is not installed",
                source.display()
            ),
        );
    }
    // Darling is x86_64-only upstream (mldr has `#error TODO: aarch64`), so
    // an arm64-only bundle has nowhere to run even with Darling present.
    if report.arch != Arch::Unknown && report.arch != host.host_arch {
        return refuse(
            RejectionReason::ArchMismatchNoEmulator {
                needed: report.arch,
            },
            format!(
                "{}: a {} bundle, but darling only supports {} today",
                source.display(),
                report.arch,
                host.host_arch
            ),
        );
    }
    let executable = PathBuf::from(executable);
    // The caveat is deliberately the same string the plain Mach-O path
    // uses: Darling's limits do not depend on how the binary was packaged,
    // and a user who sees a .app should get the same warning as a user who
    // sees a bare Mach-O.
    let summary = match minimum_system_version {
        Some(min) => format!(
            "{}: macOS bundle via darling (declares minimum macOS {min}; compatibility unmeasured)",
            source.display()
        ),
        None => format!("{}: macOS bundle via darling", source.display()),
    };
    ExecutionPlan {
        strategy: Strategy::MacBundle {
            prefix: darling_prefix(),
            bundle: source.to_path_buf(),
            executable,
        },
        confidence: Confidence::Layer {
            layer: "darling",
            caveat: "Darling runs x86_64 CLI and Foundation-class software; GUI/AppKit is partial",
        },
        summary,
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
