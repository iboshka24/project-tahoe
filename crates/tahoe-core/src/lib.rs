// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-core`: shared domain types for Project Tahoe.
//!
//! This crate intentionally has **no dependencies and no I/O**. It is the
//! vocabulary that `tahoe-format`, `tahoe-uel`, `tahoe-compositor` and the
//! shell all agree on, so that a `ProbeReport` means the same thing
//! everywhere. If you need to read a file, parse bytes, or spawn a process,
//! you are in the wrong crate.

use core::fmt;

/// CPU architecture, as relevant for execution decisions.
///
/// The numeric values are the ELF `e_machine` codes where one exists;
/// architectures without an ELF code use a Tahoe-local sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Arch {
    /// x86, 32-bit (`EM_386 = 3`).
    X86,
    /// x86-64 (`EM_X86_64 = 62`).
    X86_64,
    /// 32-bit ARM (`EM_ARM = 40`).
    Arm32,
    /// AArch64 (`EM_AARCH64 = 183`).
    Aarch64,
    /// RISC-V 64-bit (`EM_RISCV = 243`).
    Riscv64,
    /// LoongArch (`EM_LOONGARCH = 258`).
    Loongarch64,
    /// An ELF `e_machine` value we do not model.
    OtherElf(u16),
    /// Architecture unknown (not enough bytes, or a non-ELF format).
    Unknown,
}

impl Arch {
    /// Build from an ELF `e_machine` field value.
    #[must_use]
    pub fn from_elf_machine(machine: u16) -> Self {
        match machine {
            3 => Self::X86,
            62 => Self::X86_64,
            40 => Self::Arm32,
            183 => Self::Aarch64,
            243 => Self::Riscv64,
            258 => Self::Loongarch64,
            other => Self::OtherElf(other),
        }
    }

    /// Build from a Mach-O `cputype` value (low 24 bits are the type,
    /// bit 24 (`CPU_ARCH_ABI64`) marks 64-bit).
    #[must_use]
    pub fn from_macho_cputype(cputype: u32) -> Self {
        match cputype {
            7 => Self::X86,
            0x0100_0007 => Self::X86_64,
            12 => Self::Arm32,
            0x0100_000C => Self::Aarch64,
            other => Self::OtherElf((other & 0xFFFF) as u16),
        }
    }

    /// Build from a PE COFF `Machine` field value.
    #[must_use]
    pub fn from_pe_machine(machine: u16) -> Self {
        match machine {
            0x014c => Self::X86,
            0x8664 => Self::X86_64,
            0x01c0 => Self::Arm32,
            0xaa64 => Self::Aarch64,
            0x5064 => Self::Riscv64,
            0x6264 => Self::Loongarch64,
            other => Self::OtherElf(other),
        }
    }

    /// True if this is a 64-bit architecture (to the best of our knowledge).
    #[must_use]
    pub fn is_64bit(self) -> Option<bool> {
        match self {
            Self::X86 | Self::Arm32 => Some(false),
            Self::X86_64 | Self::Aarch64 | Self::Riscv64 | Self::Loongarch64 => Some(true),
            Self::OtherElf(_) | Self::Unknown => None,
        }
    }

    /// The architecture this library was compiled for (the host we run on).
    #[must_use]
    pub fn host() -> Self {
        #[cfg(target_arch = "x86")]
        return Self::X86;
        #[cfg(target_arch = "x86_64")]
        return Self::X86_64;
        #[cfg(target_arch = "arm")]
        return Self::Arm32;
        #[cfg(target_arch = "aarch64")]
        return Self::Aarch64;
        #[cfg(target_arch = "riscv64")]
        return Self::Riscv64;
        #[cfg(target_arch = "loongarch64")]
        return Self::Loongarch64;
        #[cfg(not(any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "riscv64",
            target_arch = "loongarch64"
        )))]
        return Self::Unknown;
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::X86 => write!(f, "x86"),
            Self::X86_64 => write!(f, "x86-64"),
            Self::Arm32 => write!(f, "arm32"),
            Self::Aarch64 => write!(f, "aarch64"),
            Self::Riscv64 => write!(f, "riscv64"),
            Self::Loongarch64 => write!(f, "loongarch64"),
            Self::OtherElf(v) => write!(f, "elf-machine-{v}"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Byte order of a binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endianness {
    /// Least significant byte first (`EI_DATA = 1`).
    Little,
    /// Most significant byte first (`EI_DATA = 2`).
    Big,
}

/// The OS ABI a binary targets, as far as headers reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OsAbi {
    /// The binary does not declare an OS ABI (or the format has none).
    Unspecified,
    /// ELF `EI_OSABI = 0` (System V) or `3` (Linux).
    ElfSystemV,
    /// ELF `EI_OSABI = 3`.
    ElfLinux,
    /// A Windows PE binary (subsystem recorded separately).
    Windows,
    /// A macOS Mach-O binary.
    MacOs,
}

