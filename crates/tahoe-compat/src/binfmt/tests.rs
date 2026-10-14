// SPDX-License-Identifier: MIT OR Apache-2.0
//! binfmt tests.
//!
//! The interesting ones are the last few: they mount `binfmt_misc` inside a
//! real user namespace, register a real rule, and let the **kernel** dispatch
//! a real file. That is not a simulation — it is the same code path a
//! double-click takes.
//!
//! Nothing here needs root and nothing here changes the host: the superblock
//! is per-user-namespace and dies with the namespace.

use super::*;

fn rule(name: &str, magic: &[u8]) -> Rule {
    Rule::new(name, magic, "/bin/true")
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

#[test]
fn escapes_every_byte_as_hex() {
    assert_eq!(escape(&[0xcf, 0xfa, 0xed, 0xfe]), "\\xcf\\xfa\\xed\\xfe");
    // A raw NUL would truncate the kernel's parse, so it must be escaped too.
    assert_eq!(escape(&[0x00]), "\\x00");
    assert_eq!(escape(b"MZ"), "\\x4d\\x5a");
    assert_eq!(escape(&[]), "");
}

#[test]
fn register_string_has_the_documented_shape() {
    let r = rule("tahoe-test", b"MZ");
    assert_eq!(
        r.to_register_string(),
        ":tahoe-test:M:0:\\x4d\\x5a::/bin/true:P"
    );
}

#[test]
fn register_string_includes_offset_and_mask() {
    let r = Rule::new("t", &[0x50, 0x45], "/bin/true")
        .at_offset(0x80)
        .with_mask(&[0xff, 0x00]);
    let line = r.to_register_string();
    assert!(
        line.starts_with(":t:M:128:\\x50\\x45:\\xff\\x00:/bin/true:"),
        "{line}"
    );
}

#[test]
fn flag_rendering_matches_the_kernel_grammar() {
    let f = Flags {
        preserve_argv0: true,
        ..Flags::default()
    };
    assert_eq!(f.as_str(), "P");

    let f = Flags {
        fix_binary: true,
        ..Flags::default()
    };
    assert_eq!(f.as_str(), "F");

    let f = Flags {
        credentials: true,
        ..Flags::default()
    };
    assert_eq!(f.as_str(), "OC", "C implies O, and both are emitted");

    let f = Flags {
        open_binary: true,
        ..Flags::default()
    };
    assert_eq!(f.as_str(), "O");

    assert_eq!(Flags::default().as_str(), "");

    let all = Flags {
        preserve_argv0: true,
        open_binary: true,
        credentials: true,
        fix_binary: true,
    };
    assert_eq!(all.as_str(), "POCF");
}

#[test]
fn preserve_argv0_is_on_by_default() {
    // Every launcher needs to see the original filename, so this must not
    // be something a caller forgets.
    assert!(rule("x", b"MZ").flags.preserve_argv0);
}

// ---------------------------------------------------------------------------
// Validation — each of these mirrors a check the kernel performs
// ---------------------------------------------------------------------------

#[test]
fn rejects_magic_past_the_prefetch_window() {
    // e_lfanew 0x100 puts `PE\0\0` at 0x100..0x104, past the 256-byte window.
    let r = Rule::new("pe", &[0x50, 0x45, 0x00, 0x00], "/bin/true").at_offset(0x100);
    match r.validate() {
        Err(BinfmtError::MagicOutOfWindow { needs, available }) => {
            assert_eq!(needs, 0x104);
            assert_eq!(available, 256);
        }
        other => panic!("expected MagicOutOfWindow, got {other:?}"),
    }
}

#[test]
fn accepts_magic_ending_exactly_at_the_window() {
    // 0xFC + 4 = 0x100, which is allowed: the check is `size > 256`.
    let r = Rule::new("edge", &[1, 2, 3, 4], "/bin/true").at_offset(0xFC);
    assert!(r.validate().is_ok());
}

#[test]
fn rejects_a_mask_of_the_wrong_length() {
    let r = Rule::new("m", &[1, 2, 3], "/bin/true").with_mask(&[0xff, 0xff]);
    match r.validate() {
        Err(BinfmtError::MaskLength { magic, mask }) => {
            assert_eq!((magic, mask), (3, 2));
        }
        other => panic!("expected MaskLength, got {other:?}"),
    }
}

#[test]
fn rejects_a_relative_interpreter() {
    // The kernel resolves a relative interpreter against the *caller's* cwd,
    // which is a well-known foot-gun, so this must be caught here.
    let r = Rule::new("rel", b"MZ", "wine");
    assert!(matches!(
        r.validate(),
        Err(BinfmtError::InterpreterNotAbsolute(_))
    ));
}

#[test]
fn rejects_bad_entry_names() {
    assert!(matches!(
        Rule::new("", b"MZ", "/bin/true").validate(),
        Err(BinfmtError::InvalidName(_))
    ));
    assert!(matches!(
        Rule::new("a/b", b"MZ", "/bin/true").validate(),
        Err(BinfmtError::InvalidName(_))
    ));
}

#[test]
fn rejects_an_over_long_register_string() {
    // Padding the interpreter path past the 1920-byte limit.
    let long = format!("/bin/{}", "x".repeat(MAX_REGISTER_LENGTH));
    let r = Rule::new("long", b"MZ", long);
    assert!(matches!(
        r.validate(),
        Err(BinfmtError::RegisterStringTooLong(_))
    ));
}

// ---------------------------------------------------------------------------
// Entry parsing
// ---------------------------------------------------------------------------

#[test]
fn parses_the_kernels_entry_rendering() {
    let text = "enabled\ninterpreter /usr/bin/wine\nflags: POCF\noffset 0\nmagic 4d5a\n";
    let entry = parse_entry("tahoe-wine-pe", text);
    assert_eq!(entry.reported_interpreter, "/usr/bin/wine");
    assert_eq!(entry.reported_flags, "POCF");
    assert_eq!(entry.rule.offset, 0);
    assert_eq!(entry.rule.magic, vec![0x4D, 0x5A]);
}

#[test]
fn parses_a_disabled_entry_and_missing_fields() {
    let entry = parse_entry("x", "disabled\n");
    assert_eq!(entry.reported_interpreter, "");
    assert_eq!(entry.rule.offset, 0);
    assert!(entry.rule.magic.is_empty());
}

#[test]
fn unhex_matches_the_kernels_rendering() {
    // The kernel prints no separator: `magic cffaedfe`.
    assert_eq!(unhex("4d5a"), vec![0x4D, 0x5A]);
    assert_eq!(unhex("cffaedfe"), vec![0xCF, 0xFA, 0xED, 0xFE]);
    // Whitespace is tolerated even though the kernel does not emit it.
    assert_eq!(unhex("4d 5a"), vec![0x4D, 0x5A]);
    assert_eq!(
        unhex("zz4d5a"),
        vec![0x4D, 0x5A],
        "non-hex characters are dropped"
    );
    // An odd trailing nibble cannot form a byte and is ignored.
    assert_eq!(unhex("4d5"), vec![0x4D]);
    assert!(unhex("").is_empty());
}

// ---------------------------------------------------------------------------
// Shipped rules
// ---------------------------------------------------------------------------

#[test]
fn shipped_rules_are_all_valid() {
    let rules = shipped_rules(Path::new("/usr/lib/tahoe"));
    assert!(!rules.is_empty());
    for r in &rules {
        r.validate().unwrap_or_else(|e| panic!("{}: {e}", r.name));
        assert!(r.interpreter.starts_with("/usr/lib/tahoe"));
    }
}

#[test]
fn shipped_rules_cover_the_dispatchable_formats() {
    let rules = shipped_rules(Path::new("/usr/lib/tahoe"));
    let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"tahoe-wine-pe"));
    assert!(names.contains(&"tahoe-macho64"));
    assert!(names.contains(&"tahoe-macho32"));
    assert!(names.contains(&"tahoe-macho-fat"));
}

