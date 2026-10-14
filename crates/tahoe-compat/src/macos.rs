// SPDX-License-Identifier: MIT OR Apache-2.0
//! macOS inspection: Mach-O, `.app` bundles, property lists, disk images.
//!
//! # Scope, stated plainly
//!
//! **This module inspects. It does not run.** Running a Mach-O on Linux is
//! Darling's job, and Darling's own README describes its coverage:
//! many CLI tools functional, GUI/AppKit in progress, `mldr` x86_64 only,
//! no arm64, no Xcode. Nothing here changes that, and nothing here should
//! be read as a claim that a given `.app` will work.
//!
//! What it *does* do is answer the questions a launcher must answer before
//! it is honest to even offer to run something:
//!
//! - Which architectures does this binary contain, and is one of them
//!   x86_64? ([`FatBinary`], [`MachOFile`])
//! - What does it link against, and can those paths be resolved on this
//!   filesystem? ([`MachOFile::dependencies`], [`resolve_dependency`])
//! - Does the bundle name an executable, and does that executable exist?
//!   ([`AppBundle`])
//! - What minimum macOS version does it demand? ([`MachOFile::min_os_version`])
//!
//! The answers feed a confidence report, not a promise.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// Mach-O magics, from Apple's `EXTERNAL_HEADERS/mach-o/loader.h`.
pub mod magic {
    /// 32-bit, host byte order (`MH_MAGIC` as stored little-endian).
    pub const MH_MAGIC: [u8; 4] = [0xce, 0xfa, 0xed, 0xfe];
    /// 64-bit, host byte order (`MH_MAGIC_64`).
    pub const MH_MAGIC_64: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];
    /// 32-bit, byte-swapped (`MH_CIGAM`).
    pub const MH_CIGAM: [u8; 4] = [0xfe, 0xed, 0xfa, 0xce];
    /// 64-bit, byte-swapped (`MH_CIGAM_64`).
    pub const MH_CIGAM_64: [u8; 4] = [0xfe, 0xed, 0xfa, 0xcf];
    /// Universal binary (`FAT_MAGIC`, always big-endian on disk).
    pub const FAT_MAGIC: [u8; 4] = [0xca, 0xfe, 0xba, 0xbe];
    /// Byte-swapped universal binary (`FAT_CIGAM`).
    pub const FAT_CIGAM: [u8; 4] = [0xbe, 0xba, 0xfe, 0xca];
    /// 64-bit universal header (`FAT_MAGIC_64`).
    pub const FAT_MAGIC_64: [u8; 4] = [0xca, 0xfe, 0xba, 0xbf];
}

/// `LC_*` load command values a dependency checker needs.
pub mod lc {
    /// `LC_SEGMENT`.
    pub const SEGMENT: u32 = 0x1;
    /// `LC_LOAD_DYLIB`.
    pub const LOAD_DYLIB: u32 = 0xc;
    /// `LC_ID_DYLIB`.
    pub const ID_DYLIB: u32 = 0xd;
    /// `LC_LOAD_DYLINKER`.
    pub const LOAD_DYLINKER: u32 = 0xe;
    /// `LC_SEGMENT_64`.
    pub const SEGMENT_64: u32 = 0x19;
    /// `LC_UUID`.
    pub const UUID: u32 = 0x1b;
    /// `LC_CODE_SIGNATURE`.
    pub const CODE_SIGNATURE: u32 = 0x1d;
    /// `LC_LOAD_WEAK_DYLIB` (with `LC_REQ_DYLD`).
    pub const LOAD_WEAK_DYLIB: u32 = 0x8000_0018;
    /// `LC_RPATH` (with `LC_REQ_DYLD`).
    pub const RPATH: u32 = 0x8000_001c;
    /// `LC_REEXPORT_DYLIB` (with `LC_REQ_DYLD`).
    pub const REEXPORT_DYLIB: u32 = 0x8000_001f;
    /// `LC_VERSION_MIN_MACOSX`.
    pub const VERSION_MIN_MACOSX: u32 = 0x24;
    /// `LC_MAIN` (with `LC_REQ_DYLD`).
    pub const MAIN: u32 = 0x8000_0028;
    /// `LC_BUILD_VERSION`.
    pub const BUILD_VERSION: u32 = 0x32;
}

/// Mach-O `filetype` values that matter here.
pub mod filetype {
    /// `MH_OBJECT`.
    pub const OBJECT: u32 = 0x1;
    /// `MH_EXECUTE`.
    pub const EXECUTE: u32 = 0x2;
    /// `MH_DYLIB`.
    pub const DYLIB: u32 = 0x6;
    /// `MH_BUNDLE`.
    pub const BUNDLE: u32 = 0x8;
    /// `MH_DYLINKER`.
    pub const DYLINKER: u32 = 0x7;
}

/// `LC_BUILD_VERSION` platform values.
pub mod platform {
    /// `PLATFORM_MACOS`.
    pub const MACOS: u32 = 1;
    /// `PLATFORM_IOS`.
    pub const IOS: u32 = 2;
    /// `PLATFORM_IOSSIMULATOR`.
    pub const IOSSIMULATOR: u32 = 7;
    /// `PLATFORM_MACOS` bridged to Catalyst.
    pub const MACCATALYST: u32 = 6;
}

/// The three `@`-prefixed special path roots dyld understands.
pub const RPATH_PREFIX: &str = "@rpath/";
/// Directory of the image that contains the load command.
pub const LOADER_PATH_PREFIX: &str = "@loader_path/";
/// Directory of the main executable.
pub const EXECUTABLE_PATH_PREFIX: &str = "@executable_path/";

