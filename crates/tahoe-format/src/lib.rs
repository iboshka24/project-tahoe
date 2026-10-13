// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-format`: byte-level identification of executable formats.
//!
//! This crate answers *"what is this file?"* by reading its leading bytes.
//! It never executes anything, never follows symlinks implicitly, and never
//! guesses when the bytes are ambiguous (see [`ObjectFormat::Ambiguous`]).
//!
//! ## What is actually parsed
//!
//! - **ELF**: `EI_CLASS`, `EI_DATA`, `EI_OSABI`, `e_type`, `e_machine`.
//! - **PE**: `MZ` magic, `e_lfanew`, `PE\0\0` signature, COFF machine,
//!   optional-header magic (PE32 vs PE32+), subsystem.
//! - **Mach-O**: all six header magics, `cputype`, fat `nfat_arch`.
//! - **AppImage type 2**: ELF header plus `AI\x02` at offset 8.
//! - **DMG**: only via [`probe_file`], which inspects the *tail* of the file
//!   for the `koly` trailer. There is no stable DMG *head* magic, so
//!   [`probe_bytes`] honestly reports [`ObjectFormat::Unknown`] for DMG data.
//! - **Scripts**: `#!` interpreter line.
//!
//! ## Deliberate non-goals
//!
//! - No semantic dispatch ("run this only if `e_type == ET_DYN`") — that is
//!   the UEL's job, in userspace, with full parsing (ADR-0007).
//! - `0xCAFEBABE` is **always** [`ObjectFormat::Ambiguous`]: it is both a
//!   Java class file and a Mach-O fat header, and picking one would be a
//!   guess with a wrong-dispatch failure mode.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use tahoe_core::{Arch, Endianness, ObjectFormat, OsAbi};

/// How many head bytes [`probe_file`] reads.
pub const HEAD_LEN: usize = 512;
/// Size of the DMG `koly` trailer block [`probe_file`] inspects.
pub const KOLY_LEN: usize = 512;
/// DMG `koly` trailer magic.
pub const KOLY_MAGIC: &[u8; 4] = b"koly";
/// AppImage type 2 magic (`AI\x02`) at offset 8.
pub const APPIMAGE_MAGIC: &[u8; 3] = b"AI\x02";
/// Offset of the AppImage magic.
pub const APPIMAGE_OFFSET: usize = 8;

/// Structured detail for a [`ProbeReport`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormatDetail {
    /// ELF header fields.
    Elf {
        /// Raw `e_type` value.
        e_type: u16,
        /// Human-readable `e_type` name (`EXEC`, `DYN`, …).
        e_type_name: &'static str,
        /// Raw `EI_OSABI` byte.
        osabi_byte: u8,
    },
    /// PE header fields. `None` fields mean "not reachable in this buffer".
    Pe {
        /// COFF signature present at `e_lfanew`.
        has_pe_signature: bool,
        /// Raw COFF `Machine`, if reachable.
        machine: Option<u16>,
        /// Optional-header magic: `Some(true)` = PE32+, `Some(false)` = PE32.
        pe32_plus: Option<bool>,
        /// Optional-header `Subsystem`, if reachable.
        subsystem: Option<u16>,
    },
    /// Thin Mach-O header fields.
    MachO {
        /// Raw `cputype`.
        cputype: u32,
        /// True for the 64-bit magics.
        is_64: bool,
        /// True for the byte-swapped (`CIGAM`) magics.
        swapped: bool,
    },
    /// Mach-O fat header: number of architectures, if reachable.
    FatMachO {
        /// Raw `nfat_arch`, big- or little-endian per the magic.
        nfat_arch: Option<u32>,
    },
    /// `#!` script: interpreter path as written (may be empty).
    Script {
        /// Interpreter path, lossy-decoded.
        interpreter: String,
    },
    /// DMG heuristic result.
    Dmg {
        /// True only when the `koly` trailer was actually observed.
        confident: bool,
    },
    /// No further detail available.
    None,
}

/// The result of probing a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    /// Identified format (or ambiguity / unknown).
    pub format: ObjectFormat,
    /// CPU architecture, or [`Arch::Unknown`] when not determinable.
    pub arch: Arch,
    /// Address width in bits, when the header states it.
    pub bits: Option<u8>,
    /// Byte order, when the header states it unambiguously.
    pub endian: Option<Endianness>,
    /// Declared OS ABI.
    pub os_abi: OsAbi,
    /// Parsed header fields.
    pub detail: FormatDetail,
}

impl ProbeReport {
    /// An "we know nothing" report.
    fn unknown() -> Self {
        Self {
            format: ObjectFormat::Unknown,
            arch: Arch::Unknown,
            bits: None,
            endian: None,
            os_abi: OsAbi::Unspecified,
            detail: FormatDetail::None,
        }
    }
}

/// Human-readable ELF `e_type` name.
#[must_use]
pub fn elf_type_name(e_type: u16) -> &'static str {
    match e_type {
        0 => "NONE",
        1 => "REL",
        2 => "EXEC",
        3 => "DYN",
        4 => "CORE",
        _ => "UNKNOWN",
    }
}