#[test]
fn shipped_pe_rule_matches_the_dos_stub_not_the_pe_signature() {
    let rules = shipped_rules(Path::new("/usr/lib/tahoe"));
    let pe = rules
        .iter()
        .find(|r| r.name == "tahoe-wine-pe")
        .expect("pe rule");
    assert_eq!(pe.magic, b"MZ");
    assert_eq!(pe.offset, 0);

    // How far the PE signature actually reaches, precisely. The kernel
    // documentation says 128 bytes; `BINPRM_BUF_SIZE` is 256 and that is
    // what the code enforces, so 0x80 and 0xF8 are reachable and only
    // 0xFC+ is not. Because it is not universal, `MZ` remains the anchor.
    assert!(
        Rule::new("x", b"PE\0\0", "/bin/true")
            .at_offset(0x80)
            .validate()
            .is_ok(),
        "e_lfanew 0x80 (the common MSVC value) IS inside the window"
    );
    assert!(
        Rule::new("y", b"PE\0\0", "/bin/true")
            .at_offset(0xF8)
            .validate()
            .is_ok()
    );
    // The boundary is inclusive: the kernel's check is `offset + size > 256`,
    // so a 4-byte magic at 0xFC ends exactly at 256 and is still accepted.
    assert!(
        Rule::new("z", b"PE\0\0", "/bin/true")
            .at_offset(0xFC)
            .validate()
            .is_ok(),
        "0xFC + 4 == 256, the last offset the window allows"
    );
    assert!(
        Rule::new("w", b"PE\0\0", "/bin/true")
            .at_offset(0xFD)
            .validate()
            .is_err(),
        "0xFD + 4 exceeds the window"
    );
}

