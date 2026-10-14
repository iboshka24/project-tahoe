// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fixture-driven tests for [`crate::probe_bytes`] and [`crate::probe_file`].
//!
//! Every header below is synthesised byte-by-byte so the test documents the
//! exact layout it asserts. Nothing here executes a binary.

use crate::{
    APPIMAGE_MAGIC, APPIMAGE_OFFSET, FormatDetail, elf_type_name, pe_subsystem_name, probe_bytes,
    probe_file,
};
use tahoe_core::{Arch, Endianness, ObjectFormat, OsAbi};

fn elf_header(class: u8, data: u8, osabi: u8, e_type: u16, machine: u16) -> Vec<u8> {
    let mut b = vec![
        0x7f, b'E', b'L', b'F', class, data, 1, osabi, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let be = data == 2;
    let t = if be {
        e_type.to_be_bytes()
    } else {
        e_type.to_le_bytes()
    };
    let m = if be {
        machine.to_be_bytes()
    } else {
        machine.to_le_bytes()
    };
    b.extend_from_slice(&t);
    b.extend_from_slice(&m);
    b.extend_from_slice(&[0u8; 44]);
    b
}

fn pe_image(machine: u16, opt_magic: u16, subsystem: u16) -> Vec<u8> {
    let mut b = vec![0u8; 64];
    b[0] = b'M';
    b[1] = b'Z';
    // e_lfanew = 0x40
    b[0x3c] = 0x40;
    b.extend_from_slice(b"PE\0\0");
    b.extend_from_slice(&machine.to_le_bytes());
    // pad COFF header to 24 bytes total (4 sig + 20 COFF)
    b.extend_from_slice(&[0u8; 18]);
    b.extend_from_slice(&opt_magic.to_le_bytes());
    // pad to subsystem offset (68 into optional header)
    while b.len() < 0x40 + 24 + 68 {
        b.push(0);
    }
    b.extend_from_slice(&subsystem.to_le_bytes());
    b.extend_from_slice(&[0u8; 16]);
    b
}

fn macho(magic: [u8; 4], cputype_le: u32) -> Vec<u8> {
    let mut b = magic.to_vec();
    // cputype stored in the file's endianness: LE magics -> LE, BE magics -> BE
    let be_file = magic[0] == 0xfe;
    let ct = if be_file {
        cputype_le.to_be_bytes()
    } else {
        cputype_le.to_le_bytes()
    };
    b.extend_from_slice(&ct);
    b.extend_from_slice(&[0u8; 24]);
    b
}

#[test]
fn elf64_le_exec_x86_64() {
    let r = probe_bytes(&elf_header(2, 1, 0, 2, 62));
    assert_eq!(r.format, ObjectFormat::Elf);
    assert_eq!(r.arch, Arch::X86_64);
    assert_eq!(r.bits, Some(64));
    assert_eq!(r.endian, Some(Endianness::Little));
    assert_eq!(r.os_abi, OsAbi::ElfSystemV);
    assert_eq!(
        r.detail,
        FormatDetail::Elf {
            e_type: 2,
            e_type_name: "EXEC",
            osabi_byte: 0
        }
    );
}

#[test]
fn elf64_le_dyn_x86_64() {
    let r = probe_bytes(&elf_header(2, 1, 0, 3, 62));
    assert_eq!(r.format, ObjectFormat::Elf);
    assert_eq!(r.arch, Arch::X86_64);
    assert!(matches!(r.detail, FormatDetail::Elf { e_type: 3, .. }));
}

#[test]
fn elf32_le_exec_x86() {
    let r = probe_bytes(&elf_header(1, 1, 0, 2, 3));
    assert_eq!(r.arch, Arch::X86);
    assert_eq!(r.bits, Some(32));
}

#[test]
fn elf32_be_exec_ppc() {
    // e_machine 20 = PowerPC, big-endian layout
    let r = probe_bytes(&elf_header(1, 2, 0, 2, 20));
    assert_eq!(r.endian, Some(Endianness::Big));
    assert_eq!(r.arch, Arch::OtherElf(20));
}

#[test]
fn elf64_be_dyn_aarch64() {
    let r = probe_bytes(&elf_header(2, 2, 0, 3, 183));
    assert_eq!(r.endian, Some(Endianness::Big));
    assert_eq!(r.arch, Arch::Aarch64);
}

#[test]
fn elf_osabi_linux() {
    let r = probe_bytes(&elf_header(2, 1, 3, 2, 62));
    assert_eq!(r.os_abi, OsAbi::ElfLinux);
}

#[test]
fn elf_osabi_freebsd_is_unspecified() {
    // EI_OSABI 9 (FreeBSD) is not in our model: honestly unspecified.
    let r = probe_bytes(&elf_header(2, 1, 9, 2, 62));
    assert_eq!(r.os_abi, OsAbi::Unspecified);
    assert_eq!(r.format, ObjectFormat::Elf);
}

#[test]
fn elf_core_type() {
    let r = probe_bytes(&elf_header(2, 1, 0, 4, 62));
    assert!(matches!(
        r.detail,
        FormatDetail::Elf {
            e_type: 4,
            e_type_name: "CORE",
            ..
        }
    ));
}

#[test]
fn elf_rel_type() {
    let r = probe_bytes(&elf_header(1, 1, 0, 1, 3));
    assert!(matches!(
        r.detail,
        FormatDetail::Elf {
            e_type: 1,
            e_type_name: "REL",
            ..
        }
    ));
}

#[test]
fn elf_truncated_magic_only() {
    let r = probe_bytes(&[0x7f, b'E', b'L', b'F']);
    assert_eq!(r.format, ObjectFormat::Elf);
    assert_eq!(r.arch, Arch::Unknown);
    assert_eq!(r.bits, None);
}

#[test]
fn elf_truncated_8_bytes() {
    let r = probe_bytes(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0]);
    assert_eq!(r.format, ObjectFormat::Elf);
    assert_eq!(r.bits, Some(64));
    assert_eq!(r.arch, Arch::Unknown);
}