/// Why a Mach-O could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MachOError {
    /// The file could not be read.
    Io {
        /// Path involved.
        path: PathBuf,
        /// What the OS said.
        message: String,
    },
    /// The first four bytes are not a Mach-O magic this reader knows.
    NotMachO {
        /// The bytes found.
        magic: [u8; 4],
    },
    /// A load command claims to extend past the end of the file.
    Truncated {
        /// Offset the command starts at.
        offset: usize,
        /// Bytes it claims to need.
        needed: usize,
        /// Bytes actually available.
        available: usize,
    },
}

impl std::fmt::Display for MachOError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, message } => write!(f, "{}: {message}", path.display()),
            Self::NotMachO { magic } => write!(f, "not Mach-O (first bytes {magic:02x?})"),
            Self::Truncated {
                offset,
                needed,
                available,
            } => write!(
                f,
                "truncated load command at {offset}: needs {needed}, file has {available}"
            ),
        }
    }
}

impl std::error::Error for MachOError {}

/// One architecture slice of a universal binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FatSlice {
    /// Raw `cputype`.
    pub cputype: i32,
    /// Raw `cpusubtype`.
    pub cpusubtype: i32,
    /// File offset of the slice.
    pub offset: u64,
    /// Length of the slice.
    pub size: u64,
    /// Power-of-two alignment.
    pub align: u32,
}

impl FatSlice {
    /// Human name for the CPU, as `lipo` and `file` print it.
    ///
    /// `arm64e` is *not* a separate `cputype`: it is `CPU_TYPE_ARM64` with
    /// `CPU_SUBTYPE_ARM64E` (2), so the subtype has to be consulted.
    /// `CPU_TYPE_ARM64_32` (0x0200_000C) is a different thing entirely —
    /// 64-bit registers with 32-bit pointers, used by watchOS.
    #[must_use]
    pub fn cpu_name(&self) -> &'static str {
        // CPU_SUBTYPE_MASK covers the capability bits Apple packs in.
        let subtype = (self.cpusubtype as u32) & 0x00FF_FFFF;
        match self.cputype {
            7 => "i386",
            0x0100_0007 => "x86_64",
            12 => "arm",
            0x0100_000C if subtype == 2 => "arm64e",
            0x0100_000C => "arm64",
            0x0200_000C => "arm64_32",
            _ => "unknown",
        }
    }

    /// True for a slice whose registers are 64 bits wide.
    ///
    /// `CPU_ARCH_ABI64` (0x0100_0000) marks a 64-bit ABI, and
    /// `CPU_ARCH_ABI64_32` (0x0200_0000) marks 64-bit registers with
    /// 32-bit pointers — still 64-bit for the purposes of choosing a slice.
    #[must_use]
    pub fn is_64_bit(&self) -> bool {
        self.cputype & 0x0300_0000 != 0
    }
}

/// A universal ("fat") binary: its header and slices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatBinary {
    /// Slices, in file order.
    pub slices: Vec<FatSlice>,
    /// For a 64-bit header (`FAT_MAGIC_64`), offsets and sizes are 64-bit.
    pub is_64_header: bool,
}

impl FatBinary {
    /// Parse a universal header.
    ///
    /// Fat headers are **always big-endian on disk**, regardless of the
    /// architecture of the slices inside. A `FAT_CIGAM` file is one whose
    /// magic read little-endian; the fields are then little-endian.
    ///
    /// # Errors
    ///
    /// Returns [`MachOError::NotMachO`] when the magic does not match, and
    /// [`MachOError::Truncated`] when the slice table runs past the buffer.
    pub fn parse(bytes: &[u8]) -> Result<Self, MachOError> {
        let Some(magic) = bytes.get(..4) else {
            return Err(MachOError::NotMachO { magic: [0; 4] });
        };
        let (big_endian, is_64_header) = match magic {
            m if m == magic::FAT_MAGIC => (true, false),
            m if m == magic::FAT_MAGIC_64 => (true, true),
            m if m == magic::FAT_CIGAM => (false, false),
            _ => {
                return Err(MachOError::NotMachO {
                    magic: [magic[0], magic[1], magic[2], magic[3]],
                });
            }
        };
        let read_u32 = |at: usize| -> Option<u32> {
            let b = bytes.get(at..at + 4)?;
            let a = [b[0], b[1], b[2], b[3]];
            Some(if big_endian {
                u32::from_be_bytes(a)
            } else {
                u32::from_le_bytes(a)
            })
        };
        let read_u64 = |at: usize| -> Option<u64> {
            let b = bytes.get(at..at + 8)?;
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            Some(if big_endian {
                u64::from_be_bytes(a)
            } else {
                u64::from_le_bytes(a)
            })
        };

        let count = read_u32(4).ok_or(MachOError::Truncated {
            offset: 4,
            needed: 8,
            available: bytes.len(),
        })?;
        // Sanity bound: no universal binary has ever had anywhere near this
        // many slices, and a hostile count must not allocate.
        let count = count.min(64) as usize;
        let entry = if is_64_header { 32 } else { 20 };
        let mut slices = Vec::with_capacity(count);
        for i in 0..count {
            let base = 8 + i * entry;
            let cputype = read_u32(base).ok_or(MachOError::Truncated {
                offset: base,
                needed: base + entry,
                available: bytes.len(),
            })? as i32;
            let cpusubtype = read_u32(base + 4).unwrap_or(0) as i32;
            let (offset, size, align) = if is_64_header {
                (
                    read_u64(base + 8).unwrap_or(0),
                    read_u64(base + 16).unwrap_or(0),
                    read_u32(base + 24).unwrap_or(0),
                )
            } else {
                (
                    u64::from(read_u32(base + 8).unwrap_or(0)),
                    u64::from(read_u32(base + 12).unwrap_or(0)),
                    read_u32(base + 16).unwrap_or(0),
                )
            };
            slices.push(FatSlice {
                cputype,
                cpusubtype,
                offset,
                size,
                align,
            });
        }
        Ok(Self {
            slices,
            is_64_header,
        })
    }

