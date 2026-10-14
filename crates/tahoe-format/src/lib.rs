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
//! - `0xCAFEBABE` is disambiguated by `nfat_arch` plausibility plus a
//!   validity check on each slice's Mach-O magic, the same rule `file(1)`
//!   uses. When neither reading holds, the result is
//!   [`ObjectFormat::Ambiguous`] rather than a guess.

pub mod container;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use tahoe_core::{Arch, Endianness, ObjectFormat, OsAbi};

pub use container::{ContainerKind, DmgCompression, DmgTrailer};

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

/// One architecture slice inside a Mach-O universal ("fat") binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FatSlice {
    /// CPU the slice targets.
    pub arch: Arch,
    /// Raw `cputype`.
    pub cputype: i32,
    /// Raw `cpusubtype`.
    pub cpusubtype: i32,
    /// Byte offset of the slice within the file.
    pub offset: u64,
    /// Byte length of the slice.
    pub size: u64,
    /// Power-of-two alignment.
    pub align: u32,
}

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
    /// Mach-O fat header: the declared architectures.
    FatMachO {
        /// Raw `nfat_arch`.
        nfat_arch: Option<u32>,
        /// One entry per slice, when the table was reachable.
        slices: Vec<FatSlice>,
        /// True when the file looks like a Java class file rather than a
        /// universal binary.
        looks_like_java: bool,
    },
    /// `#!` script: interpreter path as written (may be empty).
    Script {
        /// Interpreter path, lossy-decoded.
        interpreter: String,
    },
    /// DMG result.
    Dmg {
        /// True only when the `koly` trailer was actually observed.
        confident: bool,
        /// Parsed trailer, when the tail was reachable.
        trailer: Option<Box<container::DmgTrailer>>,
        /// Compression methods found in the embedded block table.
        compression: Vec<container::DmgCompression>,
    },
    /// XAR archive: `(version, uncompressed_size)` from the header.
    Xar {
        /// Header version field.
        version: u16,
        /// Total uncompressed size of the archive's contents.
        uncompressed_size: u64,
    },
    /// OLE2 compound file: the reliable way to tell an MSI from an
    /// executable stub that shares the extension.
    Ole2 {
        /// Byte-order mark from the header (`0xFFFE` little-endian).
        byte_order: u16,
        /// Sector size, as a power of two.
        sector_shift: u16,
    },
    /// Property list, with the detected encoding.
    Plist {
        /// `true` for `bplist00`, `false` for XML.
        binary: bool,
    },
    /// macOS `.app` bundle, resolved to its executable.
    AppBundle {
        /// `CFBundleExecutable`, when the plist named one.
        executable: Option<String>,
        /// `CFBundleIdentifier`.
        bundle_id: Option<String>,
        /// `LSMinimumSystemVersion`.
        minimum_system_version: Option<String>,
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

/// Split a `#!` interpreter line into the interpreter and its arguments.
///
/// Linux's `binfmt_script` splits the shebang on the first whitespace run:
/// the first token is the interpreter to `execve`, and the remainder becomes
/// a single argument to it. That is why `#!/usr/bin/env python3` works — and
/// why treating the whole line as one path does not.
///
/// Returns the interpreter path and its arguments, both trimmed. An empty
/// line yields an empty interpreter.
#[must_use]
pub fn split_shebang(line: &str) -> (String, Vec<String>) {
    let mut parts = line.split_whitespace();
    let interpreter = parts.next().unwrap_or_default().to_owned();
    let args = parts.map(str::to_owned).collect();
    (interpreter, args)
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
            // FAT_MAGIC / FAT_CIGAM collide with Java class files. `file(1)`
            // resolves this by range: a Java class stores its major version
            // where the fat header stores `nfat_arch`, and the first released
            // class format was version 43, while no universal binary has ever
            // declared 20 or more architectures. That gives a usable gap.
            [0xca, 0xfe, 0xba, 0xbe] => return probe_fat(buf, Endianness::Big),
            [0xbe, 0xba, 0xfe, 0xca] => return probe_fat(buf, Endianness::Little),
            _ => {}
        }
    }
    if let Some(kind) = container::identify_container(buf) {
        return probe_container(buf, kind);
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

/// Highest `nfat_arch` that can plausibly be a universal binary.
///
/// Chosen from `file(1)`'s rule (`magic/Magdir/cafebabe`): the first
/// released Java class format was version 43, and no Mach-O universal
/// binary has ever carried anything close to 20 slices.
const MAX_PLAUSIBLE_FAT_ARCHES: u32 = 20;

/// Disambiguate `0xCAFEBABE` between a Java class file and a Mach-O fat
/// header, and parse the architecture table when it is the latter.
fn probe_fat(buf: &[u8], endian: Endianness) -> ProbeReport {
    let nfat_arch = read_u32(buf, 4, endian);
    let slices = parse_fat_slices(buf, endian, nfat_arch);
    let looks_like_java = match nfat_arch {
        Some(n) => n >= MAX_PLAUSIBLE_FAT_ARCHES,
        None => false,
    };
    // A plausible count *plus* at least one slice that actually starts with
    // a Mach-O magic is what makes this decisive rather than a guess.
    let has_valid_slice = !slices.is_empty()
        && slices.iter().all(|s| {
            let start = usize::try_from(s.offset).ok();
            start.is_some_and(|o| {
                buf.get(o..o + 4).is_some_and(|m| {
                    matches!(m, [0xcf, 0xfa, 0xed, 0xfe] | [0xce, 0xfa, 0xed, 0xfe])
                })
            })
        });

    let format = if looks_like_java {
        ObjectFormat::JavaClass
    } else if nfat_arch.is_some() && !has_valid_slice {
        // Plausible count but the table is not a readable Mach-O table.
        // Refusing is the honest answer: a wrong guess hands a Java class
        // to a Mach-O loader.
        ObjectFormat::Ambiguous {
            candidates: &["java-class", "fat-macho"],
        }
    } else {
        ObjectFormat::FatMachO
    };
    let arch = slices.first().map_or(Arch::Unknown, |s| s.arch);
    ProbeReport {
        format,
        arch,
        bits: slices
            .first()
            .and_then(|s| Arch::from_macho_cputype(s.cputype.unsigned_abs()).is_64bit())
            .map(|is64| if is64 { 64 } else { 32 }),
        endian: Some(endian),
        os_abi: OsAbi::MacOs,
        detail: FormatDetail::FatMachO {
            nfat_arch,
            slices,
            looks_like_java,
        },
    }
}

/// Parse the `fat_arch[]` table that follows a `fat_header`.
fn parse_fat_slices(buf: &[u8], endian: Endianness, nfat_arch: Option<u32>) -> Vec<FatSlice> {
    const FAT_ARCH_SIZE: usize = 20;
    let mut out = Vec::new();
    let Some(count) = nfat_arch else { return out };
    if count == 0 || count > MAX_PLAUSIBLE_FAT_ARCHES {
        return out;
    }
    for i in 0..count as usize {
        let base = 8 + i * FAT_ARCH_SIZE;
        let Some(raw_cputype) = read_u32(buf, base, endian) else {
            return Vec::new();
        };
        let Some(cpusubtype) = read_u32(buf, base + 4, endian) else {
            return Vec::new();
        };
        let Some(offset) = read_u32(buf, base + 8, endian) else {
            return Vec::new();
        };
        let Some(size) = read_u32(buf, base + 12, endian) else {
            return Vec::new();
        };
        let Some(align) = read_u32(buf, base + 16, endian) else {
            return Vec::new();
        };
        out.push(FatSlice {
            arch: Arch::from_macho_cputype(raw_cputype),
            cputype: raw_cputype as i32,
            cpusubtype: cpusubtype as i32,
            offset: u64::from(offset),
            size: u64::from(size),
            align,
        });
    }
    out
}

/// Build a report for a container format.
fn probe_container(buf: &[u8], kind: container::ContainerKind) -> ProbeReport {
    let detail = match kind {
        container::ContainerKind::Xar => {
            // Header layout: "xar!" then u16 size (big-endian), u16 version,
            // then u64 uncompressed size — both big-endian.
            let header_size = read_u16(buf, 4, Endianness::Big).unwrap_or(0);
            FormatDetail::Xar {
                version: read_u16(buf, 6, Endianness::Big).unwrap_or(0),
                uncompressed_size: u64::from(
                    read_u32(
                        buf,
                        8 + u32::from(header_size).min(1024) as usize,
                        Endianness::Big,
                    )
                    .unwrap_or(0),
                ),
            }
        }
        container::ContainerKind::Ole2 => FormatDetail::Ole2 {
            byte_order: read_u16(buf, 0x1C, Endianness::Little).unwrap_or(0),
            sector_shift: read_u16(buf, 0x1E, Endianness::Little).unwrap_or(0),
        },
        container::ContainerKind::BinaryPlist => FormatDetail::Plist { binary: true },
        container::ContainerKind::XmlPlist => FormatDetail::Plist { binary: false },
        container::ContainerKind::Dmg => FormatDetail::Dmg {
            confident: false,
            trailer: None,
            compression: Vec::new(),
        },
    };
    ProbeReport {
        format: kind.object_format(),
        arch: Arch::Unknown,
        bits: None,
        endian: None,
        os_abi: OsAbi::Unspecified,
        detail,
    }
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
    let Some(trailer) = container::parse_dmg_trailer(&tail) else {
        return Ok(report);
    };

    // Follow the trailer's XML pointer to read the real compression table.
    let mut compression = Vec::new();
    if trailer.has_block_table() && trailer.xml_length > 0 {
        let want = trailer.xml_length.min(4 * 1024 * 1024) as usize;
        let mut xml = vec![0u8; want];
        if file.seek(SeekFrom::Start(trailer.xml_offset)).is_ok()
            && file.read_exact(&mut xml).is_ok()
        {
            compression = container::scan_dmg_compression(&xml);
        }
    }

    Ok(ProbeReport {
        format: ObjectFormat::Dmg,
        arch: Arch::Unknown,
        bits: None,
        endian: None,
        os_abi: OsAbi::MacOs,
        detail: FormatDetail::Dmg {
            confident: true,
            trailer: Some(Box::new(trailer)),
            compression,
        },
    })
}

/// Unit tests live in `tests.rs` so the parser above stays readable.
#[cfg(test)]
mod tests;
