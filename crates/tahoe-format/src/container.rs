// SPDX-License-Identifier: MIT OR Apache-2.0
//! Container and packaging formats: things that *hold* executables rather
//! than being one.
//!
//! Everything here is inspection only. The whole point of this module is
//! that a `.dmg`, a `.pkg`, or a `.msi` is **data**: it gets described,
//! listed and mounted, never handed to `execve` (ADR-0004).
//!
//! Formats parsed here, all from primary specifications:
//!
//! - **UDIF/DMG** — the 512-byte `koly` trailer. Field offsets and the
//!   big-endian rule are taken from the format as implemented by 7-Zip
//!   (`CPP/7zip/Archive/DmgHandler.cpp`) and libdmg-hfsplus; there is no
//!   Apple-published struct.
//! - **XAR** (`.pkg`, `.xip`) — `xar!` header with a big-endian size.
//! - **OLE2 / CFBF** (`.msi`) — the 8-byte `D0 CF 11 E0 A1 B1 1A E1`
//!   signature, which is *the* reliable way to tell an MSI from a
//!   self-extracting PE stub with the same extension.
//! - **Property lists** — `bplist00` binary and XML. Only the shape is
//!   detected here; `tahoe-compat` owns plist *values*.

use tahoe_core::ObjectFormat;

/// UDIF `koly` trailer magic.
pub const KOLY_MAGIC: &[u8; 4] = b"koly";
/// The trailer is always exactly this long.
pub const KOLY_LEN: usize = 512;
/// XAR archive magic.
pub const XAR_MAGIC: &[u8; 4] = b"xar!";
/// OLE2 / Compound File Binary Format magic (`.msi`, `.doc`, `.xls`).
pub const OLE2_MAGIC: &[u8; 8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// Binary property list magic.
pub const BPLIST_MAGIC: &[u8; 8] = b"bplist00";

/// How a UDIF block is compressed.
///
/// The numeric values are the on-disk `BLKXRun::type` field. The
/// human-readable names (`UDZO`, `UDBZ`, `ULFO`, …) are the community
/// labels used by `hdiutil`; the mapping from those strings to these
/// numbers is **not** published by Apple and is not asserted here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DmgCompression {
    /// `0x00000000` / `0x00000002` — sparse zeros.
    Zero,
    /// `0x00000001` — stored verbatim.
    Raw,
    /// `0x80000004` — Apple Data Compression.
    Adc,
    /// `0x80000005` — zlib/deflate. The most common.
    Zlib,
    /// `0x80000006` — bzip2.
    Bzip2,
    /// `0x80000007` — LZFSE.
    Lzfse,
    /// `0x80000008` — LZMA/XZ.
    Lzma,
    /// A method we do not model.
    Unknown(u32),
}

impl DmgCompression {
    /// Decode a `BLKXRun::type` value.
    #[must_use]
    pub fn from_run_type(value: u32) -> Self {
        match value {
            0x0000_0000 | 0x0000_0002 => Self::Zero,
            0x0000_0001 => Self::Raw,
            0x8000_0004 => Self::Adc,
            0x8000_0005 => Self::Zlib,
            0x8000_0006 => Self::Bzip2,
            0x8000_0007 => Self::Lzfse,
            0x8000_0008 => Self::Lzma,
            0x7FFF_FFFE | 0xFFFF_FFFF => Self::Raw, // comment / terminator runs
            other => Self::Unknown(other),
        }
    }

    /// True when this method can be decompressed with widely available
    /// tools (zlib, bzip2, LZMA) or read directly.
    ///
    /// LZFSE is the notable exception: it needs Apple's `lzfse` or a
    /// library, which is why a caller should surface this rather than
    /// assume every DMG is readable.
    #[must_use]
    pub fn is_widely_supported(self) -> bool {
        matches!(
            self,
            Self::Zero | Self::Raw | Self::Zlib | Self::Bzip2 | Self::Lzma
        )
    }

    /// A short label for diagnostics.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::Raw => "raw",
            Self::Adc => "adc",
            Self::Zlib => "zlib",
            Self::Bzip2 => "bzip2",
            Self::Lzfse => "lzfse",
            Self::Lzma => "lzma",
            Self::Unknown(_) => "unknown",
        }
    }
}

