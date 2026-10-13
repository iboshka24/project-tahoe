// SPDX-License-Identifier: MIT OR Apache-2.0
//! Finder tests.
//!
//! Every filesystem test runs inside its own temporary directory. Nothing
//! touches the user's files.

use super::*;
use std::fs;

/// A unique scratch directory, removed when dropped.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir()
            .join("tahoe-finder-tests")
            .join(format!(
                "{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create scratch");
        Self { path }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    fn file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&path, contents).expect("write file");
        path
    }

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.join(name);
        fs::create_dir_all(&path).expect("create dir");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn names(dir: &Directory) -> Vec<String> {
    dir.entries.iter().map(|e| e.name.clone()).collect()
}

// ---------------------------------------------------------------------------
// read_directory
// ---------------------------------------------------------------------------

#[test]
fn lists_directories_before_files_then_sorts_case_insensitively() {
    let s = Scratch::new("sort");
    s.file("b.txt", "b");
    s.file("A.txt", "a");
    s.dir("zeta");
    s.dir("Alpha");
    let dir = read_directory(&s.path).expect("list");
    assert_eq!(names(&dir), vec!["Alpha", "zeta", "A.txt", "b.txt"]);
}

#[test]
fn flags_hidden_entries() {
    let s = Scratch::new("hidden");
    s.file(".bashrc", "x");
    s.file("visible", "y");
    let dir = read_directory(&s.path).expect("list");
    assert!(dir.get(".bashrc").expect("found").hidden);
    assert!(!dir.get("visible").expect("found").hidden);
    assert_eq!(dir.visible().count(), 1);
    assert_eq!(dir.len(), 2);
}

#[test]
fn everything_is_hidden_inside_a_hidden_directory() {
    let s = Scratch::new("hidden-parent");
    s.file(".config/app/conf", "x");
    let inner = read_directory(&s.join(".config/app")).expect("list");
    assert!(inner.get("conf").expect("found").hidden);
}

#[test]
fn reports_size_for_files_but_not_directories() {
    let s = Scratch::new("size");
    s.file("a", "12345");
    s.dir("d");
    let dir = read_directory(&s.path).expect("list");
    assert_eq!(dir.get("a").expect("found").size, Some(5));
    assert_eq!(dir.get("d").expect("found").size, None);
}

#[test]
fn classifies_symlinks_without_following_them() {
    let s = Scratch::new("symlink");
    s.dir("target");
    s.file("plain.txt", "x");
    std::os::unix::fs::symlink(s.join("target"), s.join("link-to-dir")).expect("symlink");
    std::os::unix::fs::symlink(s.join("plain.txt"), s.join("link-to-file")).expect("symlink");

    let dir = read_directory(&s.path).expect("list");
    let to_dir = dir.get("link-to-dir").expect("found");
    assert_eq!(
        to_dir.kind,
        EntryKind::Symlink,
        "a link must not be reported as a directory"
    );
    assert!(
        to_dir.opens_as_directory(),
        "but it must be openable as one"
    );
    assert!(!to_dir.broken);
    assert_eq!(
        dir.get("link-to-file").expect("found").kind,
        EntryKind::Symlink
    );
}

#[test]
fn detects_broken_symlinks() {
    let s = Scratch::new("broken");
    std::os::unix::fs::symlink("/nowhere/at/all", s.join("dangling")).expect("symlink");
    let dir = read_directory(&s.path).expect("list");
    let dangling = dir.get("dangling").expect("found");
    assert!(dangling.broken, "a dangling link must be flagged");
    assert!(!dangling.opens_as_directory());
}

#[test]
fn symlink_to_directory_sorts_with_directories() {
    let s = Scratch::new("sort-links");
    s.dir("real");
    s.file("aaa.txt", "x");
    std::os::unix::fs::symlink(s.join("real"), s.join("zzz-link")).expect("symlink");
    let dir = read_directory(&s.path).expect("list");
    // `real` and `zzz-link` are directories; `aaa.txt` is a file.
    assert_eq!(names(&dir), vec!["real", "zzz-link", "aaa.txt"]);
}

#[test]
fn reading_a_file_as_a_directory_is_an_error() {
    let s = Scratch::new("notdir");
    let file = s.file("a.txt", "x");
    match read_directory(&file) {
        Err(FinderError::NotADirectory(p)) => assert_eq!(p, file),
        other => panic!("expected NotADirectory, got {other:?}"),
    }
}