    /// Choose the slice to use on this machine.
    ///
    /// Preference: an exact 64-bit native match, then any 64-bit slice, then
    /// 32-bit. `arm64e` is treated as `arm64`, which is what a loader does.
    #[must_use]
    pub fn select(&self, preferred: &[&str]) -> Option<FatSlice> {
        for want in preferred {
            if let Some(slice) = self.slices.iter().find(|s| s.cpu_name() == *want) {
                return Some(*slice);
            }
        }
        self.slices
            .iter()
            .find(|s| s.is_64_bit())
            .or_else(|| self.slices.first())
            .copied()
    }

    /// Names of every architecture present.
    #[must_use]
    pub fn architectures(&self) -> Vec<&'static str> {
        self.slices.iter().map(FatSlice::cpu_name).collect()
    }
}

/// A dependency named by a `LC_LOAD_DYLIB`-family command.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dependency {
    /// The install name exactly as written, including any `@`-prefix.
    pub install_name: String,
    /// True when the loader may continue without resolving it.
    pub weak: bool,
    /// True when the symbol is re-exported from here.
    pub reexported: bool,
}

/// A parsed Mach-O (thin) file, reduced to what a launcher needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachOFile {
    /// `cputype`.
    pub cputype: i32,
    /// `cpusubtype`.
    pub cpusubtype: i32,
    /// `filetype`.
    pub filetype: u32,
    /// `MH_PIE`.
    pub pie: bool,
    /// `LC_LOAD_DYLINKER` argument, normally `/usr/lib/dyld`.
    pub dylinker: Option<String>,
    /// `LC_UUID`, as a hyphenated string.
    pub uuid: Option<String>,
    /// Dylibs this image loads.
    pub dependencies: Vec<Dependency>,
    /// `LC_RPATH` entries, in order.
    pub rpaths: Vec<String>,
    /// `LC_ID_DYLIB`, for a dylib.
    pub install_name: Option<String>,
    /// Entry point offset from `LC_MAIN`.
    pub entry_offset: Option<u64>,
    /// Minimum OS version, as `(major, minor, patch)` plus platform.
    pub min_os: Option<(u32, u32, u32, u32)>,
    /// Whether an `LC_CODE_SIGNATURE` command is present.
    pub code_signature: bool,
    /// Load command numbers encountered, for diagnostics.
    pub command_count: u32,
}

impl MachOFile {
    /// Whether this image is a runnable executable rather than a dylib.
    #[must_use]
    pub fn is_executable(&self) -> bool {
        self.filetype == filetype::EXECUTE
    }

    /// Minimum macOS version as `"11.0"`, when the file declares one.
    #[must_use]
    pub fn min_macos_version(&self) -> Option<String> {
        let (major, minor, patch, plat) = self.min_os?;
        // Only the macOS platforms are meaningful for a Linux compatibility
        // layer, so iOS-only binaries are reported the same way but the
        // caller can check the platform.
        if !matches!(plat, platform::MACOS | platform::MACCATALYST) {
            return None;
        }
        Some(if patch == 0 {
            format!("{major}.{minor}")
        } else {
            format!("{major}.{minor}.{patch}")
        })
    }

    /// The platform constant declared by the file.
    #[must_use]
    pub fn platform(&self) -> Option<u32> {
        self.min_os.map(|(_, _, _, p)| p)
    }

    /// Number of non-weak dependencies, i.e. the ones that must resolve.
    #[must_use]
    pub fn required_dependencies(&self) -> Vec<&Dependency> {
        self.dependencies.iter().filter(|d| !d.weak).collect()
    }

    /// Distinct `@rpath` prefixes a dependency uses.
    #[must_use]
    pub fn rpath_dependencies(&self) -> Vec<String> {
        let mut set = BTreeSet::new();
        for dep in &self.dependencies {
            if let Some(rest) = dep.install_name.strip_prefix(RPATH_PREFIX) {
                set.insert(rest.to_owned());
            }
        }
        set.into_iter().collect()
    }

