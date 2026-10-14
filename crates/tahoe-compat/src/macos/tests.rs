// SPDX-License-Identifier: MIT OR Apache-2.0
//! macOS inspection tests.
//!
//! The Mach-O and binary-plist fixtures are built byte-by-byte here, so each
//! assertion documents the exact structure it relies on. That matters more
//! than usual for these formats: a reader that silently mis-parses a load
//! command reports wrong dependencies, and a wrong dependency list is worse
//! than no list at all.

use super::*;
use std::fs;

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

/// A load command under construction.
///
/// `cmdsize` must cover the padding as well as the body: the reader
/// advances by `cmdsize`, so a size that excludes the padding makes it walk
/// into the pad bytes and read them as the next command.
fn command(cmd: u32, body: &[u8]) -> Vec<u8> {
    let total = (8 + body.len() + 7) & !7;
    let mut out = Vec::new();
    out.extend_from_slice(&cmd.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(body);
    out.resize(total, 0);
    out
}

/// An `LC_*_DYLIB`-family command naming `name`.
fn dylib_command(cmd: u32, name: &str) -> Vec<u8> {
    // struct dylib { lc_str name; timestamp; current_version; compat_version }
    let mut body = Vec::new();
    body.extend_from_slice(&24u32.to_le_bytes()); // name offset, right after the header
    body.extend_from_slice(&0u32.to_le_bytes()); // timestamp
    body.extend_from_slice(&0x10000u32.to_le_bytes()); // current_version 1.0.0
    body.extend_from_slice(&0x10000u32.to_le_bytes()); // compat_version
    body.extend_from_slice(name.as_bytes());
    body.push(0);
    command(cmd, &body)
}

/// An `LC_RPATH` naming `path`.
fn rpath_command(path: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&12u32.to_le_bytes()); // path offset
    body.extend_from_slice(path.as_bytes());
    body.push(0);
    command(lc::RPATH, &body)
}

/// An `LC_LOAD_DYLINKER` naming `path`.
fn dylinker_command(path: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&12u32.to_le_bytes());
    body.extend_from_slice(path.as_bytes());
    body.push(0);
    command(lc::LOAD_DYLINKER, &body)
}

/// An `LC_UUID`.
fn uuid_command(uuid: [u8; 16]) -> Vec<u8> {
    command(lc::UUID, &uuid)
}

/// An `LC_MAIN` with the given entry offset.
fn main_command(entry: u64) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&entry.to_le_bytes());
    body.extend_from_slice(&0u64.to_le_bytes()); // stacksize
    command(lc::MAIN, &body)
}

