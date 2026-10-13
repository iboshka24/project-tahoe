// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tahoe Finder: the file manager's model, without any rendering.
//!
//! Everything here is testable against a real temporary directory. The
//! pieces:
//!
//! - [`read_directory`] — a listing with deterministic ordering, hidden-file
//!   flagging and **non-followed** symlink classification.
//! - [`History`] / [`Tab`] — back, forward and up with correct stack
//!   semantics (a new navigation truncates the forward stack, as every
//!   browser does).
//! - [`Sidebar`] — the real XDG user directories, parsed from
//!   `user-dirs.dirs`, not a hard-coded list.
//! - [`copy`], [`move_entry`], [`delete`] — real filesystem operations that
//!   refuse to do the dangerous thing, and report *which path* failed.
//!
//! The dangerous things, explicitly refused:
//! - overwriting an existing destination without [`Overwrite::Replace`];
//! - copying or moving a directory into itself or into its own subtree,
//!   which would otherwise recurse until the disk fills;
//! - deleting the root, or any path with `..` in it.
//!
//! What is **not** here: a trash implementation. Deleting is real deletion;
//! a Freedesktop trash backend is future work and is called out in
//! `docs/limitations.md` rather than half-done.

use std::{
    collections::VecDeque,
    fs, io,
    path::{Component, Path, PathBuf},
};

/// What an entry is. Symlinks are reported as symlinks, never resolved —
/// following them during a listing is how a file manager ends up showing a
/// different tree than the one the user is standing in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    /// A directory.
    Directory,
    /// A regular file.
    File,
    /// A symbolic link (whatever it points at).
    Symlink,
    /// A socket, fifo, device node, or anything else.
    Other,
}

impl EntryKind {
    /// Classify a path without following symlinks.
    #[must_use]
    pub fn of(path: &Path) -> Self {
        // `symlink_metadata` does not follow links, which is the whole point.
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => Self::Symlink,
            Ok(meta) if meta.is_dir() => Self::Directory,
            Ok(meta) if meta.is_file() => Self::File,
            _ => Self::Other,
        }
    }
}

/// One row in a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// File name, without the directory part.
    pub name: String,
    /// Full path.
    pub path: PathBuf,
    /// What it is.
    pub kind: EntryKind,
    /// Size in bytes, when it is a regular file.
    pub size: Option<u64>,
    /// Leading dot, or inside a hidden directory.
    pub hidden: bool,
    /// A symlink whose target does not exist.
    pub broken: bool,
}

impl Entry {
    /// True for a directory *or* a symlink that resolves to one, which is
    /// what a "can I open this?" check needs (as opposed to
    /// [`Entry::kind`], which reports the link itself).
    #[must_use]
    pub fn opens_as_directory(&self) -> bool {
        match self.kind {
            EntryKind::Directory => true,
            EntryKind::Symlink => fs::metadata(&self.path).is_ok_and(|m| m.is_dir()),
            _ => false,
        }
    }
}

/// A directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    /// The directory that was read.
    pub path: PathBuf,
    /// Entries, directories first then files, each case-insensitively sorted.
    pub entries: Vec<Entry>,
}

impl Directory {
    /// Entries with hidden ones removed, as the UI shows by default.
    pub fn visible(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| !e.hidden)
    }

    /// Number of entries, hidden or not.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the directory is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look an entry up by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

/// Why a Finder operation failed, always carrying the path involved.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FinderError {
    /// The path does not exist.
    NotFound(PathBuf),
    /// The OS refused access.
    PermissionDenied(PathBuf),
    /// The path is a file where a directory was needed.
    NotADirectory(PathBuf),
    /// The destination already exists and replacement was not requested.
    DestinationExists(PathBuf),
    /// A directory was required where a file was given.
    IsADirectory(PathBuf),
    /// The operation would recurse into itself.
    IntoItself {
        /// The directory being copied or moved.
        source: PathBuf,
        /// The destination that lies inside it.
        destination: PathBuf,
    },
    /// The path contains `..` or is the filesystem root.
    UnsafePath(PathBuf),
    /// Anything else, with the OS message preserved.
    Io {
        /// Path involved.
        path: PathBuf,
        /// What the OS said.
        message: String,
    },
}