#[test]
fn reading_a_missing_directory_is_an_error() {
    let s = Scratch::new("missing");
    match read_directory(&s.join("nope")) {
        Err(FinderError::NotFound(_)) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn empty_directory_lists_empty() {
    let s = Scratch::new("empty");
    let dir = read_directory(&s.path).expect("list");
    assert!(dir.is_empty());
    assert_eq!(dir.visible().count(), 0);
}

// ---------------------------------------------------------------------------
// History and Tab
// ---------------------------------------------------------------------------

#[test]
fn history_starts_empty_and_reports_no_navigation() {
    let mut h = History::default();
    assert!(h.is_empty() && !h.can_go_back() && !h.can_go_forward());
    assert_eq!(h.current(), None);
    assert_eq!(h.go_back(), None);
    assert_eq!(h.go_forward(), None);
}

#[test]
fn history_back_and_forward_round_trip() {
    let mut h = History::default();
    h.push("/a".into());
    h.push("/a/b".into());
    h.push("/a/b/c".into());
    assert!(h.can_go_back());
    assert_eq!(h.current(), Some(Path::new("/a/b/c")));
    assert_eq!(h.go_back(), Some(PathBuf::from("/a/b")));
    assert_eq!(h.go_back(), Some(PathBuf::from("/a")));
    assert!(!h.can_go_back(), "we are at the oldest entry");
    assert_eq!(h.go_back(), None);
    assert!(h.can_go_forward());
    assert_eq!(h.go_forward(), Some(PathBuf::from("/a/b")));
    assert_eq!(h.go_forward(), Some(PathBuf::from("/a/b/c")));
    assert_eq!(h.go_forward(), None);
}

#[test]
fn navigating_after_going_back_discards_the_forward_stack() {
    let mut h = History::default();
    h.push("/a".into());
    h.push("/b".into());
    h.go_back();
    assert!(h.can_go_forward());
    h.push("/c".into());
    assert!(
        !h.can_go_forward(),
        "forward must not lead somewhere we never were"
    );
    assert_eq!(h.current(), Some(Path::new("/c")));
}

#[test]
fn tab_navigation_records_history() {
    let s = Scratch::new("tab");
    s.dir("one");
    s.dir("one/two");
    let mut tab = Tab::open(&s.path).expect("open");
    assert_eq!(tab.path(), s.path.as_path());

    tab.navigate(&s.join("one")).expect("navigate");
    assert_eq!(tab.path(), s.join("one").as_path());
    tab.navigate(&s.join("one/two")).expect("navigate");

    assert!(tab.back().expect("back"));
    assert_eq!(tab.path(), s.join("one").as_path());
    assert!(tab.forward().expect("forward"));
    assert_eq!(tab.path(), s.join("one/two").as_path());
}

#[test]
fn tab_up_walks_to_the_parent() {
    let s = Scratch::new("up");
    s.dir("a/b/c");
    let mut tab = Tab::open(&s.join("a/b/c")).expect("open");
    assert!(tab.up().expect("up"));
    assert_eq!(tab.path(), s.join("a/b").as_path());
    assert!(tab.up().expect("up"));
    assert_eq!(tab.path(), s.join("a").as_path());
}

#[test]
fn tab_up_from_the_root_reports_no_movement() {
    let mut tab = Tab::open(Path::new("/")).expect("open root");
    assert!(!tab.up().expect("up at root"), "the root has no parent");
    assert_eq!(tab.path(), Path::new("/"));
}

#[test]
fn failed_navigation_leaves_the_tab_where_it_was() {
    let s = Scratch::new("failnav");
    let mut tab = Tab::open(&s.path).expect("open");
    let before = tab.path().to_path_buf();
    assert!(tab.navigate(&s.join("does-not-exist")).is_err());
    assert_eq!(
        tab.path(),
        before,
        "a failed navigation must not move the tab"
    );
    assert_eq!(tab.history.len(), 1, "and must not pollute the history");
}

#[test]
fn back_at_the_oldest_entry_reports_no_movement() {
    let s = Scratch::new("backstop");
    let mut tab = Tab::open(&s.path).expect("open");
    assert!(!tab.back().expect("back"));
    assert!(!tab.forward().expect("forward"));
}

// ---------------------------------------------------------------------------
// XDG user directories and sidebar
// ---------------------------------------------------------------------------

const USER_DIRS: &str = r#"
# This file is written by xdg-user-dirs-update
XDG_DESKTOP_DIR="$HOME/Desktop"
XDG_DOWNLOAD_DIR="$HOME/Downloads"
XDG_TEMPLATES_DIR="$HOME/Templates"
XDG_DOCUMENTS_DIR="/absolute/Documents"
XDG_MUSIC_DIR="$HOME/Music"
XDG_PICTURES_DIR="$HOME/Pictures"
XDG_VIDEOS_DIR="$HOME/Videos"
UNRELATED="$HOME/ignored"
"#;

#[test]
fn parses_xdg_user_dirs() {
    let home = Path::new("/home/u");
    let dirs = XdgUserDirs::parse(USER_DIRS, home);
    assert_eq!(dirs.desktop, Some(PathBuf::from("/home/u/Desktop")));
    assert_eq!(dirs.download, Some(PathBuf::from("/home/u/Downloads")));
    assert_eq!(dirs.music, Some(PathBuf::from("/home/u/Music")));
    assert_eq!(dirs.pictures, Some(PathBuf::from("/home/u/Pictures")));
    assert_eq!(dirs.videos, Some(PathBuf::from("/home/u/Videos")));
    // Absolute values are kept as written.
    assert_eq!(dirs.documents, Some(PathBuf::from("/absolute/Documents")));
}

#[test]
fn ignores_comments_blanks_and_unrelated_keys() {
    let dirs = XdgUserDirs::parse(
        "\n# c\n\nBROKEN\nXDG_MUSIC_DIR=\"$HOME/M\"\n",
        Path::new("/h"),
    );
    assert_eq!(dirs.music, Some(PathBuf::from("/h/M")));
    assert_eq!(dirs.desktop, None);
}

#[test]
fn rejects_relative_values() {
    let dirs = XdgUserDirs::parse("XDG_DESKTOP_DIR=\"relative/path\"\n", Path::new("/h"));
    assert_eq!(dirs.desktop, None, "a relative XDG dir is not meaningful");
}

#[test]
fn sidebar_marks_missing_places_without_hiding_them() {
    let s = Scratch::new("sidebar");
    s.dir("real");
    let dirs = XdgUserDirs {
        desktop: Some(s.join("real")),
        documents: Some(s.join("absent")),
        ..XdgUserDirs::default()
    };
    let sidebar = Sidebar::from_parts(&s.path, &dirs);
    let labels: Vec<_> = sidebar.places.iter().map(|p| p.label.as_str()).collect();
    assert!(labels.contains(&"Home"));
    assert!(labels.contains(&"Desktop"));
    assert!(labels.contains(&"Documents"));
    assert!(labels.contains(&"Filesystem"));
    assert!(
        sidebar
            .places
            .iter()
            .find(|p| p.label == "Desktop")
            .expect("desktop")
            .exists
    );
    assert!(
        !sidebar
            .places
            .iter()
            .find(|p| p.label == "Documents")
            .expect("documents")
            .exists
    );
    assert_eq!(sidebar.available().count(), 3, "Home, Desktop, Filesystem");
}

#[test]
fn sidebar_omits_directories_the_config_does_not_mention() {
    let dirs = XdgUserDirs::default();
    let sidebar = Sidebar::from_parts(Path::new("/home/u"), &dirs);
    let labels: Vec<_> = sidebar.places.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(labels, vec!["Home", "Filesystem"]);
}

#[test]
fn sidebar_ends_with_the_filesystem_root() {
    let sidebar = Sidebar::from_environment();
    assert_eq!(sidebar.places.last().expect("last").path, Path::new("/"));
}

// ---------------------------------------------------------------------------
// copy
// ---------------------------------------------------------------------------

#[test]
fn copies_a_file() {
    let s = Scratch::new("copyfile");
    let src = s.file("a.txt", "hello");
    let dst = s.join("b.txt");
    assert_eq!(copy(&src, &dst, Overwrite::Refuse).expect("copy"), 5);
    assert_eq!(fs::read_to_string(&dst).expect("read"), "hello");
    assert!(src.exists(), "the source must survive a copy");
}

#[test]
fn refuses_to_overwrite_by_default() {
    let s = Scratch::new("nooverwrite");
    let src = s.file("a.txt", "new");
    let dst = s.file("b.txt", "old");
    match copy(&src, &dst, Overwrite::Refuse) {
        Err(FinderError::DestinationExists(p)) => assert_eq!(p, dst),
        other => panic!("expected DestinationExists, got {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(&dst).expect("read"),
        "old",
        "the file must be untouched"
    );
}

#[test]
fn replaces_when_asked() {
    let s = Scratch::new("overwrite");
    let src = s.file("a.txt", "new");
    let dst = s.file("b.txt", "old");
    copy(&src, &dst, Overwrite::Replace).expect("copy");
    assert_eq!(fs::read_to_string(&dst).expect("read"), "new");
}

#[test]
fn copies_a_directory_tree_recursively() {
    let s = Scratch::new("copytree");
    s.file("tree/a.txt", "a");
    s.file("tree/sub/b.txt", "bb");
    s.file("tree/sub/deep/c.txt", "ccc");
    let dst = s.join("copy");
    let copied = copy(&s.join("tree"), &dst, Overwrite::Refuse).expect("copy");
    assert_eq!(copied, 6, "3 files plus 3 directories");
    assert_eq!(fs::read_to_string(dst.join("a.txt")).expect("a"), "a");
    assert_eq!(fs::read_to_string(dst.join("sub/b.txt")).expect("b"), "bb");
    assert_eq!(
        fs::read_to_string(dst.join("sub/deep/c.txt")).expect("c"),
        "ccc"
    );
}

#[test]
fn refuses_to_copy_a_directory_into_itself() {
    let s = Scratch::new("intoself");
    s.file("tree/a.txt", "a");
    let tree = s.join("tree");
    match copy(&tree, &tree.join("inner"), Overwrite::Refuse) {
        Err(FinderError::IntoItself { source, .. }) => assert_eq!(source, tree),
        other => panic!("expected IntoItself, got {other:?}"),
    }
    assert!(!tree.join("inner").exists(), "nothing may be created");
}

#[test]
fn refuses_to_copy_a_directory_into_its_own_subtree() {
    let s = Scratch::new("intosubtree");
    s.dir("tree/sub");
    let tree = s.join("tree");
    // The classic disk-filler: even a nested destination must be refused.
    assert!(matches!(
        copy(&tree, &tree.join("sub/again"), Overwrite::Refuse),
        Err(FinderError::IntoItself { .. })
    ));
}

#[test]
fn copies_symlinks_as_symlinks_without_following() {
    let s = Scratch::new("copylink");
    s.file("target.txt", "content");
    std::os::unix::fs::symlink(s.join("target.txt"), s.join("link")).expect("symlink");
    copy(&s.join("link"), &s.join("copied-link"), Overwrite::Refuse).expect("copy");
    let meta = fs::symlink_metadata(s.join("copied-link")).expect("stat");
    assert!(meta.file_type().is_symlink(), "a link must stay a link");
    assert_eq!(
        fs::read_link(s.join("copied-link")).expect("readlink"),
        s.join("target.txt")
    );
}

#[test]
fn copying_a_missing_source_fails_cleanly() {
    let s = Scratch::new("copymissing");
    match copy(&s.join("nope"), &s.join("dst"), Overwrite::Refuse) {
        Err(FinderError::NotFound(p)) => assert_eq!(p, s.join("nope")),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn copying_a_file_onto_a_directory_is_refused() {
    let s = Scratch::new("file-onto-dir");
    let src = s.file("a.txt", "x");
    let dst = s.dir("adir");
    assert!(matches!(
        copy(&src, &dst, Overwrite::Replace),
        Err(FinderError::IsADirectory(_))
    ));
}

// ---------------------------------------------------------------------------
// move_entry
// ---------------------------------------------------------------------------

#[test]
fn moves_a_file_by_rename() {
    let s = Scratch::new("movefile");
    let src = s.file("a.txt", "content");
    let dst = s.join("moved.txt");
    assert_eq!(move_entry(&src, &dst, Overwrite::Refuse).expect("move"), 1);
    assert!(!src.exists(), "the source must be gone after a move");
    assert_eq!(fs::read_to_string(&dst).expect("read"), "content");
}

#[test]
fn moves_a_directory() {
    let s = Scratch::new("movedir");
    s.file("tree/a.txt", "a");
    s.file("tree/sub/b.txt", "b");
    let dst = s.join("moved-tree");
    move_entry(&s.join("tree"), &dst, Overwrite::Refuse).expect("move");
    assert!(!s.join("tree").exists());
    assert_eq!(
        fs::read_to_string(dst.join("sub/b.txt")).expect("read"),
        "b"
    );
}

#[test]
fn refuses_to_move_a_directory_into_itself() {
    let s = Scratch::new("moveintoself");
    s.dir("tree");
    assert!(matches!(
        move_entry(&s.join("tree"), &s.join("tree/child"), Overwrite::Refuse),
        Err(FinderError::IntoItself { .. })
    ));
    assert!(s.join("tree").exists(), "the source must survive a refusal");
}

#[test]
fn refuses_to_overwrite_on_move() {
    let s = Scratch::new("movetwice");
    let src = s.file("a.txt", "new");
    s.file("b.txt", "old");
    assert!(matches!(
        move_entry(&src, &s.join("b.txt"), Overwrite::Refuse),
        Err(FinderError::DestinationExists(_))
    ));
    assert!(src.exists());
    assert_eq!(fs::read_to_string(s.join("b.txt")).expect("read"), "old");
}

#[test]
fn moving_a_missing_source_fails_cleanly() {
    let s = Scratch::new("movemissing");
    assert!(matches!(
        move_entry(&s.join("nope"), &s.join("dst"), Overwrite::Refuse),
        Err(FinderError::NotFound(_))
    ));
}

// ---------------------------------------------------------------------------
// delete
// ---------------------------------------------------------------------------

#[test]
fn deletes_a_file() {
    let s = Scratch::new("delfile");
    let f = s.file("a.txt", "x");
    assert_eq!(delete(&f).expect("delete"), 1);
    assert!(!f.exists());
}

#[test]
fn deletes_a_tree_and_counts_everything() {
    let s = Scratch::new("deltree");
    s.file("tree/a.txt", "a");
    s.file("tree/sub/b.txt", "b");
    // 3 filesystem objects: two files and the directory itself... plus `sub`.
    assert_eq!(delete(&s.join("tree")).expect("delete"), 4);
    assert!(!s.join("tree").exists());
}

#[test]
fn deleting_a_symlink_removes_the_link_not_the_target() {
    let s = Scratch::new("dellink");
    let target = s.file("target.txt", "keep me");
    std::os::unix::fs::symlink(&target, s.join("link")).expect("symlink");
    delete(&s.join("link")).expect("delete");
    assert!(!s.join("link").exists());
    assert!(target.exists(), "the target must survive");
}

#[test]
fn refuses_to_delete_the_root() {
    assert!(matches!(
        delete(Path::new("/")),
        Err(FinderError::UnsafePath(_))
    ));
}

#[test]
fn refuses_a_path_containing_parent_dir() {
    let s = Scratch::new("unsafe");
    let sneaky = s.path.join("..").join("escape");
    assert!(matches!(delete(&sneaky), Err(FinderError::UnsafePath(_))));
    assert!(matches!(
        copy(&sneaky, &s.join("x"), Overwrite::Refuse),
        Err(FinderError::UnsafePath(_))
    ));
}

#[test]
fn deleting_a_missing_path_fails_cleanly() {
    let s = Scratch::new("delmissing");
    assert!(matches!(
        delete(&s.join("nope")),
        Err(FinderError::NotFound(_))
    ));
}

// ---------------------------------------------------------------------------
// sizes and pending deletes
// ---------------------------------------------------------------------------

#[test]
fn tree_size_sums_file_sizes() {
    let s = Scratch::new("size");
    s.file("tree/a.txt", "12345");
    s.file("tree/sub/b.txt", "123");
    assert_eq!(tree_size(&s.join("tree")).expect("size"), 8);
}

#[test]
fn tree_size_of_a_file_is_its_length() {
    let s = Scratch::new("sizeone");
    let f = s.file("a.txt", "1234");
    assert_eq!(tree_size(&f).expect("size"), 4);
}

#[test]
fn tree_entry_count_counts_directories_too() {
    let s = Scratch::new("count");
    s.file("tree/a.txt", "a");
    s.file("tree/sub/b.txt", "b");
    // tree, sub, a.txt, b.txt
    assert_eq!(tree_entry_count(&s.join("tree")).expect("count"), 4);
    assert_eq!(tree_entry_count(&s.file("single", "")).expect("count"), 1);
}

#[test]
fn pending_delete_describes_before_it_acts() {
    let s = Scratch::new("pending");
    s.file("tree/a.txt", "12345");
    s.file("tree/sub/b.txt", "123");
    let pending = PendingDelete::describe(&s.join("tree")).expect("describe");
    assert_eq!(pending.entries, 4);
    assert_eq!(pending.bytes, 8);
    assert!(pending.path.exists(), "describing must not delete anything");
    assert!(pending.prompt().contains("3 items"), "{}", pending.prompt());

    pending.confirm().expect("confirm");
    assert!(!s.join("tree").exists());
}

#[test]
fn pending_delete_prompt_is_specific_for_a_single_file() {
    let s = Scratch::new("pendingsingle");
    let pending = PendingDelete::describe(&s.file("a.txt", "x")).expect("describe");
    assert_eq!(
        pending.prompt(),
        format!("Delete {}?", s.join("a.txt").display())
    );
}

// ---------------------------------------------------------------------------
// OperationQueue
// ---------------------------------------------------------------------------

#[test]
fn queue_runs_in_order_and_stops_when_empty() {
    let s = Scratch::new("queue");
    let a = s.file("a.txt", "a");
    let b = s.file("b.txt", "b");
    let mut queue = OperationQueue::new();
    assert!(queue.is_empty() && queue.run_next().is_none());
    queue.push(Operation::Copy {
        source: a.clone(),
        destination: s.join("a-copy"),
    });
    queue.push(Operation::Move {
        source: b.clone(),
        destination: s.join("b-moved"),
    });
    assert_eq!(queue.len(), 2);

    assert!(queue.run_next().expect("first").is_ok());
    assert_eq!(fs::read_to_string(s.join("a-copy")).expect("read"), "a");
    assert!(a.exists(), "a copy leaves the source alone");

    assert!(queue.run_next().expect("second").is_ok());
    assert!(!b.exists());
    assert!(queue.run_next().is_none());
}

#[test]
fn queue_surfaces_a_failure_and_continues_with_the_next_item() {
    let s = Scratch::new("queuefail");
    let good = s.file("good.txt", "g");
    let mut queue = OperationQueue::new();
    queue.push(Operation::Copy {
        source: s.join("missing"),
        destination: s.join("x"),
    });
    queue.push(Operation::Copy {
        source: good,
        destination: s.join("y"),
    });

    assert!(matches!(
        queue.run_next().expect("first"),
        Err(FinderError::NotFound(_))
    ));
    assert!(
        queue.run_next().expect("second").is_ok(),
        "one failure must not block the queue"
    );
    assert!(s.join("y").exists());
}

#[test]
fn queue_refuses_to_overwrite() {
    let s = Scratch::new("queueoverwrite");
    let src = s.file("a.txt", "new");
    s.file("b.txt", "old");
    let mut queue = OperationQueue::new();
    queue.push(Operation::Copy {
        source: src,
        destination: s.join("b.txt"),
    });
    assert!(matches!(
        queue.run_next().expect("run"),
        Err(FinderError::DestinationExists(_))
    ));
    assert_eq!(fs::read_to_string(s.join("b.txt")).expect("read"), "old");
}

// ---------------------------------------------------------------------------
// errors
// ---------------------------------------------------------------------------

#[test]
fn every_error_names_the_path_it_concerns() {
    let p = PathBuf::from("/some/path");
    for error in [
        FinderError::NotFound(p.clone()),
        FinderError::PermissionDenied(p.clone()),
        FinderError::NotADirectory(p.clone()),
        FinderError::DestinationExists(p.clone()),
        FinderError::IsADirectory(p.clone()),
        FinderError::UnsafePath(p.clone()),
        FinderError::Io {
            path: p.clone(),
            message: "boom".into(),
        },
    ] {
        assert_eq!(error.path(), p.as_path());
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn into_itself_error_names_the_destination() {
    let error = FinderError::IntoItself {
        source: PathBuf::from("/a"),
        destination: PathBuf::from("/a/b"),
    };
    assert_eq!(error.path(), Path::new("/a/b"));
    assert!(error.to_string().contains("into itself"));
}