#[test]
fn elf_bad_ei_data_endian_none() {
    let r = probe_bytes(&elf_header(2, 0, 0, 2, 62));
    assert_eq!(r.endian, None);
    assert_eq!(r.format, ObjectFormat::Elf);
}

#[test]
fn elf_type_names_cover_table() {
    assert_eq!(elf_type_name(0), "NONE");
    assert_eq!(elf_type_name(1), "REL");
    assert_eq!(elf_type_name(2), "EXEC");
    assert_eq!(elf_type_name(3), "DYN");
    assert_eq!(elf_type_name(4), "CORE");
    assert_eq!(elf_type_name(0xfe00), "UNKNOWN");
}

#[test]
fn appimage_detected() {
    let mut b = elf_header(2, 1, 0, 3, 62);
    b[APPIMAGE_OFFSET..APPIMAGE_OFFSET + 3].copy_from_slice(APPIMAGE_MAGIC);
    let r = probe_bytes(&b);
    assert_eq!(r.format, ObjectFormat::AppImage);
    assert_eq!(r.arch, Arch::X86_64);
}

#[test]
fn appimage_needs_full_magic() {
    // Only "AI" without the \x02 terminator: plain ELF, not AppImage.
    let mut b = elf_header(2, 1, 0, 3, 62);
    b[APPIMAGE_OFFSET] = b'A';
    b[APPIMAGE_OFFSET + 1] = b'I';
    b[APPIMAGE_OFFSET + 2] = 0x03;
    assert_eq!(probe_bytes(&b).format, ObjectFormat::Elf);
}

#[test]
fn pe32plus_x86_64_gui() {
    let r = probe_bytes(&pe_image(0x8664, 0x20b, 2));
    assert_eq!(r.format, ObjectFormat::Pe);
    assert_eq!(r.arch, Arch::X86_64);
    assert_eq!(r.bits, Some(64));
    assert_eq!(
        r.detail,
        FormatDetail::Pe {
            has_pe_signature: true,
            machine: Some(0x8664),
            pe32_plus: Some(true),
            subsystem: Some(2),
        }
    );
    assert_eq!(pe_subsystem_name(2), "GUI");
}

#[test]
fn pe32_x86_console() {
    let r = probe_bytes(&pe_image(0x014c, 0x10b, 3));
    assert_eq!(r.arch, Arch::X86);
    assert_eq!(r.bits, Some(32));
    assert!(matches!(
        r.detail,
        FormatDetail::Pe {
            pe32_plus: Some(false),
            subsystem: Some(3),
            ..
        }
    ));
    assert_eq!(pe_subsystem_name(3), "CONSOLE");
}

#[test]
fn pe_arm64() {
    let r = probe_bytes(&pe_image(0xaa64, 0x20b, 2));
    assert_eq!(r.arch, Arch::Aarch64);
}