impl FinderError {
    /// The path the failure concerns.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::NotFound(p)
            | Self::PermissionDenied(p)
            | Self::NotADirectory(p)
            | Self::DestinationExists(p)
            | Self::IsADirectory(p)
            | Self::UnsafePath(p) => p,
            Self::IntoItself { destination, .. } => destination,
            Self::Io { path, .. } => path,
        }
    }

    fn from_io(path: &Path, error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::NotFound(path.to_path_buf()),
            io::ErrorKind::PermissionDenied => Self::PermissionDenied(path.to_path_buf()),
            io::ErrorKind::NotADirectory => Self::NotADirectory(path.to_path_buf()),
            io::ErrorKind::IsADirectory => Self::IsADirectory(path.to_path_buf()),
            _ => Self::Io {
                path: path.to_path_buf(),
                message: error.to_string(),
            },
        }
    }
}

impl std::fmt::Display for FinderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(p) => write!(f, "{} does not exist", p.display()),
            Self::PermissionDenied(p) => write!(f, "permission denied: {}", p.display()),
            Self::NotADirectory(p) => write!(f, "not a directory: {}", p.display()),
            Self::DestinationExists(p) => write!(f, "{} already exists", p.display()),
            Self::IsADirectory(p) => write!(f, "{} is a directory", p.display()),
            Self::IntoItself {
                source,
                destination,
            } => write!(
                f,
                "cannot copy or move {} into itself ({})",
                source.display(),
                destination.display()
            ),
            Self::UnsafePath(p) => write!(f, "refusing to operate on {}", p.display()),
            Self::Io { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for FinderError {}

/// Read a directory into a sorted listing.
///
/// Directories sort before files, then case-insensitively by name. Entries
/// that vanish between `read_dir` and `symlink_metadata` are skipped rather
/// than failing the whole listing — a directory being written to is normal.
///
/// # Errors
///
/// Returns [`FinderError`] when the path is not a readable directory.
pub fn read_directory(path: &Path) -> Result<Directory, FinderError> {
    let meta = fs::metadata(path).map_err(|e| FinderError::from_io(path, &e))?;
    if !meta.is_dir() {
        return Err(FinderError::NotADirectory(path.to_path_buf()));
    }

    // Anything inside a hidden directory is hidden, no matter how deep.
    // This is pure string work on the path components, so no extra syscall
    // per listing and no dependence on where `HOME` is.
    let inside_hidden_dir = path
        .components()
        .any(|c| matches!(c, Component::Normal(name) if name.to_string_lossy().starts_with('.')));

    let mut entries = Vec::new();
    let read = fs::read_dir(path).map_err(|e| FinderError::from_io(path, &e))?;
    for item in read {
        let Ok(item) = item else { continue };
        let entry_path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        let Ok(link_meta) = fs::symlink_metadata(&entry_path) else {
            continue; // vanished mid-listing
        };
        let kind = if link_meta.file_type().is_symlink() {
            EntryKind::Symlink
        } else if link_meta.is_dir() {
            EntryKind::Directory
        } else if link_meta.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let broken = kind == EntryKind::Symlink && fs::metadata(&entry_path).is_err();
        let size = (kind == EntryKind::File).then_some(link_meta.len());
        entries.push(Entry {
            hidden: name.starts_with('.') || inside_hidden_dir,
            name,
            path: entry_path,
            kind,
            size,
            broken,
        });
    }

    entries.sort_by(|a, b| {
        let rank = |e: &Entry| match e.kind {
            EntryKind::Directory => 0,
            EntryKind::Symlink if e.opens_as_directory() => 0,
            _ => 1,
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(Directory {
        path: path.to_path_buf(),
        entries,
    })
}

// ---------------------------------------------------------------------------
// Navigation history
// ---------------------------------------------------------------------------

/// Back/forward history for one tab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
}

impl History {
    /// Record a new location.
    ///
    /// Navigating after going back discards the forward stack, exactly as a
    /// browser does — otherwise "forward" would lead somewhere the user
    /// never was from here.
    pub fn push(&mut self, path: PathBuf) {
        self.forward.clear();
        self.back.push(path);
    }

    /// The current location, if any.
    #[must_use]
    pub fn current(&self) -> Option<&Path> {
        self.back.last().map(PathBuf::as_path)
    }

    /// Step back. Returns the location to open.
    pub fn go_back(&mut self) -> Option<PathBuf> {
        if self.back.len() < 2 {
            return None;
        }
        let current = self.back.pop()?;
        self.forward.push(current);
        self.current().map(Path::to_path_buf)
    }

    /// Step forward. Returns the location to open.
    pub fn go_forward(&mut self) -> Option<PathBuf> {
        let next = self.forward.pop()?;
        self.back.push(next.clone());
        Some(next)
    }

    /// True when there is somewhere to go back to.
    #[must_use]
    pub fn can_go_back(&self) -> bool {
        self.back.len() > 1
    }

    /// True when there is somewhere to go forward to.
    #[must_use]
    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// How many entries the history holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.back.len()
    }

    /// True when nothing has been visited.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.back.is_empty()
    }
}

/// One Finder tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    /// Navigation history.
    pub history: History,
    /// The listing currently shown.
    pub listing: Directory,
}