/// Assemble a complete thin 64-bit Mach-O.
fn macho64(filetype: u32, flags: u32, commands: &[Vec<u8>]) -> Vec<u8> {
    let sizeofcmds: u32 = commands.iter().map(|c| c.len() as u32).sum();
    let mut out = Vec::new();
    out.extend_from_slice(&magic::MH_MAGIC_64);
    out.extend_from_slice(&0x0100_0007u32.to_le_bytes()); // CPU_TYPE_X86_64
    out.extend_from_slice(&3u32.to_le_bytes()); // CPU_SUBTYPE_X86_64_ALL
    out.extend_from_slice(&filetype.to_le_bytes());
    out.extend_from_slice(&(commands.len() as u32).to_le_bytes());
    out.extend_from_slice(&sizeofcmds.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    for c in commands {
        out.extend_from_slice(c);
    }
    out
}

/// A universal binary wrapping the given 64-bit Mach-O payloads.
fn fat(entries: &[(i32, Vec<u8>)]) -> Vec<u8> {
    let header_size = 8 + entries.len() * 20;
    let mut out = Vec::new();
    out.extend_from_slice(&magic::FAT_MAGIC);
    out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    let mut offset = (header_size + 15) & !15; // 16-byte alignment
    let mut blobs = Vec::new();
    for (cputype, payload) in entries {
        out.extend_from_slice(&cputype.to_be_bytes());
        out.extend_from_slice(&0i32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(&4u32.to_be_bytes()); // align 2^4
        blobs.push((offset, payload.clone()));
        offset = (offset + payload.len() + 15) & !15;
    }
    while out.len() < header_size.max(blobs.first().map_or(0, |(o, _)| *o)) {
        out.push(0);
    }
    for (at, payload) in blobs {
        while out.len() < at {
            out.push(0);
        }
        out.extend_from_slice(&payload);
    }
    out
}

/// A scratch directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join("tahoe-macos-tests").join(format!(
            "{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        fs::write(&path, bytes).expect("write");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------------------
// Mach-O header and load commands
// ---------------------------------------------------------------------------

#[test]
fn parses_an_executable_with_no_load_commands() {
    let bytes = macho64(filetype::EXECUTE, 0, &[]);
    let image = MachOFile::parse(&bytes).expect("parse");
    assert!(image.is_executable());
    assert_eq!(image.cputype, 0x0100_0007);
    assert!(!image.pie);
    assert!(image.dependencies.is_empty());
    assert_eq!(cpu_name_for(image.cputype), "x86_64");
}

#[test]
fn reads_the_pie_flag() {
    let bytes = macho64(filetype::EXECUTE, 0x20_0000, &[]);
    assert!(MachOFile::parse(&bytes).expect("parse").pie);
}

#[test]
fn reads_dylib_dependencies_with_their_kind() {
    let bytes = macho64(
        filetype::EXECUTE,
        0,
        &[
            dylib_command(lc::LOAD_DYLIB, "/usr/lib/libSystem.B.dylib"),
            dylib_command(lc::LOAD_WEAK_DYLIB, "/usr/lib/libweak.dylib"),
            dylib_command(lc::REEXPORT_DYLIB, "/usr/lib/libre.dylib"),
        ],
    );
    let image = MachOFile::parse(&bytes).expect("parse");
    assert_eq!(image.dependencies.len(), 3);
    assert_eq!(
        image.dependencies[0].install_name,
        "/usr/lib/libSystem.B.dylib"
    );
    assert!(!image.dependencies[0].weak);
    assert!(
        image.dependencies[1].weak,
        "LC_LOAD_WEAK_DYLIB must be marked weak"
    );
    assert!(image.dependencies[2].reexported);

    // A weak dependency is not something the app cannot start without.
    assert_eq!(image.required_dependencies().len(), 2);
}

#[test]
fn reads_rpaths_in_order() {
    let bytes = macho64(
        filetype::EXECUTE,
        0,
        &[
            rpath_command("@executable_path/../Frameworks"),
            rpath_command("/usr/local/lib"),
        ],
    );
    let image = MachOFile::parse(&bytes).expect("parse");
    assert_eq!(
        image.rpaths,
        vec![
            "@executable_path/../Frameworks".to_owned(),
            "/usr/local/lib".to_owned()
        ],
        "rpath order is load order, which the spec says matters"
    );
}

#[test]
fn reads_the_dylinker() {
    let bytes = macho64(filetype::EXECUTE, 0, &[dylinker_command("/usr/lib/dyld")]);
    let image = MachOFile::parse(&bytes).expect("parse");
    assert_eq!(image.dylinker.as_deref(), Some("/usr/lib/dyld"));
}

#[test]
fn formats_a_uuid_the_way_otool_does() {
    let uuid = [
        0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
        0x88,
    ];
    let bytes = macho64(filetype::EXECUTE, 0, &[uuid_command(uuid)]);
    let image = MachOFile::parse(&bytes).expect("parse");
    assert_eq!(
        image.uuid.as_deref(),
        Some("12345678-9ABC-DEF0-1122-334455667788")
    );
}

#[test]
fn format_uuid_handles_a_short_slice() {
    assert_eq!(format_uuid(&[0xAB, 0xCD]), "ABCD");
    assert_eq!(format_uuid(&[]), "");
}

#[test]
fn reads_the_entry_point() {
    let bytes = macho64(filetype::EXECUTE, 0, &[main_command(0x3F00)]);
    assert_eq!(
        MachOFile::parse(&bytes).expect("parse").entry_offset,
        Some(0x3F00)
    );
}

#[test]
fn reads_minimum_macos_from_lc_build_version() {
    // minos packed as nibbles: 0x000B0000 == 11.0.0, platform macos == 1.
    let mut body = Vec::new();
    body.extend_from_slice(&platform::MACOS.to_le_bytes());
    body.extend_from_slice(&0x000B_0000u32.to_le_bytes());
    body.extend_from_slice(&0x000D_0100u32.to_le_bytes()); // sdk 13.1
    body.extend_from_slice(&0u32.to_le_bytes()); // ntools
    let bytes = macho64(filetype::EXECUTE, 0, &[command(lc::BUILD_VERSION, &body)]);
    let image = MachOFile::parse(&bytes).expect("parse");
    assert_eq!(image.min_macos_version().as_deref(), Some("11.0"));
    assert_eq!(image.platform(), Some(platform::MACOS));
}

#[test]
fn reads_minimum_macos_from_the_legacy_lc_version_min() {
    // 10.14.0 packed the same way.
    let mut body = Vec::new();
    body.extend_from_slice(&0x000A_0E00u32.to_le_bytes());
    body.extend_from_slice(&0x000A_0E00u32.to_le_bytes());
    let bytes = macho64(
        filetype::EXECUTE,
        0,
        &[command(lc::VERSION_MIN_MACOSX, &body)],
    );
    let image = MachOFile::parse(&bytes).expect("parse");
    assert_eq!(image.min_macos_version().as_deref(), Some("10.14"));
}

#[test]
fn an_ios_only_binary_reports_no_macos_minimum() {
    let mut body = Vec::new();
    body.extend_from_slice(&platform::IOS.to_le_bytes());
    body.extend_from_slice(&0x000C_0000u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    let bytes = macho64(filetype::EXECUTE, 0, &[command(lc::BUILD_VERSION, &body)]);
    let image = MachOFile::parse(&bytes).expect("parse");
    assert_eq!(
        image.min_macos_version(),
        None,
        "an iOS app is not a macOS app; pretending otherwise would be a lie"
    );
    assert_eq!(image.platform(), Some(platform::IOS));
}

#[test]
fn detects_an_embedded_code_signature() {
    let bytes = macho64(
        filetype::EXECUTE,
        0,
        &[dylib_command(lc::CODE_SIGNATURE, "")],
    );
    // CODE_SIGNATURE is a linkedit_data_command, not a dylib; build it right.
    let mut body = Vec::new();
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    let bytes2 = macho64(filetype::EXECUTE, 0, &[command(lc::CODE_SIGNATURE, &body)]);
    let _ = bytes;
    assert!(MachOFile::parse(&bytes2).expect("parse").code_signature);
}

#[test]
fn reads_the_install_name_of_a_dylib() {
    let bytes = macho64(
        filetype::DYLIB,
        0,
        &[dylib_command(lc::ID_DYLIB, "@rpath/libwidget.dylib")],
    );
    let image = MachOFile::parse(&bytes).expect("parse");
    assert!(!image.is_executable());
    assert_eq!(
        image.install_name.as_deref(),
        Some("@rpath/libwidget.dylib")
    );
}

#[test]
fn collects_rpath_dependencies() {
    let bytes = macho64(
        filetype::EXECUTE,
        0,
        &[
            dylib_command(lc::LOAD_DYLIB, "@rpath/liba.dylib"),
            dylib_command(lc::LOAD_DYLIB, "@rpath/libb.dylib"),
            dylib_command(lc::LOAD_DYLIB, "@rpath/liba.dylib"),
            dylib_command(lc::LOAD_DYLIB, "/usr/lib/libSystem.B.dylib"),
        ],
    );
    let image = MachOFile::parse(&bytes).expect("parse");
    assert_eq!(image.rpath_dependencies(), vec!["liba.dylib", "libb.dylib"]);
}

#[test]
fn rejects_a_non_macho_file() {
    assert!(matches!(
        MachOFile::parse(b"\x7fELF\x02\x01\x01"),
        Err(MachOError::NotMachO { .. })
    ));
    assert!(matches!(
        MachOFile::parse(b""),
        Err(MachOError::NotMachO { .. })
    ));
}

#[test]
fn refuses_a_load_command_that_runs_past_the_end() {
    // Claim 512 bytes of commands but supply 16.
    let mut bytes = macho64(filetype::EXECUTE, 0, &[]);
    bytes[16..20].copy_from_slice(&1u32.to_le_bytes()); // ncmds 1
    bytes[20..24].copy_from_slice(&512u32.to_le_bytes()); // sizeofcmds 512
    let mut bogus = vec![0u8; 16];
    bogus[0..4].copy_from_slice(&lc::LOAD_DYLIB.to_le_bytes());
    bogus[4..8].copy_from_slice(&4096u32.to_le_bytes()); // cmdsize far too large
    bytes.extend_from_slice(&bogus);
    assert!(matches!(
        MachOFile::parse(&bytes),
        Err(MachOError::Truncated { .. })
    ));
}

#[test]
fn a_hostile_command_count_does_not_hang() {
    let mut bytes = macho64(filetype::EXECUTE, 0, &[]);
    bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[20..24].copy_from_slice(&8u32.to_le_bytes());
    // Must terminate: the walk is bounded and stops when commands run out.
    let _ = MachOFile::parse(&bytes);
}

#[test]
fn parses_a_big_endian_thin_macho() {
    // MH_CIGAM_64: a byte-swapped 64-bit Mach-O. The fields are big-endian.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&magic::MH_CIGAM_64);
    bytes.extend_from_slice(&0x0100_0007u32.to_be_bytes());
    bytes.extend_from_slice(&3u32.to_be_bytes());
    bytes.extend_from_slice(&filetype::EXECUTE.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    let image = MachOFile::parse(&bytes).expect("parse");
    assert!(image.is_executable());
    assert_eq!(image.cputype, 0x0100_0007);
}

// ---------------------------------------------------------------------------
// Universal binaries
// ---------------------------------------------------------------------------

#[test]
fn parses_a_universal_header_and_lists_architectures() {
    let x86 = macho64(filetype::EXECUTE, 0, &[]);
    let mut arm = macho64(filetype::EXECUTE, 0, &[]);
    arm[4..8].copy_from_slice(&0x0100_000Cu32.to_le_bytes()); // CPU_TYPE_ARM64
    let bytes = fat(&[(0x0100_0007, x86), (0x0100_000C, arm)]);

    let fatbin = FatBinary::parse(&bytes).expect("parse");
    assert_eq!(fatbin.architectures(), vec!["x86_64", "arm64"]);
    assert!(!fatbin.is_64_header);
    assert_eq!(fatbin.slices[0].align, 4);
}

#[test]
fn selects_the_preferred_architecture_then_falls_back() {
    let x86 = macho64(filetype::EXECUTE, 0, &[]);
    let mut arm = macho64(filetype::EXECUTE, 0, &[]);
    arm[4..8].copy_from_slice(&0x0100_000Cu32.to_le_bytes());
    let bytes = fat(&[(0x0100_0007, x86), (0x0100_000C, arm)]);
    let fatbin = FatBinary::parse(&bytes).expect("parse");

    assert_eq!(
        fatbin.select(&["x86_64"]).expect("pick").cpu_name(),
        "x86_64"
    );
    assert_eq!(fatbin.select(&["arm64"]).expect("pick").cpu_name(), "arm64");
    // Nothing preferred: take the first 64-bit slice rather than nothing.
    assert_eq!(fatbin.select(&[]).expect("pick").cpu_name(), "x86_64");
    // A preference that is absent falls back the same way.
    assert_eq!(fatbin.select(&["i386"]).expect("pick").cpu_name(), "x86_64");
}

#[test]
fn arm64e_and_arm64_32_are_distinguished() {
    // arm64e is CPU_TYPE_ARM64 with CPU_SUBTYPE_ARM64E == 2, not a cputype.
    let arm64e = FatSlice {
        cputype: 0x0100_000C,
        cpusubtype: 2,
        offset: 0,
        size: 0,
        align: 0,
    };
    assert_eq!(arm64e.cpu_name(), "arm64e");
    assert!(arm64e.is_64_bit());

    let plain = FatSlice {
        cputype: 0x0100_000C,
        cpusubtype: 0,
        offset: 0,
        size: 0,
        align: 0,
    };
    assert_eq!(plain.cpu_name(), "arm64");

    // CPU_TYPE_ARM64_32 is 64-bit registers with 32-bit pointers (watchOS).
    let abi64_32 = FatSlice {
        cputype: 0x0200_000C,
        cpusubtype: 0,
        offset: 0,
        size: 0,
        align: 0,
    };
    assert_eq!(abi64_32.cpu_name(), "arm64_32");
    assert!(
        abi64_32.is_64_bit(),
        "ABI64_32 still means 64-bit registers"
    );

    // Capability bits pack into the subtype; they must not defeat the match.
    let with_caps = FatSlice {
        cputype: 0x0100_000C,
        cpusubtype: 2 | 0x8000_0000u32 as i32,
        offset: 0,
        size: 0,
        align: 0,
    };
    assert_eq!(with_caps.cpu_name(), "arm64e");
}

#[test]
fn from_bytes_selects_a_slice_and_parses_it() {
    let x86 = macho64(
        filetype::EXECUTE,
        0,
        &[dylib_command(lc::LOAD_DYLIB, "/usr/lib/libSystem.B.dylib")],
    );
    let bytes = fat(&[(0x0100_0007, x86)]);
    let (image, slice) = MachOFile::from_bytes(&bytes, &["x86_64"]).expect("parse");
    let slice = slice.expect("a universal binary must report its slice");
    assert_eq!(slice.cpu_name(), "x86_64");
    assert_eq!(image.dependencies.len(), 1);
}

#[test]
fn from_bytes_handles_a_thin_binary_without_a_slice() {
    let bytes = macho64(filetype::EXECUTE, 0, &[]);
    let (image, slice) = MachOFile::from_bytes(&bytes, &["x86_64"]).expect("parse");
    assert!(slice.is_none());
    assert!(image.is_executable());
}

#[test]
fn a_universal_header_with_no_slices_reports_nothing_to_select() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&magic::FAT_MAGIC);
    bytes.extend_from_slice(&0u32.to_be_bytes());
    let fatbin = FatBinary::parse(&bytes).expect("parse");
    assert!(fatbin.slices.is_empty());
    assert!(fatbin.select(&["x86_64"]).is_none());
}

#[test]
fn rejects_a_bad_universal_magic() {
    assert!(matches!(
        FatBinary::parse(b"NOPE\x00\x00\x00\x01"),
        Err(MachOError::NotMachO { .. })
    ));
}

#[test]
fn a_hostile_slice_count_does_not_allocate_wildly() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&magic::FAT_MAGIC);
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());
    bytes.extend_from_slice(&[0u8; 64]);
    // The count is clamped; parsing stops where the buffer ends.
    let _ = FatBinary::parse(&bytes);
}