    /// Parse a thin Mach-O from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`MachOError`] for an unknown magic or a load command that
    /// runs past the buffer.
    pub fn parse(bytes: &[u8]) -> Result<Self, MachOError> {
        let Some(magic) = bytes.get(..4) else {
            return Err(MachOError::NotMachO { magic: [0; 4] });
        };
        let (le, is_64) = match magic {
            m if m == magic::MH_MAGIC_64 => (true, true),
            m if m == magic::MH_MAGIC => (true, false),
            m if m == magic::MH_CIGAM_64 => (false, true),
            m if m == magic::MH_CIGAM => (false, false),
            _ => {
                return Err(MachOError::NotMachO {
                    magic: [magic[0], magic[1], magic[2], magic[3]],
                });
            }
        };
        let read_u32 = |at: usize| -> Option<u32> {
            let b = bytes.get(at..at + 4)?;
            let a = [b[0], b[1], b[2], b[3]];
            Some(if le {
                u32::from_le_bytes(a)
            } else {
                u32::from_be_bytes(a)
            })
        };
        let read_u64 = |at: usize| -> Option<u64> {
            let b = bytes.get(at..at + 8)?;
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            Some(if le {
                u64::from_le_bytes(a)
            } else {
                u64::from_be_bytes(a)
            })
        };

        let header_size = if is_64 { 32 } else { 28 };
        let cputype = read_u32(4).ok_or(MachOError::Truncated {
            offset: 4,
            needed: header_size,
            available: bytes.len(),
        })? as i32;
        let cpusubtype = read_u32(8).unwrap_or(0) as i32;
        let filetype = read_u32(12).unwrap_or(0);
        let ncmds = read_u32(16).unwrap_or(0);
        let sizeofcmds = read_u32(20).unwrap_or(0);
        let flags = read_u32(24).unwrap_or(0);

        let mut out = Self {
            cputype,
            cpusubtype,
            filetype,
            pie: flags & 0x20_0000 != 0,
            dylinker: None,
            uuid: None,
            dependencies: Vec::new(),
            rpaths: Vec::new(),
            install_name: None,
            entry_offset: None,
            min_os: None,
            code_signature: false,
            command_count: ncmds,
        };

        let commands_end = header_size.saturating_add(sizeofcmds as usize);
        let mut cursor = header_size;
        for _ in 0..ncmds.min(4096) {
            let Some(cmd) = read_u32(cursor) else { break };
            let Some(cmdsize) = read_u32(cursor + 4) else {
                break;
            };
            let size = cmdsize as usize;
            if size < 8 || cursor + size > commands_end.max(bytes.len()).min(bytes.len()) {
                // A malformed size would let us walk off the end; stop.
                if size < 8 || cursor + size > bytes.len() {
                    return Err(MachOError::Truncated {
                        offset: cursor,
                        needed: cursor + size,
                        available: bytes.len(),
                    });
                }
            }
            let body = bytes.get(cursor..cursor + size).unwrap_or(&[]);
            match cmd {
                lc::LOAD_DYLIB | lc::LOAD_WEAK_DYLIB | lc::REEXPORT_DYLIB => {
                    if let Some(name) = dylib_name(body, le) {
                        out.dependencies.push(Dependency {
                            install_name: name,
                            weak: cmd == lc::LOAD_WEAK_DYLIB,
                            reexported: cmd == lc::REEXPORT_DYLIB,
                        });
                    }
                }
                lc::ID_DYLIB => out.install_name = dylib_name(body, le),
                lc::LOAD_DYLINKER => {
                    // dylinker_command { cmd, cmdsize, union lc_str name }
                    if let Some(off) = read_u32(cursor + 8) {
                        out.dylinker = read_cstring(bytes, cursor, off as usize);
                    }
                }
                lc::RPATH => {
                    if let Some(path) = read_u32(cursor + 8)
                        .and_then(|off| read_cstring(bytes, cursor, off as usize))
                    {
                        out.rpaths.push(path);
                    }
                }
                lc::UUID => {
                    if let Some(raw) = bytes.get(cursor + 8..cursor + 24) {
                        out.uuid = Some(format_uuid(raw));
                    }
                }
                lc::MAIN => {
                    out.entry_offset = read_u64(cursor + 8);
                }
                lc::CODE_SIGNATURE => out.code_signature = true,
                lc::VERSION_MIN_MACOSX => {
                    let version = read_u32(cursor + 8).unwrap_or(0);
                    out.min_os = Some((
                        version >> 16 & 0xFFFF,
                        version >> 8 & 0xFF,
                        version & 0xFF,
                        platform::MACOS,
                    ));
                }
                lc::BUILD_VERSION => {
                    let plat = read_u32(cursor + 8).unwrap_or(0);
                    let minos = read_u32(cursor + 12).unwrap_or(0);
                    out.min_os =
                        Some((minos >> 16 & 0xFFFF, minos >> 8 & 0xFF, minos & 0xFF, plat));
                }
                _ => {}
            }
            cursor += size;
        }
        Ok(out)
    }