impl Tab {
    /// Open a directory in a fresh tab.
    ///
    /// # Errors
    ///
    /// Propagates [`read_directory`] failures.
    pub fn open(path: &Path) -> Result<Self, FinderError> {
        let listing = read_directory(path)?;
        let mut history = History::default();
        history.push(listing.path.clone());
        Ok(Self { history, listing })
    }

    /// The directory being shown.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.listing.path
    }

    /// Navigate to `path`, recording history.
    ///
    /// # Errors
    ///
    /// Propagates [`read_directory`] failures; the tab is left unchanged.
    pub fn navigate(&mut self, path: &Path) -> Result<(), FinderError> {
        let listing = read_directory(path)?;
        self.history.push(listing.path.clone());
        self.listing = listing;
        Ok(())
    }

    /// Go back one step, if possible.
    ///
    /// # Errors
    ///
    /// Propagates [`read_directory`] failures.
    pub fn back(&mut self) -> Result<bool, FinderError> {
        let Some(target) = self.history.go_back() else {
            return Ok(false);
        };
        self.listing = read_directory(&target)?;
        Ok(true)
    }

    /// Go forward one step, if possible.
    ///
    /// # Errors
    ///
    /// Propagates [`read_directory`] failures.
    pub fn forward(&mut self) -> Result<bool, FinderError> {
        let Some(target) = self.history.go_forward() else {
            return Ok(false);
        };
        self.listing = read_directory(&target)?;
        Ok(true)
    }

    /// Go to the parent directory, if there is one.
    ///
    /// # Errors
    ///
    /// Propagates [`read_directory`] failures.
    pub fn up(&mut self) -> Result<bool, FinderError> {
        let Some(parent) = self.path().parent().map(Path::to_path_buf) else {
            return Ok(false);
        };
        self.navigate(&parent)?;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

/// One sidebar location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// Label shown in the sidebar.
    pub label: String,
    /// Directory it opens.
    pub path: PathBuf,
    /// True when the directory exists right now.
    pub exists: bool,
}

impl Place {
    fn new(label: &str, path: PathBuf) -> Self {
        let exists = path.is_dir();
        Self {
            label: label.to_owned(),
            path,
            exists,
        }
    }
}

/// The sidebar: real XDG user directories, plus the filesystem root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidebar {
    /// Locations in display order.
    pub places: Vec<Place>,
}

impl Sidebar {
    /// Build the sidebar from `$HOME` and `user-dirs.dirs`.
    ///
    /// Places that do not exist are still listed (with `exists == false`) so
    /// the sidebar does not silently change shape between machines; the UI
    /// can dim them.
    #[must_use]
    pub fn from_environment() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let xdg = config.join("user-dirs.dirs");
        let dirs = fs::read_to_string(&xdg).map_or_else(
            |_| XdgUserDirs::default(),
            |c| XdgUserDirs::parse(&c, &home),
        );
        Self::from_parts(&home, &dirs)
    }

    /// Build from explicit parts. Kept separate so tests never touch the
    /// real environment.
    #[must_use]
    pub fn from_parts(home: &Path, dirs: &XdgUserDirs) -> Self {
        let mut places = vec![Place::new("Home", home.to_path_buf())];
        for (label, path) in [
            ("Desktop", &dirs.desktop),
            ("Documents", &dirs.documents),
            ("Downloads", &dirs.download),
            ("Music", &dirs.music),
            ("Pictures", &dirs.pictures),
            ("Videos", &dirs.videos),
        ] {
            if let Some(path) = path {
                places.push(Place::new(label, path.clone()));
            }
        }
        places.push(Place::new("Filesystem", PathBuf::from("/")));
        Self { places }
    }

    /// Places that exist right now.
    pub fn available(&self) -> impl Iterator<Item = &Place> {
        self.places.iter().filter(|p| p.exists)
    }
}