#[test]
fn pe_subsystem_names_cover_table() {
    assert_eq!(pe_subsystem_name(1), "NATIVE");
    assert_eq!(pe_subsystem_name(10), "EFI_APPLICATION");
    assert_eq!(pe_subsystem_name(0xffff), "UNKNOWN");
}

#[test]
fn pe_truncated_no_lfanew() {
    let r = probe_bytes(b"MZ");
    assert_eq!(r.format, ObjectFormat::Pe);
    assert_eq!(r.arch, Arch::Unknown);
    assert!(matches!(
        r.detail,
        FormatDetail::Pe {
            has_pe_signature: false,
            ..
        }
    ));
}

#[test]
fn pe_lfanew_beyond_buffer() {
    let mut b = vec![0u8; 64];
    b[0] = b'M';
    b[1] = b'Z';
    // e_lfanew points 1 MiB past the end.
    b[0x3c..0x40].copy_from_slice(&0x10_0000u32.to_le_bytes());
    let r = probe_bytes(&b);
    assert_eq!(r.format, ObjectFormat::Pe);
    assert!(matches!(
        r.detail,
        FormatDetail::Pe {
            has_pe_signature: false,
            ..
        }
    ));
}

#[test]
fn pe_missing_signature_is_still_pe() {
    // MZ header whose e_lfanew points at non-PE data: a DOS MZ executable.
    let mut b = vec![0u8; 128];
    b[0] = b'M';
    b[1] = b'Z';
    b[0x3c] = 0x40;
    b[0x40..0x44].copy_from_slice(b"NOPE");
    let r = probe_bytes(&b);
    assert_eq!(r.format, ObjectFormat::Pe);
    assert_eq!(r.arch, Arch::Unknown);
}

#[test]
fn macho64_le_x86_64() {
    let r = probe_bytes(&macho([0xcf, 0xfa, 0xed, 0xfe], 0x0100_0007));
    assert_eq!(r.format, ObjectFormat::MachO);
    assert_eq!(r.arch, Arch::X86_64);
    assert_eq!(r.bits, Some(64));
    assert_eq!(r.os_abi, OsAbi::MacOs);
    assert_eq!(
        r.detail,
        FormatDetail::MachO {
            cputype: 0x0100_0007,
            is_64: true,
            swapped: false
        }
    );
}

#[test]
fn macho64_le_arm64() {
    let r = probe_bytes(&macho([0xcf, 0xfa, 0xed, 0xfe], 0x0100_000C));
    assert_eq!(r.arch, Arch::Aarch64);
}

#[test]
fn macho32_le_x86() {
    let r = probe_bytes(&macho([0xce, 0xfa, 0xed, 0xfe], 7));
    assert_eq!(r.arch, Arch::X86);
    assert_eq!(r.bits, Some(32));
}

#[test]
fn macho64_be_cigam() {
    let r = probe_bytes(&macho([0xfe, 0xed, 0xfa, 0xcf], 0x0100_0007));
    assert_eq!(r.arch, Arch::X86_64);
    assert_eq!(r.endian, Some(Endianness::Big));
    assert!(matches!(
        r.detail,
        FormatDetail::MachO { swapped: true, .. }
    ));
}

#[test]
fn macho32_be_cigam() {
    let r = probe_bytes(&macho([0xfe, 0xed, 0xfa, 0xce], 7));
    assert_eq!(r.arch, Arch::X86);
    assert_eq!(r.bits, Some(32));
}

#[test]
fn macho_truncated_to_magic() {
    let r = probe_bytes(&[0xcf, 0xfa, 0xed, 0xfe]);
    assert_eq!(r.format, ObjectFormat::MachO);
    assert_eq!(r.arch, Arch::Unknown);
    assert_eq!(r.bits, Some(64));
}

/// A synthetic universal binary with `arches` slices at the given offsets.
fn fat_binary(arches: &[(i32, u32, u32)]) -> Vec<u8> {
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe];
    b.extend_from_slice(&(arches.len() as u32).to_be_bytes());
    for (cputype, offset, size) in arches {
        b.extend_from_slice(&cputype.to_be_bytes());
        b.extend_from_slice(&0i32.to_be_bytes()); // cpusubtype
        b.extend_from_slice(&offset.to_be_bytes());
        b.extend_from_slice(&size.to_be_bytes());
        b.extend_from_slice(&12u32.to_be_bytes()); // align
    }
    b
}