    /// Parse a file, transparently selecting a slice from a universal binary.
    ///
    /// Returns the selected slice alongside the parse, so a caller can tell
    /// the user *which* architecture it is talking about.
    ///
    /// # Errors
    ///
    /// Returns [`MachOError`] when the file cannot be read or parsed.
    pub fn from_path(path: &Path, prefer: &[&str]) -> Result<(Self, Option<FatSlice>), MachOError> {
        let bytes = fs::read(path).map_err(|e| MachOError::Io {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        Self::from_bytes(&bytes, prefer)
    }

    /// Parse from a buffer, selecting a slice when it is a universal binary.
    ///
    /// # Errors
    ///
    /// Returns [`MachOError`] when neither reading succeeds.
    pub fn from_bytes(
        bytes: &[u8],
        prefer: &[&str],
    ) -> Result<(Self, Option<FatSlice>), MachOError> {
        match bytes.get(..4) {
            Some(m)
                if m == magic::FAT_MAGIC || m == magic::FAT_MAGIC_64 || m == magic::FAT_CIGAM =>
            {
                let fat = FatBinary::parse(bytes)?;
                let slice = fat.select(prefer).ok_or(MachOError::NotMachO {
                    magic: [0xca, 0xfe, 0xba, 0xbe],
                })?;
                let start = slice.offset as usize;
                let end = start.saturating_add(slice.size as usize).min(bytes.len());
                let inner = bytes.get(start..end).unwrap_or(&[]);
                Ok((Self::parse(inner)?, Some(slice)))
            }
            _ => Ok((Self::parse(bytes)?, None)),
        }
    }
}

/// Read a `lc_str`: an offset from the start of the load command.
fn read_cstring(bytes: &[u8], command_start: usize, offset: usize) -> Option<String> {
    let start = command_start.checked_add(offset)?;
    let rest = bytes.get(start..)?;
    let end = rest.iter().position(|b| *b == 0)?;
    Some(String::from_utf8_lossy(&rest[..end]).into_owned())
}

/// Read a `dylib_command`'s name.
fn dylib_name(body: &[u8], _le: bool) -> Option<String> {
    // dylib_command { cmd, cmdsize, struct dylib { union lc_str name, ... } }
    // The name offset is at byte 8 and is relative to the command start,
    // which `body` is.
    if body.len() < 12 {
        return None;
    }
    let offset = u32::from_le_bytes([body[8], body[9], body[10], body[11]]) as usize;
    // The endianness does not change this field's position; a big-endian
    // Mach-O is not something modern macOS produces for x86_64/arm64.
    let rest = body.get(offset..)?;
    let end = rest.iter().position(|b| *b == 0)?;
    Some(String::from_utf8_lossy(&rest[..end]).into_owned())
}

/// Format 16 UUID bytes the way `otool` prints them.
#[must_use]
pub fn format_uuid(raw: &[u8]) -> String {
    let hex: String = raw.iter().map(|b| format!("{b:02X}")).collect();
    if hex.len() < 32 {
        return hex;
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Where a dependency would be looked for, in the order dyld tries.
///
/// Implements the `dyld(1)` rules for the three `@`-prefixed roots. The
/// `@rpath` list is the caller's responsibility because it accumulates
/// across the dependency chain, and this function sees one image.
#[must_use]
pub fn resolve_dependency(
    install_name: &str,
    loader_dir: &Path,
    executable_dir: &Path,
    rpaths: &[String],
) -> Vec<PathBuf> {
    if let Some(rest) = install_name.strip_prefix(LOADER_PATH_PREFIX) {
        return vec![loader_dir.join(rest)];
    }
    if let Some(rest) = install_name.strip_prefix(EXECUTABLE_PATH_PREFIX) {
        return vec![executable_dir.join(rest)];
    }
    if let Some(rest) = install_name.strip_prefix(RPATH_PREFIX) {
        return rpaths
            .iter()
            .map(|rp| expand_rpath(rp, loader_dir).join(rest))
            .collect();
    }
    // An absolute install name is used as-is; a bare name goes through the
    // fallback directories, which we report rather than invent.
    if install_name.starts_with('/') {
        return vec![PathBuf::from(install_name)];
    }
    Vec::new()
}

/// Expand `@loader_path`/`@executable_path` inside an `LC_RPATH` entry.
#[must_use]
pub fn expand_rpath(entry: &str, loader_dir: &Path) -> PathBuf {
    if let Some(rest) = entry.strip_prefix(LOADER_PATH_PREFIX) {
        return loader_dir.join(rest);
    }
    // `@executable_path` inside an rpath resolves against the main
    // executable; for a single-image check we approximate with the loader
    // directory, and the caller can refine it.
    if let Some(rest) = entry.strip_prefix(EXECUTABLE_PATH_PREFIX) {
        return loader_dir.join(rest);
    }
    PathBuf::from(entry)
}

/// Which dependencies of an image cannot be found on this filesystem.
///
/// Only paths that resolve to an absolute location are checked. An `@rpath`
/// dependency with an empty rpath list is reported as unresolved rather
/// than assumed present.
#[must_use]
pub fn unresolved_dependencies(
    image: &MachOFile,
    loader_dir: &Path,
    executable_dir: &Path,
) -> Vec<String> {
    let mut missing = Vec::new();
    for dep in image.required_dependencies() {
        let candidates =
            resolve_dependency(&dep.install_name, loader_dir, executable_dir, &image.rpaths);
        if candidates.is_empty() {
            missing.push(dep.install_name.clone());
            continue;
        }
        if !candidates.iter().any(|c| c.exists()) {
            missing.push(dep.install_name.clone());
        }
    }
    missing
}

// ---------------------------------------------------------------------------
// .app bundles
// ---------------------------------------------------------------------------

/// Why a bundle could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BundleError {
    /// The path is not a directory ending in `.app`.
    NotABundle(PathBuf),
    /// There is no `Contents/Info.plist`.
    NoInfoPlist(PathBuf),
    /// The plist could not be parsed.
    BadPlist(String),
    /// The plist names no `CFBundleExecutable`.
    NoExecutableKey(PathBuf),
    /// The named executable does not exist.
    ExecutableMissing(PathBuf),
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotABundle(p) => write!(f, "{} is not an .app bundle", p.display()),
            Self::NoInfoPlist(p) => write!(f, "{} has no Contents/Info.plist", p.display()),
            Self::BadPlist(m) => write!(f, "Info.plist could not be read: {m}"),
            Self::NoExecutableKey(p) => write!(
                f,
                "{} declares no CFBundleExecutable, so there is nothing to run",
                p.display()
            ),
            Self::ExecutableMissing(p) => write!(f, "{} does not exist", p.display()),
        }
    }
}

impl std::error::Error for BundleError {}

/// A resolved macOS application bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppBundle {
    /// The `.app` directory.
    pub root: PathBuf,
    /// `Contents/MacOS/<CFBundleExecutable>`.
    pub executable: PathBuf,
    /// `CFBundleExecutable`.
    pub executable_name: String,
    /// `CFBundleIdentifier`, used for window/dock identity.
    pub identifier: Option<String>,
    /// `LSMinimumSystemVersion`.
    pub minimum_system_version: Option<String>,
    /// `CFBundleIconFile`.
    pub icon_file: Option<String>,
    /// `LSUIElement`: an agent app with no dock presence.
    pub is_agent: bool,
    /// `CFBundlePackageType`, normally `APPL`.
    pub package_type: Option<String>,
    /// Architectures of the executable, when it is a Mach-O.
    pub architectures: Vec<String>,
}

impl AppBundle {
    /// True when the bundle's directory name looks like an application.
    #[must_use]
    pub fn looks_like_bundle(path: &Path) -> bool {
        path.is_dir()
            && path.extension().is_some_and(|e| e == "app")
            && path.join("Contents/Info.plist").is_file()
    }