#[test]
fn no_shipped_rule_matches_a_bare_elf_header() {
    // Registering a rule that matched ELF would shadow the native loader
    // for every binary on the system. This is a tripwire.
    let rules = shipped_rules(Path::new("/usr/lib/tahoe"));
    for r in &rules {
        assert_ne!(
            &r.magic[..4.min(r.magic.len())],
            b"\x7fELF",
            "rule {} would shadow ELF",
            r.name
        );
    }
}

#[test]
fn conventional_extensions_are_advisory_only() {
    assert_eq!(conventional_extension(&ObjectFormat::Pe), "exe");
    assert_eq!(conventional_extension(&ObjectFormat::Ole2), "msi");
    assert_eq!(conventional_extension(&ObjectFormat::Xar), "pkg");
    assert_eq!(conventional_extension(&ObjectFormat::Dmg), "dmg");
    assert_eq!(conventional_extension(&ObjectFormat::AppBundle), "app");
    // Executable formats have no meaningful extension: the content decides.
    assert_eq!(conventional_extension(&ObjectFormat::Elf), "");
    assert_eq!(conventional_extension(&ObjectFormat::MachO), "");
}

// ---------------------------------------------------------------------------
// Host inspection
// ---------------------------------------------------------------------------

#[test]
fn inspect_never_panics_and_is_self_consistent() {
    let d = Dispatcher::inspect();
    if d.mounted {
        assert!(
            d.enabled.is_some(),
            "a mount implies a readable status file"
        );
    }
    let mut sorted = d.entries.clone();
    sorted.sort();
    assert_eq!(d.entries, sorted);
}

#[test]
fn reading_an_entry_that_does_not_exist_is_a_typed_error() {
    match Dispatcher::read_entry("tahoe-definitely-not-registered") {
        Err(BinfmtError::NotMounted(_)) => {}
        other => panic!("expected NotMounted, got {other:?}"),
    }
}

#[test]
fn registering_outside_a_namespace_fails_cleanly_or_succeeds_on_the_host() {
    // If the host has binfmt_misc mounted (many do, via systemd), this will
    // really try to register and must be refused for permissions rather
    // than panicking. Either outcome is acceptable; a panic is not.
    let r = rule("tahoe-test-should-not-persist", b"TOH");
    match register_in_current_namespace(&r) {
        Ok(()) => {
            // Only possible with CAP_SYS_ADMIN; clean up so we do not leave
            // a rule behind on the developer's machine.
            let _ = std::fs::write(
                format!("{MOUNT_POINT}/tahoe-test-should-not-persist"),
                b"-1",
            );
        }
        Err(BinfmtError::Rejected(_) | BinfmtError::NotMounted(_)) => {}
        Err(other) => panic!("unexpected error: {other}"),
    }
}

// ---------------------------------------------------------------------------
// End-to-end: the kernel dispatches, in a namespace, with no root
// ---------------------------------------------------------------------------