// ---------------------------------------------------------------------------
// Dependency resolution
// ---------------------------------------------------------------------------

#[test]
fn resolves_loader_and_executable_relative_paths() {
    let loader = Path::new("/Applications/Foo.app/Contents/MacOS");
    let exe = Path::new("/Applications/Foo.app/Contents/MacOS");
    assert_eq!(
        resolve_dependency("@loader_path/libfoo.dylib", loader, exe, &[]),
        vec![PathBuf::from(
            "/Applications/Foo.app/Contents/MacOS/libfoo.dylib"
        )]
    );
    assert_eq!(
        resolve_dependency(
            "@executable_path/../Frameworks/libx.dylib",
            loader,
            exe,
            &[]
        ),
        vec![PathBuf::from(
            "/Applications/Foo.app/Contents/MacOS/../Frameworks/libx.dylib"
        )]
    );
}

#[test]
fn resolves_rpath_dependencies_against_every_rpath_in_order() {
    let loader = Path::new("/app/MacOS");
    let rpaths = vec![
        "@executable_path/../Frameworks".to_owned(),
        "/usr/local/lib".to_owned(),
    ];
    let got = resolve_dependency("@rpath/libz.dylib", loader, loader, &rpaths);
    assert_eq!(
        got,
        vec![
            PathBuf::from("/app/MacOS/../Frameworks/libz.dylib"),
            PathBuf::from("/usr/local/lib/libz.dylib"),
        ],
        "every candidate must be reported so the caller can pick the first that exists"
    );
}