/// Human-readable PE subsystem name.
#[must_use]
pub fn pe_subsystem_name(subsystem: u16) -> &'static str {
    match subsystem {
        1 => "NATIVE",
        2 => "GUI",
        3 => "CONSOLE",
        5 => "OS2_CUI",
        7 => "POSIX_CUI",
        9 => "WINDOWS_CE_GUI",
        10 => "EFI_APPLICATION",
        11 => "EFI_BOOT_SERVICE_DRIVER",
        12 => "EFI_RUNTIME_DRIVER",
        13 => "EFI_ROM",
        14 => "XBOX",
        16 => "WINDOWS_BOOT_APPLICATION",
        _ => "UNKNOWN",
    }
}

fn read_u16(buf: &[u8], offset: usize, endian: Endianness) -> Option<u16> {
    let b = buf.get(offset..offset + 2)?;
    Some(match endian {
        Endianness::Little => u16::from_le_bytes([b[0], b[1]]),
        Endianness::Big => u16::from_be_bytes([b[0], b[1]]),
    })
}

fn read_u32(buf: &[u8], offset: usize, endian: Endianness) -> Option<u32> {
    let b = buf.get(offset..offset + 4)?;
    Some(match endian {
        Endianness::Little => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        Endianness::Big => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
    })
}

/// Identify a file from its leading bytes.
///
/// Never panics for any input length, including empty. DMG data is reported
/// as [`ObjectFormat::Unknown`] here — use [`probe_file`] for the `koly`
/// tail check.
#[must_use]
pub fn probe_bytes(buf: &[u8]) -> ProbeReport {
    if buf.len() >= 4 && buf[0] == 0x7f && buf[1] == b'E' && buf[2] == b'L' && buf[3] == b'F' {
        return probe_elf(buf);
    }
    if buf.len() >= 2 && buf[0] == b'M' && buf[1] == b'Z' {
        return probe_pe(buf);
    }
    if buf.len() >= 4 {
        match [buf[0], buf[1], buf[2], buf[3]] {
            // 64-bit Mach-O, little-endian file (MH_MAGIC_64).
            [0xcf, 0xfa, 0xed, 0xfe] => return probe_macho(buf, Endianness::Little, true, false),
            // 32-bit Mach-O, little-endian file (MH_MAGIC).
            [0xce, 0xfa, 0xed, 0xfe] => return probe_macho(buf, Endianness::Little, false, false),
            // 32-bit Mach-O, big-endian file (MH_CIGAM).
            [0xfe, 0xed, 0xfa, 0xce] => return probe_macho(buf, Endianness::Big, false, true),
            // 64-bit Mach-O, big-endian file (MH_CIGAM_64).
            [0xfe, 0xed, 0xfa, 0xcf] => return probe_macho(buf, Endianness::Big, true, true),
            // FAT_MAGIC and FAT_CIGAM collide with Java class files.
            // We refuse to guess: report ambiguity (see module docs).
            [0xca, 0xfe, 0xba, 0xbe] => {
                let nfat_arch = read_u32(buf, 4, Endianness::Big);
                return ProbeReport {
                    format: ObjectFormat::Ambiguous {
                        candidates: &["java-class", "fat-macho"],
                    },
                    arch: Arch::Unknown,
                    bits: None,
                    endian: None,
                    os_abi: OsAbi::Unspecified,
                    detail: FormatDetail::FatMachO { nfat_arch },
                };
            }
            [0xbe, 0xba, 0xfe, 0xca] => {
                let nfat_arch = read_u32(buf, 4, Endianness::Little);
                return ProbeReport {
                    format: ObjectFormat::Ambiguous {
                        candidates: &["java-class", "fat-macho"],
                    },
                    arch: Arch::Unknown,
                    bits: None,
                    endian: None,
                    os_abi: OsAbi::Unspecified,
                    detail: FormatDetail::FatMachO { nfat_arch },
                };
            }
            _ => {}
        }
    }
    if buf.len() >= 2 && buf[0] == b'#' && buf[1] == b'!' {
        let end = buf.iter().position(|&b| b == b'\n').unwrap_or(buf.len());
        let interpreter = String::from_utf8_lossy(&buf[2..end]).trim().to_owned();
        return ProbeReport {
            format: ObjectFormat::Script,
            arch: Arch::Unknown,
            bits: None,
            endian: None,
            os_abi: OsAbi::Unspecified,
            detail: FormatDetail::Script { interpreter },
        };
    }
    ProbeReport::unknown()
}

