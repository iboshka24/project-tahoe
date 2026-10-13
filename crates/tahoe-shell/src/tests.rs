// SPDX-License-Identifier: MIT OR Apache-2.0
//! Model tests: dock, workspaces, `.desktop` parsing, launcher search.

use super::*;

fn win(id: u64, app: &str, ws: usize) -> WindowRecord {
    WindowRecord {
        id,
        title: format!("win{id}"),
        app: Some(AppId::new(app)),
        workspace: ws,
        focused: false,
    }
}

#[test]
fn dock_pin_is_idempotent() {
    let mut d = Dock::default();
    d.pin(AppId::new("foot"));
    d.pin(AppId::new("foot"));
    assert_eq!(d.pinned, vec![AppId::new("foot")]);
}

#[test]
fn dock_unpin_reports() {
    let mut d = Dock::default();
    assert!(!d.unpin(&AppId::new("ghost")));
    d.pin(AppId::new("foot"));
    assert!(d.unpin(&AppId::new("foot")));
    assert!(d.pinned.is_empty());
}

#[test]
fn dock_sync_counts_windows() {
    let mut d = Dock::default();
    d.sync_running(&[win(1, "foot", 0), win(2, "foot", 0), win(3, "ff", 1)]);
    assert_eq!(d.running[&AppId::new("foot")], 2);
    assert_eq!(d.running[&AppId::new("ff")], 1);
}

#[test]
fn dock_sync_ignores_unknown_app() {
    let mut d = Dock::default();
    let mut w = win(1, "foot", 0);
    w.app = None;
    d.sync_running(&[w]);
    assert!(d.running.is_empty());
}

#[test]
fn workspaces_default_first_active() {
    let w = Workspaces::new(4);
    assert_eq!(w.names, ["1", "2", "3", "4"]);
    assert_eq!(w.active, 0);
}

#[test]
fn workspaces_zero_count_clamps_to_one() {
    assert_eq!(Workspaces::new(0).names.len(), 1);
}

#[test]
fn workspaces_switch_bounds() {
    let mut w = Workspaces::new(2);
    w.switch(1);
    assert_eq!(w.active, 1);
    w.switch(99);
    assert_eq!(w.active, 1);
}

#[test]
fn workspaces_move_window() {
    let w = Workspaces::new(3);
    let mut wins = vec![win(7, "foot", 0)];
    assert!(w.move_window(&mut wins, 7, 2));
    assert_eq!(wins[0].workspace, 2);
    assert!(!w.move_window(&mut wins, 7, 9));
    assert!(!w.move_window(&mut wins, 4242, 1));
}

#[test]
fn desktop_parse_minimal() {
    let e = parse_desktop_entry("foot", "[Desktop Entry]\nName=Foot\nExec=foot\n").unwrap();
    assert_eq!(e.name, "Foot");
    assert_eq!(e.exec, "foot");
    assert!(!e.terminal);
}

#[test]
fn desktop_parse_full() {
    let content = "# comment\n[Desktop Entry]\nType=Application\nName=Files\nExec=tahoe-files %U\nIcon=folder\nTerminal=false\nNoDisplay=false\n";
    let e = parse_desktop_entry("files", content).unwrap();
    assert_eq!(e.icon.as_deref(), Some("folder"));
    assert!(!e.hidden);
}

#[test]
fn desktop_parse_hidden() {
    let e = parse_desktop_entry("x", "[Desktop Entry]\nName=X\nExec=x\nNoDisplay=true\n").unwrap();
    assert!(e.hidden);
}

#[test]
fn desktop_parse_rejects_non_application() {
    assert!(parse_desktop_entry("l", "[Desktop Entry]\nType=Link\nName=L\nExec=x\n").is_none());
}

#[test]
fn desktop_parse_rejects_missing_exec() {
    assert!(parse_desktop_entry("n", "[Desktop Entry]\nName=N\n").is_none());
}

#[test]
fn desktop_parse_ignores_other_groups() {
    let e = parse_desktop_entry(
        "a",
        "[Desktop Action X]\nName=Wrong\n[Desktop Entry]\nName=Right\nExec=run\n",
    )
    .unwrap();
    assert_eq!(e.name, "Right");
}

#[test]
fn launcher_search_matches_name_and_id() {
    let idx = LauncherIndex {
        entries: vec![
            parse_desktop_entry(
                "org.foot",
                "[Desktop Entry]\nName=Foot Terminal\nExec=foot\n",
            )
            .unwrap(),
            parse_desktop_entry(
                "ff",
                "[Desktop Entry]\nName=Browser\nExec=ff\nNoDisplay=true\n",
            )
            .unwrap(),
        ],
    };
    assert_eq!(idx.search("foot").len(), 1);
    assert_eq!(idx.search("TERMINAL").len(), 1);
    // Hidden entries never surface.
    assert!(idx.search("browser").is_empty());
    assert_eq!(idx.search("").len(), 1);
}

#[test]
fn appid_trims() {
    assert_eq!(AppId::new("  a  ").0, "a");
}