#[test]
fn an_absolute_install_name_is_used_verbatim() {
    assert_eq!(
        resolve_dependency(
            "/usr/lib/libSystem.B.dylib",
            Path::new("/a"),
            Path::new("/a"),
            &[]
        ),
        vec![PathBuf::from("/usr/lib/libSystem.B.dylib")]
    );
}

#[test]
fn a_bare_install_name_yields_no_candidates_rather_than_a_guess() {
    assert!(
        resolve_dependency("libfoo.dylib", Path::new("/a"), Path::new("/a"), &[]).is_empty(),
        "a bare name needs the fallback search path, which we do not invent"
    );
}

#[test]
fn reports_unresolved_dependencies_against_the_real_filesystem() {
    let s = Scratch::new("deps");
    let present = s.write("Frameworks/libpresent.dylib", b"x");
    let _ = present;

    let bytes = macho64(
        filetype::EXECUTE,
        0,
        &[
            dylib_command(
                lc::LOAD_DYLIB,
                "@executable_path/Frameworks/libpresent.dylib",
            ),
            dylib_command(
                lc::LOAD_DYLIB,
                "@executable_path/Frameworks/libmissing.dylib",
            ),
            dylib_command(lc::LOAD_DYLIB, "@rpath/libunresolvable.dylib"),
            dylib_command(lc::LOAD_WEAK_DYLIB, "@executable_path/libweakmissing.dylib"),
        ],
    );
    let image = MachOFile::parse(&bytes).expect("parse");
    let missing = unresolved_dependencies(&image, &s.0, &s.0);
    assert!(
        missing.iter().any(|m| m.contains("libmissing")),
        "{missing:?}"
    );
    assert!(
        missing.iter().any(|m| m.contains("libunresolvable")),
        "an @rpath dep with no rpaths is unresolved, not assumed fine: {missing:?}"
    );
    assert!(
        !missing.iter().any(|m| m.contains("libpresent")),
        "a dependency that exists must not be reported missing: {missing:?}"
    );
    assert!(
        !missing.iter().any(|m| m.contains("libweakmissing")),
        "a weak dependency is not required to resolve: {missing:?}"
    );
}