#[test]
fn a_real_universal_binary_resolves_to_fat_macho() {
    let mut b = fat_binary(&[(0x0100_0007, 0x1000, 0x2000), (0x0100_000C, 0x4000, 0x2000)]);
    b.resize(0x6000, 0);
    // Each slice begins with a 64-bit Mach-O magic.
    b[0x1000..0x1004].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
    b[0x4000..0x4004].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);

    let r = probe_bytes(&b);
    assert_eq!(r.format, ObjectFormat::FatMachO);
    assert_eq!(
        r.arch,
        Arch::X86_64,
        "the first slice decides the reported arch"
    );
    assert_eq!(r.bits, Some(64));
    assert_eq!(r.os_abi, OsAbi::MacOs);
    match r.detail {
        FormatDetail::FatMachO {
            nfat_arch,
            slices,
            looks_like_java,
        } => {
            assert_eq!(nfat_arch, Some(2));
            assert!(!looks_like_java);
            assert_eq!(slices.len(), 2);
            assert_eq!(slices[0].arch, Arch::X86_64);
            assert_eq!(slices[0].offset, 0x1000);
            assert_eq!(slices[0].size, 0x2000);
            assert_eq!(slices[0].align, 12);
            assert_eq!(slices[1].arch, Arch::Aarch64);
        }
        other => panic!("expected FatMachO detail, got {other:?}"),
    }
}

#[test]
fn a_universal_binary_with_a_bad_slice_table_is_ambiguous() {
    // Plausible count, but no slice actually starts with a Mach-O magic.
    let mut b = fat_binary(&[(0x0100_0007, 0x1000, 0x2000)]);
    b.resize(0x2000, 0xAA);
    let r = probe_bytes(&b);
    assert_eq!(
        r.format,
        ObjectFormat::Ambiguous {
            candidates: &["java-class", "fat-macho"]
        }
    );
}

#[test]
fn a_java_class_is_recognised_as_java_not_macho() {
    // Java 8: minor 0, major 0x34 = 52, above the fat-arch plausibility
    // window, exactly as file(1) reasons.
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 0x34];
    b.extend_from_slice(&[0u8; 64]);
    let r = probe_bytes(&b);
    assert_eq!(r.format, ObjectFormat::JavaClass);
    match r.detail {
        FormatDetail::FatMachO {
            looks_like_java,
            nfat_arch,
            ..
        } => {
            assert!(looks_like_java);
            assert_eq!(nfat_arch, Some(52));
        }
        other => panic!("expected FatMachO detail, got {other:?}"),
    }
}

#[test]
fn a_huge_nfat_arch_is_java_not_macho() {
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0xff, 0xff, 0xff, 0xff];
    b.extend_from_slice(&[0u8; 64]);
    assert_eq!(probe_bytes(&b).format, ObjectFormat::JavaClass);
}

#[test]
fn fat_cigam_is_parsed_with_swapped_fields() {
    let mut b = vec![0xbe, 0xba, 0xfe, 0xca];
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&0x0100_0007i32.to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&0x200u32.to_le_bytes());
    b.extend_from_slice(&0x100u32.to_le_bytes());
    b.extend_from_slice(&12u32.to_le_bytes());
    b.resize(0x300, 0);
    b[0x200..0x204].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
    let r = probe_bytes(&b);
    assert_eq!(r.format, ObjectFormat::FatMachO);
    match r.detail {
        FormatDetail::FatMachO { slices, .. } => {
            assert_eq!(slices[0].arch, Arch::X86_64);
            assert_eq!(slices[0].offset, 0x200);
        }
        other => panic!("expected FatMachO detail, got {other:?}"),
    }
}

#[test]
fn a_zero_arch_universal_binary_has_no_slices() {
    let b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 0];
    match probe_bytes(&b).detail {
        FormatDetail::FatMachO { slices, .. } => assert!(slices.is_empty()),
        other => panic!("expected FatMachO detail, got {other:?}"),
    }
}

#[test]
fn container_formats_are_identified() {
    assert_eq!(probe_bytes(b"xar!....").format, ObjectFormat::Xar);
    assert_eq!(probe_bytes(b"bplist00\xd1").format, ObjectFormat::Plist);
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.extend_from_slice(&[0u8; 64]);
    assert_eq!(probe_bytes(&ole).format, ObjectFormat::Ole2);
}

#[test]
fn script_sh() {
    let r = probe_bytes(b"#!/bin/sh\necho hi\n");
    assert_eq!(r.format, ObjectFormat::Script);
    assert!(matches!(r.detail, FormatDetail::Script { .. }));
    if let FormatDetail::Script { interpreter } = r.detail {
        assert_eq!(interpreter, "/bin/sh");
    }
}