impl fmt::Display for OsAbi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unspecified => write!(f, "unspecified"),
            Self::ElfSystemV => write!(f, "elf-system-v"),
            Self::ElfLinux => write!(f, "elf-linux"),
            Self::Windows => write!(f, "windows"),
            Self::MacOs => write!(f, "macos"),
        }
    }
}

/// What kind of object a file is, as determined by `tahoe-format`.
///
/// This is an **identification**, not a promise of executability. See
/// `ARCHITECTURE.md` §6 ("Honesty guarantees").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ObjectFormat {
    /// ELF executable, shared object, relocatable or core — see `e_type`.
    Elf,
    /// Windows PE (starts with `MZ`).
    Pe,
    /// macOS Mach-O, thin (32- or 64-bit).
    MachO,
    /// macOS Mach-O universal ("fat") header.
    FatMachO,
    /// AppImage type 2 (ELF with `AI\x02` at offset 8).
    AppImage,
    /// macOS disk image (UDIF). **Data, never executed** (ADR-0004).
    Dmg,
    /// Script with a `#!` interpreter line.
    Script,
    /// Java class file (`CAFEBABE`).
    JavaClass,
    /// The bytes match more than one format; candidates are listed and
    /// the caller must not guess.
    Ambiguous {
        /// Human-readable candidate names, most likely first.
        candidates: &'static [&'static str],
    },
    /// Not recognised.
    Unknown,
}

impl fmt::Display for ObjectFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Elf => write!(f, "elf"),
            Self::Pe => write!(f, "pe"),
            Self::MachO => write!(f, "macho"),
            Self::FatMachO => write!(f, "fat-macho"),
            Self::AppImage => write!(f, "appimage"),
            Self::Dmg => write!(f, "dmg"),
            Self::Script => write!(f, "script"),
            Self::JavaClass => write!(f, "java-class"),
            Self::Ambiguous { candidates } => {
                write!(f, "ambiguous({})", candidates.join("|"))
            }
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elf_machine_roundtrip_common() {
        assert_eq!(Arch::from_elf_machine(3), Arch::X86);
        assert_eq!(Arch::from_elf_machine(62), Arch::X86_64);
        assert_eq!(Arch::from_elf_machine(40), Arch::Arm32);
        assert_eq!(Arch::from_elf_machine(183), Arch::Aarch64);
        assert_eq!(Arch::from_elf_machine(243), Arch::Riscv64);
        assert_eq!(Arch::from_elf_machine(258), Arch::Loongarch64);
    }

    #[test]
    fn elf_machine_unknown_passthrough() {
        assert_eq!(Arch::from_elf_machine(0x1234), Arch::OtherElf(0x1234));
    }

    #[test]
    fn macho_cputype_known() {
        assert_eq!(Arch::from_macho_cputype(7), Arch::X86);
        assert_eq!(Arch::from_macho_cputype(0x0100_0007), Arch::X86_64);
        assert_eq!(Arch::from_macho_cputype(12), Arch::Arm32);
        assert_eq!(Arch::from_macho_cputype(0x0100_000C), Arch::Aarch64);
    }

    #[test]
    fn pe_machine_known() {
        assert_eq!(Arch::from_pe_machine(0x014c), Arch::X86);
        assert_eq!(Arch::from_pe_machine(0x8664), Arch::X86_64);
        assert_eq!(Arch::from_pe_machine(0xaa64), Arch::Aarch64);
    }

    #[test]
    fn bitness_known_arches() {
        assert_eq!(Arch::X86.is_64bit(), Some(false));
        assert_eq!(Arch::X86_64.is_64bit(), Some(true));
        assert_eq!(Arch::Aarch64.is_64bit(), Some(true));
        assert_eq!(Arch::Unknown.is_64bit(), None);
    }

    #[test]
    fn host_is_known_on_ci_machines() {
        assert_ne!(Arch::host(), Arch::Unknown);
    }

    #[test]
    fn display_is_stable() {
        assert_eq!(Arch::X86_64.to_string(), "x86-64");
        assert_eq!(OsAbi::Windows.to_string(), "windows");
        assert_eq!(ObjectFormat::Pe.to_string(), "pe");
        assert_eq!(
            ObjectFormat::Ambiguous {
                candidates: &["java-class", "fat-macho"]
            }
            .to_string(),
            "ambiguous(java-class|fat-macho)"
        );
    }
}