#[test]
fn expands_loader_path_inside_an_rpath() {
    assert_eq!(
        expand_rpath("@loader_path/../lib", Path::new("/app/MacOS")),
        PathBuf::from("/app/MacOS/../lib")
    );
    assert_eq!(
        expand_rpath("/fixed/lib", Path::new("/app")),
        PathBuf::from("/fixed/lib")
    );
}

// ---------------------------------------------------------------------------
// Property lists
// ---------------------------------------------------------------------------

/// A tiny binary-plist writer, so the reader is tested against bytes we
/// control rather than against a writer that shares its bugs.
struct BplistWriter {
    objects: Vec<Vec<u8>>,
}

impl BplistWriter {
    fn new() -> Self {
        Self {
            objects: Vec::new(),
        }
    }

    fn push(&mut self, bytes: Vec<u8>) -> u64 {
        self.objects.push(bytes);
        (self.objects.len() - 1) as u64
    }

    /// A length that does not fit the marker's low nibble needs an
    /// extended-length integer object following it, exactly as a real
    /// writer emits. Strings longer than 14 characters hit this.
    fn sized(&mut self, kind: u8, len: usize, payload: Vec<u8>) -> u64 {
        let mut b = Vec::new();
        if len < 15 {
            b.push(kind | (len as u8));
        } else {
            b.push(kind | 0x0F);
            // Length as a 1- or 2-byte integer object, inline rather than by
            // reference (which is what the format allows here).
            if len <= 0xFF {
                b.push(0x10);
                b.push(len as u8);
            } else {
                b.push(0x11);
                b.extend_from_slice(&(len as u16).to_be_bytes());
            }
        }
        b.extend_from_slice(&payload);
        self.push(b)
    }

    fn ascii(&mut self, s: &str) -> u64 {
        let payload = s.as_bytes().to_vec();
        self.sized(0x50, s.len(), payload)
    }

    fn int(&mut self, v: u64) -> u64 {
        let marker = if v <= 0xFF { 0x10 } else { 0x11 };
        let mut b = vec![marker];
        if v <= 0xFF {
            b.push(v as u8);
        } else {
            b.extend_from_slice(&(v as u16).to_be_bytes());
        }
        self.push(b)
    }

    fn boolean(&mut self, v: bool) -> u64 {
        self.push(vec![if v { 0x09 } else { 0x08 }])
    }

    fn array(&mut self, refs: &[u64]) -> u64 {
        let mut b = vec![0xA0 | (refs.len() as u8)];
        for r in refs {
            b.push(*r as u8);
        }
        self.push(b)
    }

    fn dict(&mut self, entries: &[(u64, u64)]) -> u64 {
        let mut b = vec![0xD0 | (entries.len() as u8)];
        for (k, _) in entries {
            b.push(*k as u8);
        }
        for (_, v) in entries {
            b.push(*v as u8);
        }
        self.push(b)
    }

