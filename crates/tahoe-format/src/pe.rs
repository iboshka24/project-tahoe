// SPDX-License-Identifier: MIT OR Apache-2.0
//! PE import-table parsing.
//!
//! # Why this exists
//!
//! Wine and Proton are not interchangeable. Proton is Valve's fork tuned for
//! games: its `run` verb does not even execute your program directly, it
//! executes a `steam.exe` stub with your program as an argument, seeds the
//! prefix with Steam client DLLs, and applies per-appid workarounds. For a
//! business application that is unnecessary machinery; for a game it is the
//! difference between working and not.
//!
//! So the launcher has to decide *per program*, and the only honest input for
//! that decision is what the program actually links against. A PE that
//! imports `d3d11.dll` and `dxgi.dll` is a game or a 3D application. One that
//! imports `shell32.dll` and `comctl32.dll` is a normal desktop program.
//!
//! This is a heuristic and the code says so. It is also *honest*: it reads
//! the file instead of the filename, and when it cannot tell, it says so
//! rather than guessing.
//!
//! # Format
//!
//! The import directory is optional-header data directory index 1, pointing
//! at an array of 20-byte `IMAGE_IMPORT_DESCRIPTOR`s terminated by an
//! all-zero entry. Each descriptor names a DLL and points at a thunk table.
//! Reaching any of it requires mapping an RVA to a file offset through the
//! section table, which is the part that actually takes care.

use tahoe_core::Arch;

/// How many imports are read before giving up.
///
/// A hostile or corrupt file can claim an arbitrary count; the descriptors
/// are terminated by a zero entry, but that terminator may be missing.
const MAX_DESCRIPTORS: usize = 4096;

/// One DLL a PE links against.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImportedDll {
    /// The name as written, e.g. `KERNEL32.dll`. Case varies by toolchain,
    /// so comparisons should be case-insensitive.
    pub name: String,
    /// Number of named functions imported from it, when the thunk table was
    /// reachable. `None` means "the DLL is imported but we could not count".
    pub function_count: Option<u32>,
}

impl ImportedDll {
    /// The lowercase name, for matching.
    #[must_use]
    pub fn lower(&self) -> String {
        self.name.to_ascii_lowercase()
    }
}

/// What a PE's imports say about how it should be run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramKind {
    /// Imports a 3D API. Proton is the better-tested path.
    Game,
    /// Imports only ordinary desktop APIs.
    DesktopApplication,
    /// Nothing was imported, or the table was not readable.
    Unknown,
}

/// The APIs whose presence suggests a game or a 3D application.
///
/// Every entry is a real system DLL that ships with Windows; the list is
/// short on purpose, because a long list would produce false positives and a
/// wrong default is worse than no default.
pub const GRAPHICS_IMPORTS: &[&str] = &[
    "d3d8.dll",
    "d3d9.dll",
    "d3d10.dll",
    "d3d11.dll",
    "d3d12.dll",
    "dxgi.dll",
    "ddraw.dll",
    "opengl32.dll",
    "vulkan-1.dll",
];

/// APIs that indicate a Steam-integrated program.
pub const STEAM_IMPORTS: &[&str] = &["steam_api.dll", "steam_api64.dll", "steamclient.dll"];

/// The parsed import table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PeImports {
    /// DLLs imported, in table order, deduplicated.
    pub dlls: Vec<ImportedDll>,
    /// Whether the optional header carried a data directory at all.
    pub has_data_directory: bool,
}

impl PeImports {
    /// The classification these imports imply.
    #[must_use]
    pub fn kind(&self) -> ProgramKind {
        if self.dlls.is_empty() {
            return ProgramKind::Unknown;
        }
        let names: Vec<String> = self.dlls.iter().map(ImportedDll::lower).collect();
        let is_graphical = names.iter().any(|n| GRAPHICS_IMPORTS.contains(&n.as_str()));
        let is_steam = names.iter().any(|n| STEAM_IMPORTS.contains(&n.as_str()));
        if is_graphical || is_steam {
            ProgramKind::Game
        } else {
            ProgramKind::DesktopApplication
        }
    }

    /// True when a 3D API is imported.
    #[must_use]
    pub fn imports_graphics(&self) -> bool {
        self.dlls
            .iter()
            .any(|d| GRAPHICS_IMPORTS.contains(&d.lower().as_str()))
    }