/// The parsed 512-byte `koly` trailer of a UDIF disk image.
///
/// Every multi-byte field is big-endian; the image is *not* host-endian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmgTrailer {
    /// Format version, normally 4.
    pub version: u32,
    /// Size of this trailer, normally 512.
    pub header_size: u32,
    /// Trailer flags.
    pub flags: u32,
    /// Offset of the data fork within the file.
    pub data_fork_offset: u64,
    /// Length of the data fork.
    pub data_fork_length: u64,
    /// Offset of the resource fork, when present.
    pub resource_fork_offset: u64,
    /// Length of the resource fork.
    pub resource_fork_length: u64,
    /// Segment number for split images (1-based).
    pub segment_number: u32,
    /// Total segments in a split image.
    pub segment_count: u32,
    /// Offset of the embedded XML property list.
    pub xml_offset: u64,
    /// Length of the embedded XML property list.
    pub xml_length: u64,
    /// Offset of the embedded code-signature blob.
    pub blob_offset: u64,
    /// Length of the embedded code-signature blob.
    pub blob_length: u64,
    /// Image variant.
    pub image_variant: u32,
    /// Total sectors (512 bytes each) in the decoded image.
    pub sector_count: u64,
}

impl DmgTrailer {
    /// Size of the decoded (uncompressed) image in bytes.
    #[must_use]
    pub fn decoded_size(&self) -> u64 {
        self.sector_count.saturating_mul(512)
    }

    /// True when the file is one part of a split image.
    #[must_use]
    pub fn is_split_segment(&self) -> bool {
        self.segment_count > 1
    }

    /// True when an embedded XML property list is present, which is where
    /// the `blkx` block tables live.
    #[must_use]
    pub fn has_block_table(&self) -> bool {
        self.xml_length > 0
    }
}

/// Parse a 512-byte `koly` trailer.
///
/// Returns `None` when the slice is too short or does not start with
/// `koly`, so a caller can use this directly as a detector.
#[must_use]
pub fn parse_dmg_trailer(tail: &[u8]) -> Option<DmgTrailer> {
    if tail.len() < KOLY_LEN || &tail[..4] != KOLY_MAGIC {
        return None;
    }
    let u32_at =
        |o: usize| -> u32 { u32::from_be_bytes([tail[o], tail[o + 1], tail[o + 2], tail[o + 3]]) };
    let u64_at = |o: usize| -> u64 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&tail[o..o + 8]);
        u64::from_be_bytes(b)
    };
    Some(DmgTrailer {
        version: u32_at(0x004),
        header_size: u32_at(0x008),
        flags: u32_at(0x00C),
        data_fork_offset: u64_at(0x018),
        data_fork_length: u64_at(0x020),
        resource_fork_offset: u64_at(0x028),
        resource_fork_length: u64_at(0x030),
        segment_number: u32_at(0x038),
        segment_count: u32_at(0x03C),
        xml_offset: u64_at(0x0D8),
        xml_length: u64_at(0x0E0),
        blob_offset: u64_at(0x128),
        blob_length: u64_at(0x130),
        image_variant: u32_at(0x1E8),
        sector_count: u64_at(0x1EC),
    })
}

/// What kind of container the bytes represent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    /// UDIF disk image with a parsed trailer.
    Dmg,
    /// XAR archive (`.pkg`, `.xip`).
    Xar,
    /// OLE2 compound file (`.msi`, and also legacy Office documents).
    Ole2,
    /// XML property list.
    XmlPlist,
    /// Binary property list.
    BinaryPlist,
}

impl ContainerKind {
    /// The [`ObjectFormat`] this container reports as.
    #[must_use]
    pub fn object_format(self) -> ObjectFormat {
        match self {
            Self::Dmg => ObjectFormat::Dmg,
            Self::Xar => ObjectFormat::Xar,
            Self::Ole2 => ObjectFormat::Ole2,
            Self::XmlPlist | Self::BinaryPlist => ObjectFormat::Plist,
        }
    }
}