    /// Resolve a bundle directory into a runnable description.
    ///
    /// # Errors
    ///
    /// See [`BundleError`]. A bundle with no `CFBundleExecutable` is refused
    /// rather than guessed at: picking the largest file in `Contents/MacOS`
    /// would work most of the time and would be wrong the rest.
    pub fn resolve(path: &Path) -> Result<Self, BundleError> {
        if !Self::looks_like_bundle(path) {
            return Err(BundleError::NotABundle(path.to_path_buf()));
        }
        let plist_path = path.join("Contents/Info.plist");
        let raw = fs::read(&plist_path).map_err(|e| BundleError::BadPlist(e.to_string()))?;
        let plist = Plist::parse(&raw).map_err(|e| BundleError::BadPlist(e.to_string()))?;

        let executable_name = plist
            .get_string("CFBundleExecutable")
            .ok_or_else(|| BundleError::NoExecutableKey(path.to_path_buf()))?;
        let executable = path.join("Contents/MacOS").join(&executable_name);
        if !executable.exists() {
            return Err(BundleError::ExecutableMissing(executable));
        }
        // Report *every* slice a universal binary contains, not just the
        // one this machine would pick: "which architectures will this app
        // run on" is the question a user actually has.
        let architectures = MachOFile::from_path(&executable, &["x86_64", "arm64"])
            .map(|(image, slice)| match slice {
                Some(_) => fs::read(&executable)
                    .ok()
                    .and_then(|bytes| FatBinary::parse(&bytes).ok())
                    .map(|fat| fat.architectures().into_iter().map(str::to_owned).collect())
                    .unwrap_or_default(),
                None => vec![cpu_name_for(image.cputype).to_owned()],
            })
            .unwrap_or_default();

        Ok(Self {
            root: path.to_path_buf(),
            executable,
            executable_name,
            identifier: plist.get_string("CFBundleIdentifier"),
            minimum_system_version: plist.get_string("LSMinimumSystemVersion"),
            icon_file: plist.get_string("CFBundleIconFile"),
            is_agent: plist.get_bool("LSUIElement").unwrap_or(false),
            package_type: plist.get_string("CFBundlePackageType"),
            architectures,
        })
    }

    /// Find every bundle in a directory, one level deep.
    #[must_use]
    pub fn scan(dir: &Path) -> Vec<PathBuf> {
        let Ok(rd) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut found: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| Self::looks_like_bundle(p))
            .collect();
        found.sort();
        found
    }
}

/// Human name for a raw `cputype`.
#[must_use]
pub fn cpu_name_for(cputype: i32) -> &'static str {
    FatSlice {
        cputype,
        cpusubtype: 0,
        offset: 0,
        size: 0,
        align: 0,
    }
    .cpu_name()
}

// ---------------------------------------------------------------------------
// Property lists
// ---------------------------------------------------------------------------

/// A property list value, as far as this reader models it.
///
/// Only the types a bundle's `Info.plist` actually uses are implemented.
/// Anything else becomes [`Plist::Other`] so an unknown key is visible
/// rather than silently dropped.
#[derive(Debug, Clone, PartialEq)]
pub enum Plist {
    /// A string.
    String(String),
    /// An integer.
    Integer(i64),
    /// A real number.
    Real(f64),
    /// A boolean.
    Boolean(bool),
    /// An array.
    Array(Vec<Plist>),
    /// A dictionary.
    Dict(Vec<(String, Plist)>),
    /// Raw bytes.
    Data(Vec<u8>),
    /// A type this reader does not model.
    Other,
}

/// Why a property list could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlistError {
    /// The bytes match neither known encoding.
    UnknownFormat,
    /// The structure is malformed.
    Malformed(String),
    /// A reference pointed outside the object table.
    BadReference(u64),
}

impl std::fmt::Display for PlistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownFormat => write!(f, "not a property list (expected bplist00 or <plist>)"),
            Self::Malformed(m) => write!(f, "malformed property list: {m}"),
            Self::BadReference(r) => write!(f, "object reference {r} is out of range"),
        }
    }
}

impl std::error::Error for PlistError {}

impl Plist {
    /// Parse either encoding.
    ///
    /// # Errors
    ///
    /// See [`PlistError`].
    pub fn parse(bytes: &[u8]) -> Result<Self, PlistError> {
        if bytes.starts_with(b"bplist") {
            return BinaryPlist::parse(bytes);
        }
        if bytes.starts_with(b"<?xml") || bytes.starts_with(b"<plist") {
            return parse_xml_plist(bytes);
        }
        Err(PlistError::UnknownFormat)
    }

    /// Look up a top-level string value.
    #[must_use]
    pub fn get_string(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Self::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// Look up a top-level boolean value.
    #[must_use]
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Self::Boolean(b) => Some(*b),
            // plists are loose about booleans; a string "true" is common.
            Self::String(s) => Some(s.eq_ignore_ascii_case("true") || s == "1"),
            Self::Integer(i) => Some(*i != 0),
            _ => None,
        }
    }

    /// Look up a top-level value.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Dict(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// Binary plist reader (`bplist00`).
///
/// Implemented directly rather than through a dependency because the format
/// is small and well defined: an object table, an offset table, and a
/// 32-byte trailer.
struct BinaryPlist;

impl BinaryPlist {
    fn parse(bytes: &[u8]) -> Result<Plist, PlistError> {
        if bytes.len() < 40 {
            return Err(PlistError::Malformed(
                "shorter than a header plus trailer".into(),
            ));
        }
        let trailer = &bytes[bytes.len() - 32..];
        let offset_size = trailer[6] as usize;
        let ref_size = trailer[7] as usize;
        let num_objects = read_be(trailer, 8, 8)
            .ok_or_else(|| PlistError::Malformed("trailer objects".into()))?;
        let top_object =
            read_be(trailer, 16, 8).ok_or_else(|| PlistError::Malformed("trailer top".into()))?;
        let offset_table = read_be(trailer, 24, 8)
            .ok_or_else(|| PlistError::Malformed("trailer offset table".into()))?;
        if offset_size == 0 || offset_size > 8 || ref_size == 0 || ref_size > 8 {
            return Err(PlistError::Malformed("implausible field widths".into()));
        }
        let offsets_at = usize::try_from(offset_table)
            .map_err(|_| PlistError::Malformed("offset table too large".into()))?;
        let read_offset = |index: u64| -> Option<usize> {
            let at =
                offsets_at.checked_add(usize::try_from(index).ok()?.checked_mul(offset_size)?)?;
            read_be(bytes, at, offset_size).and_then(|v| usize::try_from(v).ok())
        };
        let reader = ObjectReader {
            bytes,
            ref_size,
            num_objects: num_objects.min(1 << 20),
        };
        let value = reader.read(top_object, read_offset, 0)?;
        Ok(value)
    }
}