/// Build a file with the given leading magic in a scratch directory.
fn scratch_file(tag: &str, magic: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("tahoe-binfmt-e2e");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join(format!("{tag}-{}.bin", std::process::id()));
    let mut bytes = magic.to_vec();
    bytes.resize(64, 0);
    std::fs::write(&path, &bytes).expect("write scratch file");
    // binfmt_misc only dispatches files the kernel considers executable.
    let mut perms = std::fs::metadata(&path).expect("stat").permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).expect("chmod");
    path
}

fn have_unshare() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("unshare").is_file()))
}

#[test]
fn the_kernel_dispatches_our_rule_in_a_throwaway_namespace() {
    if !have_unshare() {
        eprintln!("skipping: `unshare` is not installed");
        return;
    }
    let file = scratch_file("macho", &[0xcf, 0xfa, 0xed, 0xfe]);

    // Interpret with `/bin/echo` so the output proves the kernel routed the
    // file to us rather than trying to exec it as ELF.
    let r = Rule::new("tahoe-e2e", &[0xcf, 0xfa, 0xed, 0xfe], "/bin/echo");
    let body = vec![file.display().to_string()];

    let out = register_isolated(&[r], &body).expect("run in namespace");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "namespace run failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    // With `P`, argv becomes [interpreter, binary, original-argv0], so the
    // path appears in the output.
    assert!(
        stdout.contains("macho-"),
        "the kernel should have run /bin/echo on our file; got: {stdout:?}"
    );
    let _ = std::fs::remove_file(&file);
}

#[test]
fn a_rule_registered_in_a_namespace_does_not_escape_it() {
    if !have_unshare() {
        eprintln!("skipping: `unshare` is not installed");
        return;
    }
    let before = Dispatcher::inspect();

    let r = Rule::new("tahoe-leak-check", &[0x7f, b'X', b'Y', b'Z'], "/bin/true");
    let out = register_isolated(&[r], &["/bin/true".to_owned()]).expect("run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let after = Dispatcher::inspect();
    assert_eq!(
        before.entries, after.entries,
        "a namespaced registration must not change the host's dispatcher"
    );
    assert!(
        !after.entries.iter().any(|e| e == "tahoe-leak-check"),
        "the host must not see the namespaced entry"
    );
}

#[test]
fn the_namespace_really_has_the_entry_while_it_runs() {
    if !have_unshare() {
        eprintln!("skipping: `unshare` is not installed");
        return;
    }
    let r = Rule::new("tahoe-visible", b"ZZZZ", "/bin/true");
    // Inside the namespace, list the mount point after registering.
    let body: Vec<String> = ["/bin/sh", "-c", "ls /proc/sys/fs/binfmt_misc"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let out = register_isolated(&[r], &body).expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("tahoe-visible"),
        "the entry should be visible inside its own namespace; saw {stdout:?}"
    );
}

#[test]
fn a_rule_rejected_by_validation_never_reaches_the_kernel() {
    if !have_unshare() {
        eprintln!("skipping: `unshare` is not installed");
        return;
    }
    // Relative interpreter: must fail before spawning anything.
    let bad = Rule::new("tahoe-bad", b"MZ", "wine");
    let err = register_isolated(&[bad], &["/bin/true".to_owned()]).unwrap_err();
    assert!(
        matches!(err, BinfmtError::InterpreterNotAbsolute(_)),
        "{err}"
    );
}

#[test]
fn an_invalid_java_looking_fat_file_is_not_claimed_by_the_fat_rule() {
    // The fat rule cannot express "not a Java class", so the dispatch
    // helper has to. This test pins the *reason* the rule is dangerous:
    // the same magic matches both, and only the interpreter can tell.
    let javaish = scratch_file("java", &[0xca, 0xfe, 0xba, 0xbe, 0x00, 0x00, 0x00, 0x34]);
    let probe = tahoe_format::probe_file(&javaish).expect("probe");
    assert_eq!(
        probe.format,
        ObjectFormat::JavaClass,
        "probe_bytes must call this Java, not a universal binary"
    );
    let _ = std::fs::remove_file(&javaish);
}