/// Identify a container from its leading bytes. No DMG trailer is looked
/// for here — see [`parse_dmg_trailer`] and `probe_file`.
#[must_use]
pub fn identify_container(head: &[u8]) -> Option<ContainerKind> {
    if head.len() >= XAR_MAGIC.len() && &head[..4] == XAR_MAGIC {
        return Some(ContainerKind::Xar);
    }
    if head.len() >= OLE2_MAGIC.len() && &head[..8] == OLE2_MAGIC {
        return Some(ContainerKind::Ole2);
    }
    if head.len() >= BPLIST_MAGIC.len() && &head[..8] == BPLIST_MAGIC {
        return Some(ContainerKind::BinaryPlist);
    }
    // An XML plist has no fixed magic; require the declaration so a random
    // XML file is not mistaken for one.
    if head.starts_with(b"<?xml") || head.starts_with(b"bplist") {
        let text = head.get(..head.len().min(512)).unwrap_or(head);
        if let Ok(text) = std::str::from_utf8(text) {
            let forked = text.contains("<plist") || text.starts_with("bplist");
            if forked {
                return Some(ContainerKind::XmlPlist);
            }
        }
    }
    None
}

/// Read the `BLKX` block-run table from a DMG's embedded XML plist.
///
/// Returns the distinct compression methods the image uses, in the order
/// they first appear. An empty result means the table was not reachable —
/// it is **not** a claim that the image is uncompressed.
///
/// The `blkx` resources are base64 blobs whose first field is the ASCII
/// signature `mish` followed by the run table; the compression type lives
/// in each 40-byte run record.
#[must_use]
pub fn scan_dmg_compression(xml: &[u8]) -> Vec<DmgCompression> {
    // The XML is a plist; extract base64 payloads between <data> tags.
    let Ok(text) = std::str::from_utf8(xml) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for chunk in text.split("<data>").skip(1) {
        let Some(end) = chunk.find("</data>") else {
            continue;
        };
        let b64: String = chunk[..end]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let Some(bytes) = base64_decode(&b64) else {
            continue;
        };
        if bytes.len() < 4 || &bytes[..4] != b"mish" {
            continue;
        }
        // BLKXTable header is 0xCC bytes, then u32 blocksRunCount, then
        // 40-byte runs: type, reserved, sectorStart, sectorCount,
        // compOffset, compLength.
        let count_offset = 0xCC;
        if bytes.len() < count_offset + 4 {
            continue;
        }
        let count = u32::from_be_bytes([
            bytes[count_offset],
            bytes[count_offset + 1],
            bytes[count_offset + 2],
            bytes[count_offset + 3],
        ]) as usize;
        for i in 0..count.min(4096) {
            let run = count_offset + 4 + i * 40;
            if bytes.len() < run + 4 {
                break;
            }
            let ty =
                u32::from_be_bytes([bytes[run], bytes[run + 1], bytes[run + 2], bytes[run + 3]]);
            let method = DmgCompression::from_run_type(ty);
            if !found.contains(&method) {
                found.push(method);
            }
        }
    }
    found
}