/// The XDG user directories, as declared in `user-dirs.dirs`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XdgUserDirs {
    /// `XDG_DESKTOP_DIR`.
    pub desktop: Option<PathBuf>,
    /// `XDG_DOWNLOAD_DIR`.
    pub download: Option<PathBuf>,
    /// `XDG_DOCUMENTS_DIR`.
    pub documents: Option<PathBuf>,
    /// `XDG_MUSIC_DIR`.
    pub music: Option<PathBuf>,
    /// `XDG_PICTURES_DIR`.
    pub pictures: Option<PathBuf>,
    /// `XDG_VIDEOS_DIR`.
    pub videos: Option<PathBuf>,
}

impl XdgUserDirs {
    /// Parse `user-dirs.dirs`.
    ///
    /// The format is shell assignments with `$HOME` expansion, so this
    /// deliberately implements only that: `KEY="$HOME/rest"` or
    /// `KEY="/absolute"`. Unknown keys, comments and blanks are ignored.
    #[must_use]
    pub fn parse(contents: &str, home: &Path) -> Self {
        let mut dirs = Self::default();
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"');
            let path = if let Some(rest) = value.strip_prefix("$HOME") {
                home.join(rest.trim_start_matches('/'))
            } else if value.starts_with('/') {
                PathBuf::from(value)
            } else {
                continue;
            };
            match key.trim() {
                "XDG_DESKTOP_DIR" => dirs.desktop = Some(path),
                "XDG_DOWNLOAD_DIR" => dirs.download = Some(path),
                "XDG_DOCUMENTS_DIR" => dirs.documents = Some(path),
                "XDG_MUSIC_DIR" => dirs.music = Some(path),
                "XDG_PICTURES_DIR" => dirs.pictures = Some(path),
                "XDG_VIDEOS_DIR" => dirs.videos = Some(path),
                _ => {}
            }
        }
        dirs
    }
}

// ---------------------------------------------------------------------------
// File operations
// ---------------------------------------------------------------------------

/// What to do when the destination exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    /// Refuse, returning [`FinderError::DestinationExists`]. The default.
    Refuse,
    /// Replace whatever is there.
    Replace,
}