    /// True when a Steam API is imported.
    #[must_use]
    pub fn imports_steam(&self) -> bool {
        self.dlls
            .iter()
            .any(|d| STEAM_IMPORTS.contains(&d.lower().as_str()))
    }

    /// The DLL names, for a log line.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.dlls.iter().map(|d| d.name.clone()).collect()
    }
}

/// A section header, reduced to what RVA mapping needs.
#[derive(Debug, Clone, Copy)]
struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_pointer: u32,
    raw_size: u32,
}

impl Section {
    fn contains(&self, rva: u32) -> bool {
        rva >= self.virtual_address
            && rva
                < self
                    .virtual_address
                    .saturating_add(self.virtual_size.max(self.raw_size))
    }

    fn to_offset(self, rva: u32) -> Option<usize> {
        let delta = rva.checked_sub(self.virtual_address)?;
        // Past the raw data there is nothing in the file to read, even
        // though the virtual range extends further.
        if delta >= self.raw_size {
            return None;
        }
        usize::try_from(self.raw_pointer.checked_add(delta)?).ok()
    }
}

/// Everything `parse_imports` needs, worked out once.
struct Headers {
    optional_magic: u16,
    optional_offset: usize,
    section_offset: usize,
    sections: Vec<Section>,
}

impl Headers {
    fn parse(bytes: &[u8]) -> Option<Self> {
        let read_u16 = |at: usize| -> Option<u16> {
            let b = bytes.get(at..at + 2)?;
            Some(u16::from_le_bytes([b[0], b[1]]))
        };
        let read_u32 = |at: usize| -> Option<u32> {
            let b = bytes.get(at..at + 4)?;
            Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };

        if bytes.get(..2)? != b"MZ" {
            return None;
        }
        let lfanew = usize::try_from(read_u32(0x3C)?).ok()?;
        if bytes.get(lfanew..lfanew + 4)? != b"PE\0\0" {
            return None;
        }
        let section_count = usize::from(read_u16(lfanew + 6)?);
        let size_of_optional = usize::from(read_u16(lfanew + 20)?);
        let optional_offset = lfanew + 24;
        let optional_magic = read_u16(optional_offset)?;
        if optional_magic != 0x10b && optional_magic != 0x20b {
            return None;
        }
        let section_offset = optional_offset.checked_add(size_of_optional)?;

        // A count in the thousands is not a real PE; bounding it keeps a
        // corrupt header from becoming a long loop.
        let section_count = section_count.min(96);
        let mut sections = Vec::with_capacity(section_count);
        for i in 0..section_count {
            let base = section_offset + i * 40;
            let Some(virtual_size) = read_u32(base + 8) else {
                break;
            };
            let Some(virtual_address) = read_u32(base + 12) else {
                break;
            };
            let Some(raw_size) = read_u32(base + 16) else {
                break;
            };
            let Some(raw_pointer) = read_u32(base + 20) else {
                break;
            };
            sections.push(Section {
                virtual_address,
                virtual_size,
                raw_pointer,
                raw_size,
            });
        }
        Some(Self {
            optional_magic,
            optional_offset,
            section_offset,
            sections,
        })
    }

    fn rva_to_offset(&self, rva: u32) -> Option<usize> {
        self.sections.iter().find_map(|s| {
            if s.contains(rva) {
                s.to_offset(rva)
            } else {
                None
            }
        })
    }

    /// Offset of the data directory array within the optional header.
    ///
    /// PE32 has 96 bytes of standard and Windows-specific fields before the
    /// directories; PE32+ has 112, because the stack and heap fields are
    /// 64-bit. Getting this wrong by 16 bytes reads a completely unrelated
    /// directory, which is exactly the kind of bug that produces a plausible
    /// but wrong dependency list.
    fn data_directory_offset(&self) -> usize {
        self.optional_offset
            + if self.optional_magic == 0x20b {
                112
            } else {
                96
            }
    }
}

/// Read a NUL-terminated ASCII string at a file offset.
fn cstring_at(bytes: &[u8], offset: usize) -> Option<String> {
    let rest = bytes.get(offset..)?;
    let end = rest.iter().position(|b| *b == 0)?;
    // Some names use these bytes for non-ASCII; refusing is better than
    // inventing a name.
    let raw = rest.get(..end)?;
    if raw.iter().any(|b| !b.is_ascii()) {
        return None;
    }
    Some(String::from_utf8_lossy(raw).into_owned())
}

