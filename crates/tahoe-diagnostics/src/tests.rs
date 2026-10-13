// SPDX-License-Identifier: MIT OR Apache-2.0
//! Parsing unit tests (synthetic strings) plus read-only live probes that
//! assert shape, not values — they must pass on any machine.

use super::*;

#[test]
fn kernel_version_parses() {
    assert_eq!(
        parse_kernel_version("6.18.55-1-cachyos-lts"),
        Some((6, 18, 55))
    );
    assert_eq!(parse_kernel_version("6.14.0-arch1-1"), Some((6, 14, 0)));
    assert_eq!(parse_kernel_version("5.15.216"), Some((5, 15, 216)));
}

#[test]
fn kernel_version_rejects_garbage() {
    assert_eq!(parse_kernel_version(""), None);
    assert_eq!(parse_kernel_version("not-a-kernel"), None);
    assert_eq!(parse_kernel_version("6.18"), None);
}

#[test]
fn binfmt_status_parses() {
    assert_eq!(parse_binfmt_status("enabled"), Some(true));
    assert_eq!(parse_binfmt_status("enabled\n"), Some(true));
    assert_eq!(parse_binfmt_status("disabled"), Some(false));
    assert_eq!(parse_binfmt_status(""), None);
    assert_eq!(parse_binfmt_status("bogus"), None);
}

#[test]
fn ntsync_threshold_is_6_14() {
    assert!("6.14.0".split('.').count() == 3); // sanity, threshold below is real logic
    let capable = |r: &str| parse_kernel_version(r).is_some_and(|(a, b, _)| (a, b) >= (6, 14));
    assert!(capable("6.14.0-x"));
    assert!(capable("6.18.55-1"));
    assert!(capable("7.1.6"));
    assert!(!capable("6.13.9"));
    assert!(!capable("5.15.216"));
}

#[test]
fn live_binfmt_probe_returns_shape() {
    let s = probe_binfmt();
    // Mounted implies the status file was readable; unmounted implies not.
    if s.mounted {
        assert!(s.enabled.is_some());
    }
    let mut sorted = s.entries.clone();
    sorted.sort();
    assert_eq!(s.entries, sorted);
}

#[test]
fn live_layers_cover_known_names() {
    let layers = probe_layers();
    let names: Vec<_> = layers.iter().map(|l| l.name).collect();
    assert!(names.contains(&"wine"));
    assert!(names.contains(&"darling"));
    for l in &layers {
        if let Some(p) = &l.path {
            assert!(p.is_file(), "{} entry is not a file", l.name);
        }
    }
}

#[test]
fn live_kernel_probe_parses_or_reports_empty() {
    let k = probe_kernel();
    if !k.release.is_empty() {
        assert!(
            k.version.is_some(),
            "non-empty release should parse: {}",
            k.release
        );
    }
}

#[test]
fn live_collect_holds_together() {
    let r = collect();
    assert_eq!(r.layers.len(), 6);
}