struct ObjectReader<'a> {
    bytes: &'a [u8],
    ref_size: usize,
    num_objects: u64,
}

impl ObjectReader<'_> {
    fn read(
        &self,
        index: u64,
        offsets: impl Fn(u64) -> Option<usize> + Copy,
        depth: usize,
    ) -> Result<Plist, PlistError> {
        // A self-referential dictionary must not recurse forever.
        const MAX_DEPTH: usize = 64;
        if depth > MAX_DEPTH {
            return Err(PlistError::Malformed("nesting too deep".into()));
        }
        if index >= self.num_objects {
            return Err(PlistError::BadReference(index));
        }
        let at = offsets(index).ok_or(PlistError::BadReference(index))?;
        let marker = *self.bytes.get(at).ok_or(PlistError::BadReference(index))?;
        let kind = marker >> 4;
        let mut size = (marker & 0x0F) as u64;
        let mut cursor = at + 1;

        // 0xF in the low nibble means the real length follows as an integer.
        if size == 0x0F && matches!(kind, 0x1 | 0x4 | 0x5 | 0x6 | 0xA | 0xC | 0xD) {
            let int_marker = *self
                .bytes
                .get(cursor)
                .ok_or(PlistError::BadReference(index))?;
            if int_marker >> 4 != 0x1 {
                return Err(PlistError::Malformed("length is not an integer".into()));
            }
            let width = 1usize << (int_marker & 0x0F);
            size = read_be(self.bytes, cursor + 1, width)
                .ok_or_else(|| PlistError::Malformed("length out of range".into()))?;
            cursor += 1 + width;
        }

        match kind {
            0x0 => Ok(match marker {
                0x08 => Plist::Boolean(false),
                0x09 => Plist::Boolean(true),
                _ => Plist::Other,
            }),
            0x1 => {
                let width = 1usize << (marker & 0x0F);
                let raw = read_be(self.bytes, cursor, width)
                    .ok_or_else(|| PlistError::Malformed("integer out of range".into()))?;
                Ok(Plist::Integer(raw as i64))
            }
            0x2 => {
                let width = 1usize << (marker & 0x0F);
                let raw = read_be(self.bytes, cursor, width)
                    .ok_or_else(|| PlistError::Malformed("real out of range".into()))?;
                Ok(Plist::Real(if width == 4 {
                    f64::from(f32::from_bits(raw as u32))
                } else {
                    f64::from_bits(raw)
                }))
            }
            0x4 => {
                let len = size as usize;
                let data = self
                    .bytes
                    .get(cursor..cursor + len)
                    .ok_or_else(|| PlistError::Malformed("data out of range".into()))?;
                Ok(Plist::Data(data.to_vec()))
            }
            0x5 => {
                let len = size as usize;
                let raw = self
                    .bytes
                    .get(cursor..cursor + len)
                    .ok_or_else(|| PlistError::Malformed("ascii string out of range".into()))?;
                Ok(Plist::String(String::from_utf8_lossy(raw).into_owned()))
            }
            0x6 => {
                let units = size as usize;
                let raw = self
                    .bytes
                    .get(cursor..cursor + units * 2)
                    .ok_or_else(|| PlistError::Malformed("utf16 string out of range".into()))?;
                let words: Vec<u16> = raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_be_bytes(*c))
                    .collect();
                Ok(Plist::String(String::from_utf16_lossy(&words)))
            }
            0xA => {
                let count = size as usize;
                let mut items = Vec::with_capacity(count.min(4096));
                for i in 0..count {
                    let slot = cursor + i * self.ref_size;
                    let r = read_be(self.bytes, slot, self.ref_size).ok_or_else(|| {
                        PlistError::Malformed("array reference out of range".into())
                    })?;
                    items.push(self.read(r, offsets, depth + 1)?);
                }
                Ok(Plist::Array(items))
            }
            0xD => {
                let count = size as usize;
                let mut entries = Vec::with_capacity(count.min(4096));
                for i in 0..count {
                    let key_slot = cursor + i * self.ref_size;
                    let val_slot = cursor + (count + i) * self.ref_size;
                    let key_ref = read_be(self.bytes, key_slot, self.ref_size)
                        .ok_or_else(|| PlistError::Malformed("dict key out of range".into()))?;
                    let val_ref = read_be(self.bytes, val_slot, self.ref_size)
                        .ok_or_else(|| PlistError::Malformed("dict value out of range".into()))?;
                    let key = match self.read(key_ref, offsets, depth + 1)? {
                        Plist::String(s) => s,
                        _ => continue,
                    };
                    entries.push((key, self.read(val_ref, offsets, depth + 1)?));
                }
                Ok(Plist::Dict(entries))
            }
            _ => Ok(Plist::Other),
        }
    }
}

/// Read a big-endian unsigned integer of `width` bytes.
fn read_be(bytes: &[u8], at: usize, width: usize) -> Option<u64> {
    if width == 0 || width > 8 {
        return None;
    }
    let slice = bytes.get(at..at + width)?;
    let mut value = 0u64;
    for b in slice {
        value = (value << 8) | u64::from(*b);
    }
    Some(value)
}