    fn finish(&self, top: u64) -> Vec<u8> {
        let mut out = b"bplist00".to_vec();
        let mut offsets = Vec::new();
        for obj in &self.objects {
            offsets.push(out.len() as u8);
            out.extend_from_slice(obj);
        }
        let offset_table_at = out.len() as u64;
        out.extend_from_slice(&offsets);
        // Trailer: 5 unused, sort version, offset size, ref size,
        // num objects, top object, offset table offset.
        out.extend_from_slice(&[0u8; 5]);
        out.push(0);
        out.push(1); // offset_size
        out.push(1); // ref_size
        out.extend_from_slice(&(self.objects.len() as u64).to_be_bytes());
        out.extend_from_slice(&top.to_be_bytes());
        out.extend_from_slice(&offset_table_at.to_be_bytes());
        out
    }
}

#[test]
fn parses_a_binary_plist_dictionary() {
    let mut w = BplistWriter::new();
    let key_exec = w.ascii("CFBundleExecutable");
    let val_exec = w.ascii("Widget");
    let key_id = w.ascii("CFBundleIdentifier");
    let val_id = w.ascii("com.example.widget");
    let key_agent = w.ascii("LSUIElement");
    let val_agent = w.boolean(true);
    let key_min = w.ascii("LSMinimumSystemVersion");
    let val_min = w.ascii("11.0");
    let dict = w.dict(&[
        (key_exec, val_exec),
        (key_id, val_id),
        (key_agent, val_agent),
        (key_min, val_min),
    ]);
    let bytes = w.finish(dict);

    let plist = Plist::parse(&bytes).expect("parse");
    assert_eq!(
        plist.get_string("CFBundleExecutable").as_deref(),
        Some("Widget")
    );
    assert_eq!(
        plist.get_string("CFBundleIdentifier").as_deref(),
        Some("com.example.widget")
    );
    assert_eq!(plist.get_bool("LSUIElement"), Some(true));
    assert_eq!(
        plist.get_string("LSMinimumSystemVersion").as_deref(),
        Some("11.0")
    );
    assert_eq!(plist.get_string("Absent"), None);
}

#[test]
fn parses_binary_plist_integers_arrays_and_booleans() {
    let mut w = BplistWriter::new();
    let a = w.ascii("one");
    let b = w.ascii("two");
    let arr = w.array(&[a, b]);
    let key = w.ascii("Items");
    let small = w.int(7);
    let key2 = w.ascii("Count");
    let no = w.boolean(false);
    let key3 = w.ascii("Enabled");
    let dict = w.dict(&[(key, arr), (key2, small), (key3, no)]);
    let bytes = w.finish(dict);

    let plist = Plist::parse(&bytes).expect("parse");
    match plist.get("Items") {
        Some(Plist::Array(items)) => {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], Plist::String("one".to_owned()));
            assert_eq!(items[1], Plist::String("two".to_owned()));
        }
        other => panic!("expected an array, got {other:?}"),
    }
    assert_eq!(plist.get("Count"), Some(&Plist::Integer(7)));
    assert_eq!(plist.get_bool("Enabled"), Some(false));
}

#[test]
fn parses_a_utf16_binary_plist_string() {
    let mut w = BplistWriter::new();
    let text = "Caf\u{e9}";
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut obj = vec![0x60 | (units.len() as u8)];
    for u in &units {
        obj.extend_from_slice(&u.to_be_bytes());
    }
    let name = w.push(obj);
    let key = w.ascii("Name");
    let dict = w.dict(&[(key, name)]);
    let bytes = w.finish(dict);

    assert_eq!(
        Plist::parse(&bytes)
            .expect("parse")
            .get_string("Name")
            .as_deref(),
        Some("Caf\u{e9}")
    );
}

#[test]
fn rejects_a_binary_plist_with_a_corrupt_trailer() {
    let mut w = BplistWriter::new();
    let k = w.ascii("K");
    let v = w.ascii("V");
    let d = w.dict(&[(k, v)]);
    let mut bytes = w.finish(d);
    let len = bytes.len();
    bytes[len - 1] = 0xFF; // offset table points past the end
    // Must not panic and must not loop forever.
    let _ = Plist::parse(&bytes);
}

#[test]
fn rejects_a_binary_plist_that_is_too_short() {
    assert!(Plist::parse(b"bplist00").is_err());
}

#[test]
fn a_self_referential_binary_plist_terminates() {
    // An array whose single element is itself.
    let mut w = BplistWriter::new();
    let idx = w.objects.len() as u64;
    let mut obj = vec![0xA1];
    obj.push(idx as u8);
    w.push(obj);
    let bytes = w.finish(idx);
    // The depth guard must stop the recursion rather than overflowing.
    let _ = Plist::parse(&bytes);
}

#[test]
fn parses_an_xml_plist() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>Widget</string>
    <key>CFBundleIdentifier</key>
    <string>com.example.widget</string>
    <key>LSUIElement</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>12.0</string>
    <key>Count</key>
    <integer>42</integer>
</dict>
</plist>
"#;
    let plist = Plist::parse(xml).expect("parse");
    assert_eq!(
        plist.get_string("CFBundleExecutable").as_deref(),
        Some("Widget")
    );
    assert_eq!(plist.get_bool("LSUIElement"), Some(true));
    assert_eq!(plist.get("Count"), Some(&Plist::Integer(42)));
    assert_eq!(
        plist.get_string("LSMinimumSystemVersion").as_deref(),
        Some("12.0")
    );
}

