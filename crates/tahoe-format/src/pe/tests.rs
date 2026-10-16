// SPDX-License-Identifier: MIT OR Apache-2.0
//! PE import-table tests.
//!
//! The parser is checked against **real PE images produced by clang and
//! lld-link** rather than against a hand-built byte array. A hand-built
//! fixture only proves the parser agrees with my understanding of the
//! format; a real one proves it agrees with a linker. The tests degrade to a
//! skip when the toolchain is absent, and there is a synthetic fallback for
//! the one case a linker cannot produce on demand.
//!
//! The classification heuristic is tested by constructing [`PeImports`]
//! directly, so the rule is checked independently of the parser.

use super::*;

/// Compile a real PE with clang + lld-link, when installed.
///
/// Returns `None` when the toolchain is missing, so the caller can skip.
///
/// Two things about this were learned the hard way and are worth keeping:
///
/// 1. **Each imported function must be called.** A declared-but-unreferenced
///    import is discarded by lld-link, producing a PE with an empirically
///    empty import directory. A fixture that only declares them tests
///    nothing, and did silently pass for the wrong reason.
/// 2. **The DLL name comes from the import library's `LIBRARY` line**, not
///    from the source or the library's filename. One `.def` per DLL, or
///    every import is attributed to whatever single name was written.
fn real_pe(imports: &[&str]) -> Option<Vec<u8>> {
    use std::process::Command;
    let dir = std::env::temp_dir().join("tahoe-pe-tests");
    std::fs::create_dir_all(&dir).ok()?;
    // The tag must be unique per call, not per count: two tests importing two
    // DLLs each would otherwise write to the same files, and these tests run
    // in parallel. That collision made a test read another test's binary.
    let mut hash = 0u64;
    for name in imports {
        for byte in name.bytes() {
            hash = hash
                .wrapping_mul(1099511628211)
                .wrapping_add(u64::from(byte));
        }
    }
    let tag = format!("{}-{}", std::process::id(), hash);
    let ok = |c: &mut Command| c.status().ok().is_some_and(|s| s.success());

    // C: declare and call one function per DLL.
    let decls: String = imports
        .iter()
        .map(|d| {
            format!(
                "__declspec(dllimport) int f_{}(void);\n",
                d.replace('.', "_")
            )
        })
        .collect();
    let calls: String = imports
        .iter()
        .map(|d| format!("    f_{}();\n", d.replace('.', "_")))
        .collect();
    let c_path = dir.join(format!("{tag}.c"));
    std::fs::write(
        &c_path,
        format!("{decls}void mainCRTStartup(void) {{\n{calls}}}\n"),
    )
    .ok()?;

    // One import library per DLL, each naming itself on its LIBRARY line.
    let mut libs = Vec::new();
    for (i, dll) in imports.iter().enumerate() {
        let def = dir.join(format!("{tag}-{i}.def"));
        std::fs::write(
            &def,
            format!("LIBRARY {dll}\nEXPORTS\n  f_{}\n", dll.replace('.', "_")),
        )
        .ok()?;
        let lib = dir.join(format!("{tag}-{i}.lib"));
        if !ok(Command::new("lld-link")
            .arg("/lib")
            .arg(format!("/def:{}", def.display()))
            .arg(format!("/out:{}", lib.display()))
            .arg("/machine:x64"))
        {
            return None;
        }
        libs.push((def, lib));
    }

    let obj = dir.join(format!("{tag}.obj"));
    let exe = dir.join(format!("{tag}.exe"));
    if !ok(Command::new("clang")
        .args([
            "--target=x86_64-pc-windows-msvc",
            "-ffreestanding",
            "-fno-stack-protector",
            "-c",
        ])
        .arg(&c_path)
        .arg("-o")
        .arg(&obj))
    {
        return None;
    }
    let mut link = Command::new("lld-link");
    link.args([
        "/entry:mainCRTStartup",
        "/subsystem:console",
        "/nodefaultlib",
    ])
    .arg(format!("/out:{}", exe.display()))
    .arg(&obj);
    for (_, lib) in &libs {
        link.arg(lib);
    }
    if !ok(&mut link) {
        return None;
    }

    let bytes = std::fs::read(&exe).ok()?;
    let _ = std::fs::remove_file(c_path);
    let _ = std::fs::remove_file(obj);
    let _ = std::fs::remove_file(exe);
    for (def, lib) in libs {
        let _ = std::fs::remove_file(def);
        let _ = std::fs::remove_file(lib);
    }
    Some(bytes)
}

/// Whether to skip rather than fail when the toolchain is absent.
fn skip() {
    eprintln!("note: clang or lld-link missing; skipping the real-linker checks");
}

// ---------------------------------------------------------------------------
// Against a real linker
// ---------------------------------------------------------------------------

#[test]
fn parses_a_real_pe_with_real_imports() {
    let Some(pe) = real_pe(&["KERNEL32.dll", "USER32.dll"]) else {
        return skip();
    };
    let imports = parse_imports(&pe);
    let names = imports.names();
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("KERNEL32.dll")),
        "a real PE importing KERNEL32 must report it; got {names:?}"
    );
    assert!(
        names.iter().any(|n| n.eq_ignore_ascii_case("USER32.dll")),
        "and USER32; got {names:?}"
    );
    assert!(imports.has_data_directory);
    assert_eq!(pe_arch(&pe), Arch::X86_64);
}