#[test]
fn script_python_env() {
    let r = probe_bytes(b"#!/usr/bin/env python3\nprint(1)\n");
    if let FormatDetail::Script { interpreter } = r.detail {
        assert_eq!(interpreter, "/usr/bin/env python3");
    } else {
        panic!("expected script detail");
    }
}

#[test]
fn script_no_newline() {
    let r = probe_bytes(b"#!/bin/bash");
    assert_eq!(r.format, ObjectFormat::Script);
    if let FormatDetail::Script { interpreter } = r.detail {
        assert_eq!(interpreter, "/bin/bash");
    } else {
        panic!("expected script detail");
    }
}

#[test]
fn script_trims_whitespace() {
    let r = probe_bytes(b"#!  /bin/sh  \n");
    if let FormatDetail::Script { interpreter } = r.detail {
        assert_eq!(interpreter, "/bin/sh");
    } else {
        panic!("expected script detail");
    }
}

#[test]
fn empty_file_is_unknown() {
    let r = probe_bytes(&[]);
    assert_eq!(r.format, ObjectFormat::Unknown);
    assert_eq!(r.arch, Arch::Unknown);
}

#[test]
fn short_inputs_never_panic() {
    for len in 0..70usize {
        let buf = vec![0x7fu8; len];
        let _ = probe_bytes(&buf);
        let mut mz = vec![0u8; len];
        if len >= 2 {
            mz[0] = b'M';
            mz[1] = b'Z';
        }
        let _ = probe_bytes(&mz);
    }
}

#[test]
fn garbage_is_unknown() {
    let r = probe_bytes(b"\x00\x01\x02\x03not a binary at all................");
    assert_eq!(r.format, ObjectFormat::Unknown);
}

#[test]
fn dmg_head_bytes_are_unknown() {
    // probe_bytes has no head magic for DMG: honest Unknown, not a guess.
    let r = probe_bytes(&[0u8; 512]);
    assert_eq!(r.format, ObjectFormat::Unknown);
}

fn write_temp(name: &str, data: &[u8]) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "tahoe-format-test-{name}-{}.bin",
        std::process::id()
    ));
    std::fs::write(&p, data).expect("write temp fixture");
    p
}

#[test]
fn probe_file_roundtrip_elf() {
    let p = write_temp("elf", &elf_header(2, 1, 0, 3, 62));
    let r = probe_file(&p).expect("probe temp file");
    std::fs::remove_file(&p).ok();
    assert_eq!(r.format, ObjectFormat::Elf);
    assert_eq!(r.arch, Arch::X86_64);
}

#[test]
fn probe_file_roundtrip_pe() {
    let p = write_temp("pe", &pe_image(0x8664, 0x20b, 3));
    let r = probe_file(&p).expect("probe temp file");
    std::fs::remove_file(&p).ok();
    assert_eq!(r.format, ObjectFormat::Pe);
}

#[test]
fn probe_file_missing_is_error() {
    let mut p = std::env::temp_dir();
    p.push("tahoe-format-test-definitely-missing.bin");
    std::fs::remove_file(&p).ok();
    assert!(probe_file(&p).is_err());
}

#[test]
fn probe_file_dmg_tail_koly() {
    // Garbage head + real koly trailer: Dmg, confident.
    let mut data = vec![0xA5u8; 2048];
    data.extend_from_slice(b"koly");
    data.extend_from_slice(&[0u8; 508]);
    let p = write_temp("dmg", &data);
    let r = probe_file(&p).expect("probe temp file");
    std::fs::remove_file(&p).ok();
    assert_eq!(r.format, ObjectFormat::Dmg);
    assert!(matches!(
        r.detail,
        FormatDetail::Dmg {
            confident: true,
            trailer: Some(_),
            ..
        }
    ));
}

#[test]
fn probe_file_without_koly_stays_unknown() {
    let data = vec![0xA5u8; 2048];
    let p = write_temp("nodmg", &data);
    let r = probe_file(&p).expect("probe temp file");
    std::fs::remove_file(&p).ok();
    assert_eq!(r.format, ObjectFormat::Unknown);
}

#[test]
fn probe_file_small_script() {
    let p = write_temp("sh", b"#!/bin/sh\necho hi\n");
    let r = probe_file(&p).expect("probe temp file");
    std::fs::remove_file(&p).ok();
    assert_eq!(r.format, ObjectFormat::Script);
}