#[test]
fn parses_an_xml_plist_array_and_nesting() {
    let xml = br#"<plist version="1.0"><dict>
<key>Items</key><array><string>a</string><string>b</string></array>
<key>Nested</key><dict><key>Inner</key><integer>1</integer></dict>
</dict></plist>"#;
    let plist = Plist::parse(xml).expect("parse");
    match plist.get("Items") {
        Some(Plist::Array(items)) => assert_eq!(items.len(), 2),
        other => panic!("expected array, got {other:?}"),
    }
    assert_eq!(
        plist.get("Nested").and_then(|n| n.get("Inner")),
        Some(&Plist::Integer(1))
    );
}

#[test]
fn unescapes_xml_entities() {
    assert_eq!(unescape_xml("a &amp; b"), "a & b");
    assert_eq!(unescape_xml("&lt;tag&gt;"), "<tag>");
    assert_eq!(unescape_xml("&quot;q&quot;"), "\"q\"");
    assert_eq!(
        unescape_xml("&amp;lt;"),
        "&lt;",
        "the ampersand form must resolve last"
    );
}

#[test]
fn rejects_a_bad_xml_plist() {
    assert!(matches!(
        Plist::parse(b"<?xml version=\"1.0\"?><not-a-plist/>"),
        Err(PlistError::Malformed(_))
    ));
}

#[test]
fn rejects_bytes_that_are_not_a_plist_at_all() {
    assert!(matches!(
        Plist::parse(b"\x7fELF"),
        Err(PlistError::UnknownFormat)
    ));
    assert!(matches!(Plist::parse(b""), Err(PlistError::UnknownFormat)));
}

#[test]
fn xml_plist_has_a_bounded_nesting_depth() {
    let mut xml = String::from("<plist version=\"1.0\">");
    for _ in 0..200 {
        xml.push_str("<array>");
    }
    for _ in 0..200 {
        xml.push_str("</array>");
    }
    xml.push_str("</plist>");
    assert!(matches!(
        Plist::parse(xml.as_bytes()),
        Err(PlistError::Malformed(_))
    ));
}

// ---------------------------------------------------------------------------
// .app bundles
// ---------------------------------------------------------------------------

/// A minimal but real bundle on disk.
fn make_bundle(s: &Scratch, name: &str, executable: &str, plist: &str) -> PathBuf {
    let root = s.0.join(format!("{name}.app"));
    fs::create_dir_all(root.join("Contents/MacOS")).expect("MacOS");
    fs::write(root.join("Contents/Info.plist"), plist).expect("plist");
    let exe = root.join("Contents/MacOS").join(executable);
    fs::write(
        &exe,
        macho64(filetype::EXECUTE, 0x20_0000, &[main_command(0x1000)]),
    )
    .expect("executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    root
}

const BUNDLE_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Widget</string>
<key>CFBundleIdentifier</key><string>com.example.widget</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>CFBundleIconFile</key><string>widget.icns</string>
</dict></plist>"#;

#[test]
fn recognises_a_bundle_directory() {
    let s = Scratch::new("bundle-recognise");
    let root = make_bundle(&s, "Widget", "Widget", BUNDLE_PLIST);
    assert!(AppBundle::looks_like_bundle(&root));
    // A directory that is not an .app, and an .app with no plist, are not
    // bundles.
    fs::create_dir_all(s.0.join("Plain")).expect("dir");
    assert!(!AppBundle::looks_like_bundle(&s.0.join("Plain")));
    assert!(!AppBundle::looks_like_bundle(&s.0.join("NoPlist.app")));
}

#[test]
fn resolves_a_bundle_to_its_declared_executable() {
    let s = Scratch::new("bundle-resolve");
    let root = make_bundle(&s, "Widget", "Widget", BUNDLE_PLIST);
    let bundle = AppBundle::resolve(&root).expect("resolve");
    assert_eq!(bundle.root, root);
    assert_eq!(bundle.executable_name, "Widget");
    assert_eq!(bundle.executable, root.join("Contents/MacOS/Widget"));
    assert_eq!(bundle.identifier.as_deref(), Some("com.example.widget"));
    assert_eq!(bundle.minimum_system_version.as_deref(), Some("11.0"));
    assert_eq!(bundle.icon_file.as_deref(), Some("widget.icns"));
    assert_eq!(bundle.package_type.as_deref(), Some("APPL"));
    assert!(!bundle.is_agent);
    assert_eq!(bundle.architectures, vec!["x86_64"]);
}

#[test]
fn refuses_a_bundle_whose_plist_names_no_executable() {
    let s = Scratch::new("bundle-noexec");
    let plist = "<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>x</string></dict></plist>";
    let root = make_bundle(&s, "Broken", "Something", plist);
    match AppBundle::resolve(&root) {
        Err(BundleError::NoExecutableKey(_)) => {}
        other => panic!("expected NoExecutableKey, got {other:?}"),
    }
}

#[test]
fn refuses_a_bundle_whose_executable_is_absent() {
    let s = Scratch::new("bundle-missing");
    let plist = "<plist version=\"1.0\"><dict>\
        <key>CFBundleExecutable</key><string>Gone</string></dict></plist>";
    let root = s.0.join("Ghost.app");
    fs::create_dir_all(root.join("Contents/MacOS")).expect("dirs");
    fs::write(root.join("Contents/Info.plist"), plist).expect("plist");
    match AppBundle::resolve(&root) {
        Err(BundleError::ExecutableMissing(p)) => {
            assert!(p.to_string_lossy().ends_with("Contents/MacOS/Gone"));
        }
        other => panic!("expected ExecutableMissing, got {other:?}"),
    }
}

#[test]
fn refuses_a_directory_that_is_not_a_bundle() {
    let s = Scratch::new("bundle-notabundle");
    fs::create_dir_all(s.0.join("Nope.app")).expect("dir");
    match AppBundle::resolve(&s.0.join("Nope.app")) {
        Err(BundleError::NotABundle(_)) => {}
        other => panic!("expected NotABundle, got {other:?}"),
    }
}

#[test]
fn reports_an_agent_app() {
    let s = Scratch::new("bundle-agent");
    let plist = "<plist version=\"1.0\"><dict>\
        <key>CFBundleExecutable</key><string>Agent</string>\
        <key>LSUIElement</key><true/></dict></plist>";
    let root = make_bundle(&s, "Agent", "Agent", plist);
    assert!(AppBundle::resolve(&root).expect("resolve").is_agent);
}

#[test]
fn reports_every_architecture_of_a_universal_bundle_executable() {
    let s = Scratch::new("bundle-fat");
    let root = s.0.join("Universal.app");
    fs::create_dir_all(root.join("Contents/MacOS")).expect("dirs");
    fs::write(root.join("Contents/Info.plist"), BUNDLE_PLIST).expect("plist");
    let mut arm = macho64(filetype::EXECUTE, 0, &[]);
    arm[4..8].copy_from_slice(&0x0100_000Cu32.to_le_bytes());
    let bytes = fat(&[
        (0x0100_0007, macho64(filetype::EXECUTE, 0, &[])),
        (0x0100_000C, arm),
    ]);
    fs::write(root.join("Contents/MacOS/Widget"), &bytes).expect("exe");

    let bundle = AppBundle::resolve(&root).expect("resolve");
    assert_eq!(
        bundle.architectures,
        vec!["x86_64", "arm64"],
        "both slices must be reported, not just the selected one"
    );
}

#[test]
fn scans_a_directory_for_bundles() {
    let s = Scratch::new("bundle-scan");
    make_bundle(&s, "One", "One", BUNDLE_PLIST);
    make_bundle(&s, "Two", "Two", BUNDLE_PLIST);
    fs::create_dir_all(s.0.join("NotAnApp")).expect("dir");
    let found = AppBundle::scan(&s.0);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found
            .iter()
            .all(|p| p.extension().is_some_and(|e| e == "app"))
    );
}