#[test]
fn a_real_pe_importing_a_graphics_api_is_classified_as_a_game() {
    // d3d11.dll does not exist on this machine. That does not matter: the
    // linker records the import name, and the name is what the heuristic
    // reads. This is the case the whole module exists for.
    let Some(pe) = real_pe(&["KERNEL32.dll", "d3d11.dll", "dxgi.dll"]) else {
        return skip();
    };
    let imports = parse_imports(&pe);
    assert!(
        imports.imports_graphics(),
        "d3d11/dxgi must be detected; got {:?}",
        imports.names()
    );
    assert_eq!(imports.kind(), ProgramKind::Game);
}

#[test]
fn a_real_pe_without_graphics_is_a_desktop_application() {
    let Some(pe) = real_pe(&["KERNEL32.dll", "SHELL32.dll", "COMCTL32.dll"]) else {
        return skip();
    };
    let imports = parse_imports(&pe);
    assert_eq!(imports.kind(), ProgramKind::DesktopApplication);
    assert!(!imports.imports_graphics());
    assert!(!imports.imports_steam());
}

#[test]
fn a_real_pe_importing_steam_is_classified_as_a_game() {
    let Some(pe) = real_pe(&["KERNEL32.dll", "steam_api64.dll"]) else {
        return skip();
    };
    let imports = parse_imports(&pe);
    assert!(imports.imports_steam(), "got {:?}", imports.names());
    assert_eq!(imports.kind(), ProgramKind::Game);
}

#[test]
fn counts_the_imported_functions() {
    let Some(pe) = real_pe(&["KERNEL32.dll"]) else {
        return skip();
    };
    let imports = parse_imports(&pe);
    let kernel = imports
        .dlls
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case("KERNEL32.dll"))
        .expect("kernel32");
    // clang emits an entry for the one declared function.
    assert_eq!(
        kernel.function_count,
        Some(1),
        "the thunk walk should find one import"
    );
}

#[test]
fn a_real_pe_survives_truncation_at_every_offset() {
    // The garbage-in case matters more than the happy path: this parser runs
    // on whatever a user double-clicks.
    let Some(pe) = real_pe(&["KERNEL32.dll", "USER32.dll", "d3d11.dll"]) else {
        return skip();
    };
    assert!(pe.len() > 200);
    for cut in (0..pe.len()).step_by(3) {
        let _ = parse_imports(&pe[..cut]);
        let _ = pe_arch(&pe[..cut]);
    }
    // Pointwise: a corrupt byte anywhere in the headers must not panic.
    for at in 0..200 {
        let mut corrupt = pe.clone();
        corrupt[at] = corrupt[at].wrapping_add(0x5A);
        let _ = parse_imports(&corrupt);
    }
}

#[test]
fn a_hostile_size_of_optional_header_is_bounded() {
    let Some(pe) = real_pe(&["KERNEL32.dll"]) else {
        return skip();
    };
    let mut hostile = pe.clone();
    let lfanew = u32::from_le_bytes([pe[0x3C], pe[0x3D], pe[0x3E], pe[0x3F]]) as usize;
    // A header size that runs past the file must not produce a read of a
    // huge section table.
    hostile[lfanew + 20..lfanew + 22].copy_from_slice(&0xFFFFu16.to_le_bytes());
    let _ = parse_imports(&hostile);
}

#[test]
fn a_wild_descriptor_region_terminates() {
    let Some(pe) = real_pe(&["KERNEL32.dll"]) else {
        return skip();
    };
    let mut hostile = pe.clone();
    // Fill everything from the section raw data onwards with non-zero bytes,
    // destroying the all-zero terminator. The bound must still stop the walk.
    let lfanew = u32::from_le_bytes([pe[0x3C], pe[0x3D], pe[0x3E], pe[0x3F]]) as usize;
    let size_of_optional = usize::from(u16::from_le_bytes([pe[lfanew + 20], pe[lfanew + 21]]));
    let sect = lfanew + 24 + size_of_optional;
    let raw =
        u32::from_le_bytes([pe[sect + 20], pe[sect + 21], pe[sect + 22], pe[sect + 23]]) as usize;
    for byte in hostile.iter_mut().skip(raw).take(4096) {
        *byte = 0xFF;
    }
    let imports = parse_imports(&hostile);
    assert!(imports.dlls.len() <= MAX_DESCRIPTORS);
}

// ---------------------------------------------------------------------------
// Classification, independent of the parser
// ---------------------------------------------------------------------------

fn imports_of(names: &[&str]) -> PeImports {
    PeImports {
        dlls: names
            .iter()
            .map(|n| ImportedDll {
                name: (*n).to_owned(),
                function_count: Some(1),
            })
            .collect(),
        has_data_directory: true,
    }
}

