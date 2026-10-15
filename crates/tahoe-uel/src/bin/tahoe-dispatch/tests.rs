// SPDX-License-Identifier: MIT OR Apache-2.0
//! Dispatch tests.
//!
//! `dispatch` cannot be tested — it replaces the process — so the tests
//! exercise `resolve`, which is the decision. That split exists because of
//! these tests.
//!
//! The layer lookup reads the real PATH, so these tests assert *structure*
//! and the refusal path, not which binary was found.

use super::*;

fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join("tahoe-dispatch-tests");
    std::fs::create_dir_all(&dir).expect("scratch");
    let path = dir.join(format!("{name}-{}.bin", std::process::id()));
    std::fs::write(&path, bytes).expect("write");
    path
}

fn pe() -> Vec<u8> {
    let mut b = vec![0u8; 64];
    b[0] = b'M';
    b[1] = b'Z';
    b[0x3c] = 0x40;
    b.extend_from_slice(b"PE\0\0");
    b.extend_from_slice(&0x8664u16.to_le_bytes());
    b.extend_from_slice(&[0u8; 18]);
    b.extend_from_slice(&0x20bu16.to_le_bytes());
    while b.len() < 0x40 + 24 + 68 {
        b.push(0);
    }
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&[0u8; 16]);
    b
}

fn macho64() -> Vec<u8> {
    let mut b = vec![0xcf, 0xfa, 0xed, 0xfe];
    b.extend_from_slice(&0x0100_0007u32.to_le_bytes());
    b.extend_from_slice(&3u32.to_le_bytes());
    b.extend_from_slice(&2u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b
}

#[test]
fn a_non_foreign_format_is_refused_with_the_reason() {
    // The `ca fe ba be` rule matches Java class files, and a `MZ` rule
    // matches DOS executables. The interpreter must not hand those to Wine.
    let elf = scratch("elf", &{
        let mut b = vec![0x7f, b'E', b'L', b'F', 2, 1, 1, 0];
        b.extend_from_slice(&[0u8; 56]);
        b
    });
    let err = resolve(&elf, &[]).expect_err("an ELF must be refused");
    assert!(err.contains("not a Windows or macOS program"), "{err}");
    assert!(
        err.contains("too broad"),
        "the message must name the cause: {err}"
    );
    let _ = std::fs::remove_file(&elf);
}

#[test]
fn a_java_class_is_refused_and_named_as_such() {
    // This is the case the fat-Mach-O magic cannot express. The rule fires;
    // the interpreter must undo it.
    let java = scratch("java", &{
        let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0x00, 0x00, 0x00, 0x34];
        b.extend_from_slice(&[0u8; 64]);
        b
    });
    let err = resolve(&java, &[]).expect_err("a Java class must be refused");
    assert!(err.contains("java-class"), "{err}");
    let _ = std::fs::remove_file(&java);
}

#[test]
fn a_shell_script_is_refused() {
    let script = scratch("sh", b"#!/bin/sh\necho hi\n");
    let err = resolve(&script, &[]).expect_err("a script must be refused");
    assert!(err.contains("script"), "{err}");
    let _ = std::fs::remove_file(&script);
}

#[test]
fn a_missing_file_is_refused_with_the_io_reason() {
    let err = resolve(Path::new("/definitely/not/here.exe"), &[])
        .expect_err("a missing file must be refused");
    assert!(err.contains("cannot read it"), "{err}");
}

#[test]
fn the_refusal_for_a_dmg_says_it_is_a_container() {
    // A `.dmg` cannot reach here through a shipped rule, but a hand-written
    // rule could manage it, and the message must not be "unknown file".
    let mut dmg = vec![0u8; 1024];
    dmg.extend_from_slice(b"koly");
    dmg.extend_from_slice(&[0u8; 508]);
    let path = scratch("dmg", &dmg);
    let err = resolve(&path, &[]).expect_err("a disk image must be refused");
    assert!(err.contains("dmg"), "{err}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_pe_without_wine_names_wine() {
    // Structural: on a machine with no Wine the refusal must name the layer
    // rather than say "unsupported format". On a machine with Wine it
    // produces a command instead, so both outcomes are accepted and the
    // assertion is only that neither is a lie.
    let path = scratch("pe", &pe());
    match resolve(&path, &[]) {
        Err(message) => assert!(
            message.contains("wine") || message.contains("Wine"),
            "a PE refusal must name Wine: {message}"
        ),
        Ok(command) => {
            let program = command.program().to_string_lossy().into_owned();
            assert!(
                program.contains("wine"),
                "a PE must dispatch to wine, got {program}"
            );
            assert_eq!(
                PathBuf::from(&command.argv[1]),
                path,
                "the binary must be the first argument after the program"
            );
        }
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn extra_arguments_are_forwarded_after_the_binary() {
    let path = scratch("pe-args", &pe());
    let rest: Vec<OsString> = ["--silent", "/install"].iter().map(Into::into).collect();
    if let Ok(command) = resolve(&path, &rest) {
        assert_eq!(
            PathBuf::from(&command.argv[1]),
            path,
            "the binary comes first"
        );
        let forwarded: Vec<String> = command.argv[2..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(forwarded, vec!["--silent", "/install"]);
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_macho_without_darling_names_darling() {
    let path = scratch("macho", &macho64());
    match resolve(&path, &[]) {
        Err(message) => assert!(
            message.contains("darling"),
            "a Mach-O refusal must name Darling: {message}"
        ),
        Ok(command) => {
            let program = command.program().to_string_lossy().into_owned();
            assert!(program.contains("darling"), "got {program}");
        }
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_program_is_never_this_binary() {
    // A dispatch that resolved to itself would fork forever. It cannot
    // happen through the UEL's strategies, and this pins that.
    let path = scratch("loop", &pe());
    if let Ok(command) = resolve(&path, &[]) {
        let program = command.program().to_string_lossy().into_owned();
        assert!(!program.contains("tahoe-dispatch"), "{program}");
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn same_file_distinguishes_paths_and_rejects_nonsense() {
    let a = scratch("same", &pe());
    assert!(same_file(&a, &a));
    assert!(!same_file(&a, Path::new("/definitely/not/here")));
    let _ = std::fs::remove_file(&a);
}