/// Parse the import table of a PE image.
///
/// Returns an empty table (never an error) when the imports cannot be read:
/// a launcher should fall back to a safer default, not refuse to run the
/// program. The caller distinguishes "no imports" from "unreadable" by
/// [`PeImports::has_data_directory`].
#[must_use]
pub fn parse_imports(bytes: &[u8]) -> PeImports {
    let read_u32 = |at: usize| -> Option<u32> {
        let b = bytes.get(at..at + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };

    let Some(headers) = Headers::parse(bytes) else {
        return PeImports::default();
    };
    let dir_offset = headers.data_directory_offset();
    // The directory array must lie inside the optional header, or the
    // header size lied and nothing after it can be trusted.
    let optional_end = headers.section_offset;
    if dir_offset + 8 > optional_end {
        return PeImports::default();
    }
    let Some(import_rva) = read_u32(dir_offset + 8) else {
        return PeImports::default();
    };
    let Some(import_size) = read_u32(dir_offset + 12) else {
        return PeImports::default();
    };
    if import_rva == 0 || import_size == 0 {
        // A PE with no imports at all is legal (a driver, or a fully static
        // image), so this is "unknown", not an error.
        return PeImports {
            dlls: Vec::new(),
            has_data_directory: true,
        };
    }
    let Some(mut cursor) = headers.rva_to_offset(import_rva) else {
        return PeImports {
            dlls: Vec::new(),
            has_data_directory: true,
        };
    };

    let mut dlls: Vec<ImportedDll> = Vec::new();
    for _ in 0..MAX_DESCRIPTORS {
        let Some(zeroth) = read_u32(cursor) else {
            break;
        };
        let Some(name_rva) = read_u32(cursor + 12) else {
            break;
        };
        let Some(first_thunk) = read_u32(cursor + 16) else {
            break;
        };
        if zeroth == 0 && name_rva == 0 && first_thunk == 0 {
            break;
        }
        if name_rva == 0 {
            break;
        }
        if let Some(name_offset) = headers.rva_to_offset(name_rva)
            && let Some(name) = cstring_at(bytes, name_offset)
            && !name.is_empty()
            && !dlls.iter().any(|d| d.name.eq_ignore_ascii_case(&name))
        {
            dlls.push(ImportedDll {
                name,
                function_count: count_thunks(bytes, &headers, zeroth.max(first_thunk)),
            });
        }
        cursor = cursor.saturating_add(20);
    }

    // Bound the work a corrupt table can cause.
    dlls.truncate(MAX_DESCRIPTORS);
    PeImports {
        dlls,
        has_data_directory: true,
    }
}

/// Count named imports in a thunk table.
///
/// The import lookup table is an array of 4- or 8-byte entries terminated by
/// a zero one; the top bit of an entry means "import by ordinal", which has
/// no name.
fn count_thunks(bytes: &[u8], headers: &Headers, thunk_rva: u32) -> Option<u32> {
    if thunk_rva == 0 {
        return None;
    }
    let start = headers.rva_to_offset(thunk_rva)?;
    let entry = if headers.optional_magic == 0x20b {
        8usize
    } else {
        4
    };
    let mut count = 0u32;
    for i in 0..MAX_DESCRIPTORS {
        let at = start.checked_add(i.checked_mul(entry)?)?;
        let value = if entry == 8 {
            let b = bytes.get(at..at + 8)?;
            u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        } else {
            let b = bytes.get(at..at + 4)?;
            u64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        if value == 0 {
            break;
        }
        count += 1;
    }
    Some(count)
}

/// The architecture a PE targets, from its COFF header.
///
/// Duplicated from the format probe so this module stands alone; the probe
/// reports more, this reports the one field a launcher needs.
#[must_use]
pub fn pe_arch(bytes: &[u8]) -> Arch {
    let read_u16 = |at: usize| -> Option<u16> {
        let b = bytes.get(at..at + 2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    };
    let read_u32 = |at: usize| -> Option<u32> {
        let b = bytes.get(at..at + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let Some(lfanew) = read_u32(0x3C).and_then(|v| usize::try_from(v).ok()) else {
        return Arch::Unknown;
    };
    match read_u16(lfanew + 4) {
        Some(machine) => Arch::from_pe_machine(machine),
        None => Arch::Unknown,
    }
}

#[cfg(test)]
mod tests;