/// Minimal base64 decoder, tolerant of whitespace already stripped by the
/// caller. Standard alphabet with `=` padding; returns `None` on bad input.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u8> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            return None;
        }
        let pad = chunk.iter().filter(|c| **c == b'=').count();
        let mut buf = [0u8; 4];
        for (i, c) in chunk.iter().enumerate() {
            buf[i] = if *c == b'=' { 0 } else { value(*c)? };
        }
        out.push((buf[0] << 2) | (buf[1] >> 4));
        if pad < 2 {
            out.push((buf[1] << 4) | (buf[2] >> 2));
        }
        if pad < 1 {
            out.push((buf[2] << 6) | buf[3]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic `koly` trailer with the given fields.
    fn trailer(version: u32, sector_count: u64, xml_len: u64, segments: u32) -> Vec<u8> {
        let mut b = vec![0u8; KOLY_LEN];
        b[..4].copy_from_slice(KOLY_MAGIC);
        b[0x004..0x008].copy_from_slice(&version.to_be_bytes());
        b[0x008..0x00C].copy_from_slice(&512u32.to_be_bytes());
        b[0x018..0x020].copy_from_slice(&512u64.to_be_bytes());
        b[0x020..0x028].copy_from_slice(&1024u64.to_be_bytes());
        b[0x038..0x03C].copy_from_slice(&1u32.to_be_bytes());
        b[0x03C..0x040].copy_from_slice(&segments.to_be_bytes());
        b[0x0D8..0x0E0].copy_from_slice(&2048u64.to_be_bytes());
        b[0x0E0..0x0E8].copy_from_slice(&xml_len.to_be_bytes());
        b[0x128..0x130].copy_from_slice(&4096u64.to_be_bytes());
        b[0x130..0x138].copy_from_slice(&128u64.to_be_bytes());
        b[0x1EC..0x1F4].copy_from_slice(&sector_count.to_be_bytes());
        b
    }

    #[test]
    fn parses_a_koly_trailer() {
        let t = parse_dmg_trailer(&trailer(4, 2048, 4096, 1)).expect("trailer");
        assert_eq!(t.version, 4);
        assert_eq!(t.header_size, 512);
        assert_eq!(t.data_fork_offset, 512);
        assert_eq!(t.data_fork_length, 1024);
        assert_eq!(t.segment_number, 1);
        assert_eq!(t.segment_count, 1);
        assert_eq!(t.xml_offset, 2048);
        assert_eq!(t.xml_length, 4096);
        assert_eq!(t.blob_offset, 4096);
        assert_eq!(t.blob_length, 128);
        assert_eq!(t.sector_count, 2048);
        assert_eq!(t.decoded_size(), 2048 * 512);
        assert!(!t.is_split_segment());
        assert!(t.has_block_table());
    }

    #[test]
    fn koly_fields_are_big_endian() {
        // If we read little-endian, 0x0000_0800 would come back as 0x0008_0000.
        let t = parse_dmg_trailer(&trailer(4, 0x0000_0800, 0, 1)).expect("trailer");
        assert_eq!(t.sector_count, 0x800);
    }

    #[test]
    fn rejects_a_short_tail() {
        assert!(parse_dmg_trailer(&trailer(4, 1, 0, 1)[..256]).is_none());
        assert!(parse_dmg_trailer(&[]).is_none());
    }

    #[test]
    fn rejects_a_bad_magic() {
        let mut t = trailer(4, 1, 0, 1);
        t[0] = b'X';
        assert!(parse_dmg_trailer(&t).is_none());
    }

    #[test]
    fn detects_a_split_image() {
        let t = parse_dmg_trailer(&trailer(4, 100, 10, 3)).expect("trailer");
        assert!(t.is_split_segment());
        assert_eq!(t.segment_count, 3);
    }

    #[test]
    fn reports_no_block_table_without_xml() {
        let t = parse_dmg_trailer(&trailer(4, 100, 0, 1)).expect("trailer");
        assert!(!t.has_block_table());
    }

    #[test]
    fn compression_types_decode_and_classify() {
        assert_eq!(
            DmgCompression::from_run_type(0x0000_0000),
            DmgCompression::Zero
        );
        assert_eq!(
            DmgCompression::from_run_type(0x0000_0001),
            DmgCompression::Raw
        );
        assert_eq!(
            DmgCompression::from_run_type(0x8000_0004),
            DmgCompression::Adc
        );
        assert_eq!(
            DmgCompression::from_run_type(0x8000_0005),
            DmgCompression::Zlib
        );
        assert_eq!(
            DmgCompression::from_run_type(0x8000_0006),
            DmgCompression::Bzip2
        );
        assert_eq!(
            DmgCompression::from_run_type(0x8000_0007),
            DmgCompression::Lzfse
        );
        assert_eq!(
            DmgCompression::from_run_type(0x8000_0008),
            DmgCompression::Lzma
        );
        assert_eq!(
            DmgCompression::from_run_type(0xDEAD_BEEF),
            DmgCompression::Unknown(0xDEAD_BEEF)
        );

        assert!(DmgCompression::Zlib.is_widely_supported());
        assert!(DmgCompression::Lzma.is_widely_supported());
        assert!(
            !DmgCompression::Lzfse.is_widely_supported(),
            "LZFSE needs Apple's implementation; callers must not assume it"
        );
        assert_eq!(DmgCompression::Unknown(1).label(), "unknown");
    }

    #[test]
    fn base64_round_trips_known_vectors() {
        assert_eq!(base64_decode("aGlzaA==").unwrap(), b"hish");
        assert_eq!(base64_decode("bWlzaA==").unwrap(), b"mish");
        assert_eq!(base64_decode("AAAA").unwrap(), vec![0, 0, 0]);
        assert_eq!(base64_decode("").unwrap(), Vec::<u8>::new());
        assert!(base64_decode("!!!!").is_none());
    }

    /// Encode with the standard alphabet, for building fixtures.
    fn b64(bytes: &[u8]) -> String {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in bytes.chunks(3) {
            let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            out.push(A[(n >> 18) as usize & 63] as char);
            out.push(A[(n >> 12) as usize & 63] as char);
            out.push(if c.len() > 1 {
                A[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if c.len() > 2 {
                A[n as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    /// Build one `blkx` table with the given run types.
    fn blkx(run_types: &[u32]) -> Vec<u8> {
        let mut t = vec![0u8; 0xCC + 4];
        t[..4].copy_from_slice(b"mish");
        t[4..8].copy_from_slice(&1u32.to_be_bytes());
        t[0xCC..0xD0].copy_from_slice(&(run_types.len() as u32).to_be_bytes());
        for ty in run_types {
            let mut run = [0u8; 40];
            run[..4].copy_from_slice(&ty.to_be_bytes());
            t.extend_from_slice(&run);
        }
        t
    }

    #[test]
    fn scans_compression_from_an_xml_block_table() {
        let xml = format!(
            "<?xml version=\"1.0\"?><plist><dict><key>blkx</key><array><dict><key>Data</key><data>{}</data></dict></array></dict></plist>",
            b64(&blkx(&[0x0000_0001, 0x8000_0005, 0x8000_0005]))
        );
        let found = scan_dmg_compression(xml.as_bytes());
        assert_eq!(
            found,
            vec![DmgCompression::Raw, DmgCompression::Zlib],
            "duplicates collapse"
        );
    }

    #[test]
    fn scans_multiple_blkx_entries() {
        let first = b64(&blkx(&[0x0000_0000]));
        let second = b64(&blkx(&[0x8000_0007]));
        let xml = format!("<plist><data>{first}</data><data>{second}</data></plist>");
        let found = scan_dmg_compression(xml.as_bytes());
        assert_eq!(found, vec![DmgCompression::Zero, DmgCompression::Lzfse]);
    }

    #[test]
    fn ignores_data_blobs_that_are_not_block_tables() {
        let xml = format!(
            "<plist><data>{}</data></plist>",
            b64(b"not a block table at all")
        );
        assert!(scan_dmg_compression(xml.as_bytes()).is_empty());
    }

    #[test]
    fn survives_malformed_xml_and_base64() {
        assert!(scan_dmg_compression(b"<data>!!!!</data>").is_empty());
        assert!(scan_dmg_compression(b"<data>unterminated").is_empty());
        assert!(scan_dmg_compression(&[0xff, 0xfe]).is_empty());
        assert!(scan_dmg_compression(b"").is_empty());
    }

    #[test]
    fn a_huge_run_count_does_not_hang() {
        // A hostile or corrupt table claiming 4 billion runs must be capped.
        let mut t = vec![0u8; 0xCC];
        t[..4].copy_from_slice(b"mish");
        t.extend_from_slice(&u32::MAX.to_be_bytes());
        let xml = format!("<plist><data>{}</data></plist>", b64(&t));
        let found = scan_dmg_compression(xml.as_bytes());
        assert!(found.is_empty(), "no runs beyond the buffer should be read");
    }

    #[test]
    fn identifies_xar() {
        assert_eq!(identify_container(b"xar!...."), Some(ContainerKind::Xar));
        assert_eq!(ContainerKind::Xar.object_format(), ObjectFormat::Xar);
    }

    #[test]
    fn identifies_ole2() {
        let mut head = OLE2_MAGIC.to_vec();
        head.extend_from_slice(&[0u8; 16]);
        assert_eq!(identify_container(&head), Some(ContainerKind::Ole2));
        assert_eq!(ContainerKind::Ole2.object_format(), ObjectFormat::Ole2);
    }

    #[test]
    fn identifies_binary_plist() {
        assert_eq!(
            identify_container(b"bplist00\xd1\x01\x02"),
            Some(ContainerKind::BinaryPlist)
        );
    }

    #[test]
    fn identifies_xml_plist_but_not_random_xml() {
        assert_eq!(
            identify_container(b"<?xml version=\"1.0\"?>\n<plist version=\"1.0\">"),
            Some(ContainerKind::XmlPlist)
        );
        assert_eq!(identify_container(b"<?xml version=\"1.0\"?>\n<svg>"), None);
    }

    #[test]
    fn unknown_bytes_are_not_a_container() {
        assert_eq!(identify_container(b"\x7fELF"), None);
        assert_eq!(identify_container(b""), None);
        assert_eq!(identify_container(&[0u8; 512]), None);
    }
}