fn probe_elf(buf: &[u8]) -> ProbeReport {
    let class = buf.get(4).copied();
    let data = buf.get(5).copied();
    let osabi_byte = buf.get(7).copied().unwrap_or(0);

    let bits = match class {
        Some(1) => Some(32),
        Some(2) => Some(64),
        _ => None,
    };
    let endian = match data {
        Some(1) => Some(Endianness::Little),
        Some(2) => Some(Endianness::Big),
        _ => None,
    };
    let os_abi = match osabi_byte {
        0 => OsAbi::ElfSystemV,
        3 => OsAbi::ElfLinux,
        _ => OsAbi::Unspecified,
    };

    // e_type / e_machine need 20 bytes; without them we still know it is ELF.
    let order = endian.unwrap_or(Endianness::Little);
    let (e_type, arch) = match (read_u16(buf, 16, order), read_u16(buf, 18, order)) {
        (Some(t), Some(m)) => (t, Arch::from_elf_machine(m)),
        _ => (0, Arch::Unknown),
    };
    let complete = read_u16(buf, 16, order).is_some() && read_u16(buf, 18, order).is_some();

    let is_appimage = buf.len() > APPIMAGE_OFFSET + APPIMAGE_MAGIC.len()
        && buf[APPIMAGE_OFFSET..].starts_with(APPIMAGE_MAGIC);

    let format = if is_appimage {
        ObjectFormat::AppImage
    } else {
        ObjectFormat::Elf
    };
    // A truncated ELF header still identifies the format, but fields may be
    // absent; e_type 0/NONE doubles as "not present".
    let detail = FormatDetail::Elf {
        e_type,
        e_type_name: if complete {
            elf_type_name(e_type)
        } else {
            "TRUNCATED"
        },
        osabi_byte,
    };
    ProbeReport {
        format,
        arch,
        bits,
        endian,
        os_abi,
        detail,
    }
}

fn probe_pe(buf: &[u8]) -> ProbeReport {
    let base = ProbeReport {
        format: ObjectFormat::Pe,
        arch: Arch::Unknown,
        bits: None,
        endian: Some(Endianness::Little),
        os_abi: OsAbi::Windows,
        detail: FormatDetail::Pe {
            has_pe_signature: false,
            machine: None,
            pe32_plus: None,
            subsystem: None,
        },
    };
    // e_lfanew lives at 0x3C; without 64 bytes we cannot even find the header.
    let Some(lfanew) = read_u32(buf, 0x3c, Endianness::Little) else {
        return base;
    };
    let Ok(hdr) = usize::try_from(lfanew) else {
        return base;
    };
    let Some(sig) = buf.get(hdr..hdr.saturating_add(6)) else {
        return base;
    };
    if sig.len() < 6 || sig[0] != b'P' || sig[1] != b'E' || sig[2] != 0 || sig[3] != 0 {
        return base;
    }
    let machine = u16::from_le_bytes([sig[4], sig[5]]);
    let arch = Arch::from_pe_machine(machine);
    let opt_magic = read_u16(buf, hdr + 24, Endianness::Little);
    let pe32_plus = opt_magic.map(|m| m == 0x20b);
    let bits = opt_magic.map(|m| if m == 0x20b { 64 } else { 32 });
    let subsystem = read_u16(buf, hdr + 24 + 68, Endianness::Little);
    ProbeReport {
        format: ObjectFormat::Pe,
        arch,
        bits,
        endian: Some(Endianness::Little),
        os_abi: OsAbi::Windows,
        detail: FormatDetail::Pe {
            has_pe_signature: true,
            machine: Some(machine),
            pe32_plus,
            subsystem,
        },
    }
}

fn probe_macho(buf: &[u8], endian: Endianness, is_64: bool, swapped: bool) -> ProbeReport {
    let cputype = read_u32(buf, 4, endian);
    ProbeReport {
        format: ObjectFormat::MachO,
        arch: cputype
            .map(Arch::from_macho_cputype)
            .unwrap_or(Arch::Unknown),
        bits: Some(if is_64 { 64 } else { 32 }),
        endian: Some(endian),
        os_abi: OsAbi::MacOs,
        detail: FormatDetail::MachO {
            cputype: cputype.unwrap_or(0),
            is_64,
            swapped,
        },
    }
}

/// Identify a file on disk.
///
/// Reads the head ([`HEAD_LEN`] bytes) and runs [`probe_bytes`]. If that
/// yields [`ObjectFormat::Unknown`] and the file is larger than the head, the
/// last [`KOLY_LEN`] bytes are inspected for the DMG `koly` trailer; only an
/// observed trailer upgrades the report to [`ObjectFormat::Dmg`].
///
/// # Errors
///
/// Returns [`std::io::Error`] if the file cannot be opened, read, or seeked.
pub fn probe_file(path: &Path) -> std::io::Result<ProbeReport> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut head = vec![0u8; HEAD_LEN.min(len as usize)];
    file.read_exact(&mut head)?;
    let report = probe_bytes(&head);
    if report.format != ObjectFormat::Unknown || len <= HEAD_LEN as u64 {
        return Ok(report);
    }
    let mut tail = vec![0u8; KOLY_LEN.min(len as usize)];
    file.seek(SeekFrom::End(-(tail.len() as i64)))?;
    file.read_exact(&mut tail)?;
    if tail.starts_with(KOLY_MAGIC) {
        return Ok(ProbeReport {
            format: ObjectFormat::Dmg,
            arch: Arch::Unknown,
            bits: None,
            endian: None,
            os_abi: OsAbi::MacOs,
            detail: FormatDetail::Dmg { confident: true },
        });
    }
    Ok(report)
}

/// Unit tests live in `tests.rs` so the parser above stays readable.
#[cfg(test)]
mod tests;