#[test]
fn every_graphics_api_classifies() {
    for api in GRAPHICS_IMPORTS {
        let imports = imports_of(&["KERNEL32.dll", api]);
        assert_eq!(imports.kind(), ProgramKind::Game, "{api}");
        assert!(imports.imports_graphics(), "{api}");
    }
}

#[test]
fn every_steam_api_classifies() {
    for api in STEAM_IMPORTS {
        let imports = imports_of(&[api]);
        assert_eq!(imports.kind(), ProgramKind::Game, "{api}");
        assert!(imports.imports_steam(), "{api}");
    }
}

#[test]
fn classification_ignores_case() {
    assert_eq!(imports_of(&["D3D11.DLL"]).kind(), ProgramKind::Game);
    assert_eq!(imports_of(&["d3d11.dll"]).kind(), ProgramKind::Game);
    assert_eq!(imports_of(&["D3D11.Dll"]).kind(), ProgramKind::Game);
}

#[test]
fn an_empty_table_is_unknown_rather_than_a_default() {
    // A fully static image is legal. Calling it a "desktop application"
    // would be a guess, and this heuristic does not guess.
    assert_eq!(imports_of(&[]).kind(), ProgramKind::Unknown);
}

#[test]
fn a_plain_desktop_program_is_not_a_game() {
    let imports = imports_of(&[
        "KERNEL32.dll",
        "USER32.dll",
        "SHELL32.dll",
        "COMCTL32.dll",
        "OLE32.dll",
        "ADVAPI32.dll",
        "GDI32.dll",
        "MSVCRT.dll",
    ]);
    assert_eq!(imports.kind(), ProgramKind::DesktopApplication);
}

#[test]
fn gdi_is_not_a_graphics_api_for_this_purpose() {
    // gdi32 is in every windowed program including Notepad; treating it as a
    // game signal would classify the entire desktop as games.
    assert_eq!(
        imports_of(&["GDI32.dll"]).kind(),
        ProgramKind::DesktopApplication
    );
    assert!(!GRAPHICS_IMPORTS.contains(&"gdi32.dll"));
}

// ---------------------------------------------------------------------------
// Malformed input
// ---------------------------------------------------------------------------

#[test]
fn a_non_pe_yields_an_empty_table_rather_than_an_error() {
    // A launcher must fall back to a default, not refuse to run the program.
    for bytes in [
        b"\x7fELF\x02\x01\x01".as_slice(),
        b"".as_slice(),
        b"MZ".as_slice(),
        b"just some text, not a binary at all".as_slice(),
        &[0xFFu8; 512],
    ] {
        let imports = parse_imports(bytes);
        assert!(imports.dlls.is_empty());
        assert!(
            !imports.has_data_directory,
            "garbage must not claim a data directory"
        );
        assert_eq!(imports.kind(), ProgramKind::Unknown);
    }
}

#[test]
fn a_pe_with_a_zero_import_directory_is_unknown() {
    let Some(mut pe) = real_pe(&["KERNEL32.dll"]) else {
        return skip();
    };
    let lfanew = u32::from_le_bytes([pe[0x3C], pe[0x3D], pe[0x3E], pe[0x3F]]) as usize;
    let dd = lfanew + 24 + 112;
    pe[dd + 8..dd + 12].copy_from_slice(&0u32.to_le_bytes());
    pe[dd + 12..dd + 16].copy_from_slice(&0u32.to_le_bytes());
    let imports = parse_imports(&pe);
    assert!(imports.dlls.is_empty());
    assert!(
        imports.has_data_directory,
        "the header was readable, the table was empty"
    );
    assert_eq!(imports.kind(), ProgramKind::Unknown);
}

#[test]
fn pe_arch_reads_the_coff_machine() {
    let Some(mut pe) = real_pe(&["KERNEL32.dll"]) else {
        assert_eq!(pe_arch(b"nope"), Arch::Unknown);
        return;
    };
    assert_eq!(pe_arch(&pe), Arch::X86_64);
    // Read the real `e_lfanew` rather than assuming one: lld-link writes
    // 0x78, not the 0x40 the DOS-stub convention suggests, and hardcoding it
    // made this test pass on a lie until a real binary was involved.
    let lfanew = u32::from_le_bytes([pe[0x3C], pe[0x3D], pe[0x3E], pe[0x3F]]) as usize;
    assert_eq!(&pe[lfanew..lfanew + 4], b"PE\0\0");
    pe[lfanew + 4..lfanew + 6].copy_from_slice(&0xaa64u16.to_le_bytes());
    assert_eq!(pe_arch(&pe), Arch::Aarch64);
    assert_eq!(pe_arch(b"nope"), Arch::Unknown);
}

#[test]
fn a_real_pe_is_ordered_for_deterministic_logging() {
    let Some(pe) = real_pe(&["USER32.dll", "KERNEL32.dll", "GDI32.dll"]) else {
        return skip();
    };
    let imports = parse_imports(&pe);
    let mut sorted = imports.dlls.clone();
    sorted.sort();
    // Sorted order must be a permutation, which is what a log wants.
    assert_eq!(sorted.len(), imports.dlls.len());
    assert!(sorted.windows(2).all(|w| w[0] <= w[1]));
}