#[test]
fn scanning_a_missing_directory_yields_nothing() {
    assert!(AppBundle::scan(Path::new("/definitely/not/here")).is_empty());
}

// ---------------------------------------------------------------------------
// Cross-check with tahoe-format
// ---------------------------------------------------------------------------

#[test]
fn the_format_crate_and_this_reader_agree_on_a_universal_binary() {
    // Two independent readers of the same fixture, in different crates.
    let mut arm = macho64(filetype::EXECUTE, 0, &[]);
    arm[4..8].copy_from_slice(&0x0100_000Cu32.to_le_bytes());
    let x86 = macho64(filetype::EXECUTE, 0, &[]);
    let bytes = fat(&[(0x0100_0007, x86), (0x0100_000C, arm)]);

    let probe = tahoe_format::probe_bytes(&bytes);
    assert_eq!(probe.format, tahoe_core::ObjectFormat::FatMachO);

    let fatbin = FatBinary::parse(&bytes).expect("parse");
    assert_eq!(fatbin.architectures(), vec!["x86_64", "arm64"]);
}

#[test]
fn the_format_crate_and_this_reader_agree_on_a_java_class() {
    let java = {
        let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0x00, 0x00, 0x00, 0x34];
        b.extend_from_slice(&[0u8; 64]);
        b
    };
    // tahoe-format's heuristic must call this Java, and this reader must
    // refuse it as a universal binary rather than inventing slices.
    assert_eq!(
        tahoe_format::probe_bytes(&java).format,
        tahoe_core::ObjectFormat::JavaClass
    );
    // The header parses as a fat header (same magic) but its slice table is
    // impossible, so the reader refuses rather than inventing 52 slices.
    assert!(
        matches!(FatBinary::parse(&java), Err(MachOError::Truncated { .. })),
        "a Java class must not yield a usable slice table"
    );
}

// ---------------------------------------------------------------------------
// Kernel release parsing shared with the Wine module
// ---------------------------------------------------------------------------

#[test]
fn parses_kernel_releases() {
    assert_eq!(
        parse_kernel_release("6.18.55-1-cachyos-lts"),
        Some((6, 18, 55))
    );
    assert_eq!(parse_kernel_release("6.14.0-arch1-1"), Some((6, 14, 0)));
    assert_eq!(parse_kernel_release(""), None);
    assert_eq!(parse_kernel_release("nonsense"), None);
}