/// Reject paths that would be dangerous to operate on.
fn check_safe(path: &Path) -> Result<(), FinderError> {
    if path.parent().is_none() {
        return Err(FinderError::UnsafePath(path.to_path_buf()));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(FinderError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

/// True when `candidate` is `ancestor` or lies inside it.
fn is_within(ancestor: &Path, candidate: &Path) -> bool {
    let Ok(ancestor) = fs::canonicalize(ancestor) else {
        return false;
    };
    // The candidate may not exist yet, so canonicalise its nearest existing
    // ancestor and append the remainder.
    let mut probe = candidate;
    let mut tail = Vec::new();
    while !probe.exists() {
        match (probe.file_name(), probe.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_os_string());
                probe = parent;
            }
            _ => return false,
        }
    }
    let Ok(mut resolved) = fs::canonicalize(probe) else {
        return false;
    };
    for part in tail.iter().rev() {
        resolved.push(part);
    }
    resolved.starts_with(&ancestor)
}

/// Copy a file or directory tree.
///
/// Symlinks are copied as symlinks, never followed — following them can
/// escape the tree and, with a link pointing at an ancestor, loop forever.
///
/// # Errors
///
/// Returns [`FinderError`] for a missing source, an existing destination
/// under [`Overwrite::Refuse`], an attempt to copy a directory into itself,
/// or any underlying I/O failure.
pub fn copy(source: &Path, destination: &Path, overwrite: Overwrite) -> Result<u64, FinderError> {
    check_safe(source)?;
    check_safe(destination)?;

    let meta = fs::symlink_metadata(source).map_err(|e| FinderError::from_io(source, &e))?;
    if meta.is_dir() {
        return copy_directory(source, destination, overwrite);
    }
    copy_one(source, destination, overwrite)
}

fn copy_directory(
    source: &Path,
    destination: &Path,
    overwrite: Overwrite,
) -> Result<u64, FinderError> {
    if is_within(source, destination) {
        return Err(FinderError::IntoItself {
            source: source.to_path_buf(),
            destination: destination.to_path_buf(),
        });
    }
    if destination.exists() {
        if overwrite == Overwrite::Refuse {
            return Err(FinderError::DestinationExists(destination.to_path_buf()));
        }
        if !destination.is_dir() {
            return Err(FinderError::IsADirectory(destination.to_path_buf()));
        }
    } else {
        fs::create_dir_all(destination).map_err(|e| FinderError::from_io(destination, &e))?;
        // Preserve the source directory's permissions on the new root.
        if let Ok(meta) = fs::symlink_metadata(source) {
            let _ = fs::set_permissions(destination, meta.permissions());
        }
    }

    let mut copied = 0u64;
    let listing = read_directory(source)?;
    for entry in &listing.entries {
        let target = destination.join(&entry.name);
        copied += if entry.kind == EntryKind::Directory {
            copy_directory(&entry.path, &target, overwrite)?
        } else {
            copy_one(&entry.path, &target, overwrite)?
        };
    }
    Ok(copied)
}

fn copy_one(source: &Path, destination: &Path, overwrite: Overwrite) -> Result<u64, FinderError> {
    if destination.exists() {
        if overwrite == Overwrite::Refuse {
            return Err(FinderError::DestinationExists(destination.to_path_buf()));
        }
        if destination.is_dir() {
            return Err(FinderError::IsADirectory(destination.to_path_buf()));
        }
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| FinderError::from_io(parent, &e))?;
    }

    let meta = fs::symlink_metadata(source).map_err(|e| FinderError::from_io(source, &e))?;
    if meta.file_type().is_symlink() {
        let target = fs::read_link(source).map_err(|e| FinderError::from_io(source, &e))?;
        if destination.exists() || fs::symlink_metadata(destination).is_ok() {
            fs::remove_file(destination).map_err(|e| FinderError::from_io(destination, &e))?;
        }
        std::os::unix::fs::symlink(target, destination)
            .map_err(|e| FinderError::from_io(destination, &e))?;
        return Ok(0);
    }

    fs::copy(source, destination).map_err(|e| FinderError::from_io(destination, &e))
}

/// Move a file or directory.
///
/// Tries a rename first, which is atomic and free, and falls back to
/// copy-then-delete for a cross-filesystem move. The count is the number of
/// entries copied in the fallback path, or 1 for a rename.
///
/// # Errors
///
/// As [`copy`], plus any failure while removing the source after a
/// successful copy.
pub fn move_entry(
    source: &Path,
    destination: &Path,
    overwrite: Overwrite,
) -> Result<u64, FinderError> {
    check_safe(source)?;
    check_safe(destination)?;

    if fs::symlink_metadata(source).is_err() {
        return Err(FinderError::NotFound(source.to_path_buf()));
    }
    let meta = fs::symlink_metadata(source).map_err(|e| FinderError::from_io(source, &e))?;
    if meta.is_dir() && is_within(source, destination) {
        return Err(FinderError::IntoItself {
            source: source.to_path_buf(),
            destination: destination.to_path_buf(),
        });
    }
    if destination.exists() {
        if overwrite == Overwrite::Refuse {
            return Err(FinderError::DestinationExists(destination.to_path_buf()));
        }
        delete(destination)?;
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| FinderError::from_io(parent, &e))?;
    }

    match fs::rename(source, destination) {
        Ok(()) => Ok(1),
        Err(_) => {
            // Different filesystem, or a rename the OS refused: copy then
            // remove. A failure here leaves the source intact.
            let copied = copy(source, destination, overwrite)?;
            delete(source)?;
            Ok(copied)
        }
    }
}