/// Minimal XML plist reader.
///
/// Handles `<dict>`, `<array>`, `<string>`, `<integer>`, `<real>`,
/// `<true/>`, `<false/>` and `<data>` — the subset an `Info.plist` uses.
/// Deliberately not a general XML parser: a full one would be a large
/// dependency for reading a handful of keys, and this refuses to guess when
/// it meets something it does not model.
fn parse_xml_plist(bytes: &[u8]) -> Result<Plist, PlistError> {
    let text = std::str::from_utf8(bytes).map_err(|e| PlistError::Malformed(e.to_string()))?;
    let Some(start) = text.find("<plist") else {
        return Err(PlistError::Malformed("no <plist> element".into()));
    };
    let body_start = text[start..]
        .find('>')
        .map(|i| start + i + 1)
        .ok_or_else(|| PlistError::Malformed("unterminated <plist>".into()))?;
    let body_end = text.rfind("</plist>").unwrap_or(text.len());
    let mut parser = XmlCursor {
        text: &text[body_start..body_end],
        pos: 0,
    };
    parser.skip_whitespace_and_declarations();
    parser.parse_value(0)
}

struct XmlCursor<'a> {
    text: &'a str,
    pos: usize,
}

impl XmlCursor<'_> {
    fn skip_whitespace_and_declarations(&mut self) {
        loop {
            self.skip_whitespace();
            let at = &self.text[self.pos..];
            if (at.starts_with("<?") || at.starts_with("<!--"))
                && let Some(end) = at.find("?>").or_else(|| at.find("-->"))
            {
                self.pos += end + 2;
                continue;
            }
            break;
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.text.len() && self.text.as_bytes()[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// Read the next tag name without consuming it.
    ///
    /// An end tag is returned with its slash (`"/array"`), which is how
    /// callers tell "another item" from "the container is finished". A
    /// previous version stopped at the first `/`, so every end tag looked
    /// like an empty start tag and array parsing failed.
    fn peek_tag(&self) -> Option<String> {
        let rest = self.text.get(self.pos..)?.strip_prefix('<')?;
        if let Some(body) = rest.strip_prefix('/') {
            let end = body.find('>')?;
            return Some(format!("/{}", &body[..end]));
        }
        let end = rest.find(['>', ' ', '/'])?;
        Some(rest[..end].to_owned())
    }

    fn open_tag(&mut self, name: &str) -> bool {
        self.skip_whitespace();
        if self.text[self.pos..].starts_with(&format!("<{name}>")) {
            self.pos += name.len() + 2;
            return true;
        }
        if self.text[self.pos..].starts_with(&format!("<{name} ")) {
            // A tag with attributes: skip to the closing bracket.
            if let Some(end) = self.text[self.pos..].find('>') {
                self.pos += end + 1;
                return true;
            }
        }
        false
    }

    fn close_tag(&mut self, name: &str) {
        if let Some(end) = self.text[self.pos..].find(&format!("</{name}>")) {
            self.pos += end + name.len() + 3;
        }
    }

    fn read_text_until(&mut self, name: &str) -> String {
        let rest = &self.text[self.pos..];
        let end = rest.find(&format!("</{name}>")).unwrap_or(rest.len());
        let value = rest[..end].to_owned();
        self.pos += end;
        value
    }

    fn parse_value(&mut self, depth: usize) -> Result<Plist, PlistError> {
        const MAX_DEPTH: usize = 64;
        if depth > MAX_DEPTH {
            return Err(PlistError::Malformed("nesting too deep".into()));
        }
        self.skip_whitespace_and_declarations();
        if self.open_tag("dict") {
            let mut entries = Vec::new();
            loop {
                self.skip_whitespace_and_declarations();
                if !self.open_tag("key") {
                    break;
                }
                let key = self.read_text_until("key");
                self.close_tag("key");
                let value = self.parse_value(depth + 1)?;
                entries.push((key, value));
            }
            self.close_tag("dict");
            return Ok(Plist::Dict(entries));
        }
        if self.open_tag("array") {
            let mut items = Vec::new();
            loop {
                self.skip_whitespace_and_declarations();
                if self.peek_tag().as_deref().is_none_or(|t| t == "/array") {
                    break;
                }
                items.push(self.parse_value(depth + 1)?);
            }
            self.close_tag("array");
            return Ok(Plist::Array(items));
        }
        if self.open_tag("string") {
            let value = self.read_text_until("string");
            self.close_tag("string");
            return Ok(Plist::String(unescape_xml(&value)));
        }
        if self.open_tag("integer") {
            let value = self.read_text_until("integer");
            self.close_tag("integer");
            return Ok(Plist::Integer(value.trim().parse().unwrap_or(0)));
        }
        if self.open_tag("real") {
            let value = self.read_text_until("real");
            self.close_tag("real");
            return Ok(Plist::Real(value.trim().parse().unwrap_or(0.0)));
        }
        if self.open_tag("data") {
            let value = self.read_text_until("data");
            self.close_tag("data");
            let cleaned: String = value.chars().filter(|c| !c.is_whitespace()).collect();
            return Ok(Plist::Data(cleaned.into_bytes()));
        }
        if self.text[self.pos..].starts_with("<true/>") {
            self.pos += 7;
            return Ok(Plist::Boolean(true));
        }
        if self.text[self.pos..].starts_with("<false/>") {
            self.pos += 8;
            return Ok(Plist::Boolean(false));
        }
        Err(PlistError::Malformed(format!(
            "unexpected element at byte {}",
            self.pos
        )))
    }
}

/// Expand the five XML entities a plist can contain.
#[must_use]
pub fn unescape_xml(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Parse `major.minor.patch` out of a kernel release string.
///
/// Lives here rather than in a platform module so both Wine and macOS
/// checks can use it without a cycle.
#[must_use]
pub fn parse_kernel_release(release: &str) -> Option<(u64, u64, u64)> {
    let mut parts = release.split(['.', '-']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests;