/// Delete a file, symlink or directory tree.
///
/// The root and any path containing `..` are refused. This is real
/// deletion; a trash backend is not implemented yet.
///
/// # Errors
///
/// Returns [`FinderError`] for a missing path, an unsafe path, or an I/O
/// failure. The count is the number of filesystem objects removed.
pub fn delete(path: &Path) -> Result<u64, FinderError> {
    check_safe(path)?;
    let meta = fs::symlink_metadata(path).map_err(|e| FinderError::from_io(path, &e))?;
    if meta.is_dir() {
        let mut removed = 0u64;
        let listing = read_directory(path)?;
        for entry in &listing.entries {
            removed += delete(&entry.path)?;
        }
        fs::remove_dir(path).map_err(|e| FinderError::from_io(path, &e))?;
        Ok(removed + 1)
    } else {
        fs::remove_file(path).map_err(|e| FinderError::from_io(path, &e))?;
        Ok(1)
    }
}

/// Total size of a tree, in bytes, without following symlinks.
///
/// # Errors
///
/// Propagates listing failures.
pub fn tree_size(path: &Path) -> Result<u64, FinderError> {
    let meta = fs::symlink_metadata(path).map_err(|e| FinderError::from_io(path, &e))?;
    if !meta.is_dir() {
        return Ok(meta.len());
    }
    let mut total = 0u64;
    for entry in read_directory(path)?.entries {
        total += tree_size(&entry.path)?;
    }
    Ok(total)
}

/// Count the entries in a tree, for a progress estimate.
///
/// # Errors
///
/// Propagates listing failures.
pub fn tree_entry_count(path: &Path) -> Result<u64, FinderError> {
    let meta = fs::symlink_metadata(path).map_err(|e| FinderError::from_io(path, &e))?;
    if !meta.is_dir() {
        return Ok(1);
    }
    let mut total = 1u64;
    for entry in read_directory(path)?.entries {
        total += tree_entry_count(&entry.path)?;
    }
    Ok(total)
}

/// A user-visible destructive action awaiting confirmation.
///
/// The model never performs a destructive operation as a side effect of
/// navigation; a pending action must be confirmed explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDelete {
    /// What would be removed.
    pub path: PathBuf,
    /// How many entries that is, computed up front so the prompt can be
    /// specific.
    pub entries: u64,
    /// Total bytes.
    pub bytes: u64,
}

impl PendingDelete {
    /// Describe a deletion without performing it.
    ///
    /// # Errors
    ///
    /// Propagates traversal failures.
    pub fn describe(path: &Path) -> Result<Self, FinderError> {
        Ok(Self {
            path: path.to_path_buf(),
            entries: tree_entry_count(path)?,
            bytes: tree_size(path)?,
        })
    }

    /// A sentence a confirmation dialog can show verbatim.
    #[must_use]
    pub fn prompt(&self) -> String {
        if self.entries <= 1 {
            format!("Delete {}?", self.path.display())
        } else {
            format!(
                "Delete {} and its {} items?",
                self.path.display(),
                self.entries - 1
            )
        }
    }

    /// Carry it out.
    ///
    /// # Errors
    ///
    /// Propagates [`delete`] failures.
    pub fn confirm(self) -> Result<u64, FinderError> {
        delete(&self.path)
    }
}

/// A queue of file operations, so the UI can show progress and cancel.
#[derive(Debug, Default)]
pub struct OperationQueue {
    queued: VecDeque<Operation>,
}

/// A single queued operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Copy `source` to `destination`.
    Copy {
        /// What to copy.
        source: PathBuf,
        /// Where to put it.
        destination: PathBuf,
    },
    /// Move `source` to `destination`.
    Move {
        /// What to move.
        source: PathBuf,
        /// Where to put it.
        destination: PathBuf,
    },
}

impl OperationQueue {
    /// A new, empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an operation.
    pub fn push(&mut self, operation: Operation) {
        self.queued.push_back(operation);
    }

    /// Number of queued operations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queued.len()
    }

    /// True when nothing is queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queued.is_empty()
    }

    /// Run the next operation, if any, refusing to overwrite.
    ///
    /// Returns the number of filesystem objects affected.
    ///
    /// # Errors
    ///
    /// Propagates the underlying [`copy`] or [`move_entry`] failure. A
    /// failed operation is not retried; the caller reports it and moves on.
    pub fn run_next(&mut self) -> Option<Result<u64, FinderError>> {
        match self.queued.pop_front()? {
            Operation::Copy {
                source,
                destination,
            } => Some(copy(&source, &destination, Overwrite::Refuse)),
            Operation::Move {
                source,
                destination,
            } => Some(move_entry(&source, &destination, Overwrite::Refuse)),
        }
    }
}

#[cfg(test)]
mod tests;
