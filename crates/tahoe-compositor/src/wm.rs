// SPDX-License-Identifier: MIT OR Apache-2.0
//! Window-management logic, free of Smithay and Wayland types.
//!
//! The compositor's behaviour that users actually notice — z-order, focus,
//! hit testing, cascade placement, interactive move — lives here as a plain
//! data structure so it can be unit-tested without a GPU, a socket, or a
//! running compositor. The Smithay glue in [`crate::state`] only converts
//! protocol events into calls on this module.

use std::fmt;

/// Stable identifier for a managed window.
///
/// Assigned by [`WindowStack`] and never reused, so a stale id is always
/// detectably stale rather than silently referring to a new window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "win{}", self.0)
    }
}

/// A rectangle in logical compositor coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width, always > 0 for a valid rect.
    pub w: i32,
    /// Height, always > 0 for a valid rect.
    pub h: i32,
}

impl Rect {
    /// Build a rect, normalising non-positive sizes to 1x1 so that a
    /// degenerate buffer can never make a window unhittable *and*
    /// unplaceable at the same time.
    #[must_use]
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            x,
            y,
            w: w.max(1),
            h: h.max(1),
        }
    }

    /// A rect centred at `(x, y)` with the given size.
    #[must_use]
    pub fn centered_at(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self::new(x - w / 2, y - h / 2, w, h)
    }

    /// True when `(px, py)` lies inside, half-open on right/bottom edges.
    ///
    /// Half-open is deliberate: adjacent windows sharing an edge must not
    /// both claim a pixel, or hit testing becomes order-dependent at borders.
    #[must_use]
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }

    /// Overlapping area, or `None` when they do not intersect.
    #[must_use]
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.w).min(other.x + other.w);
        let bottom = (self.y + self.h).min(other.y + other.h);
        if right <= x || bottom <= y {
            None
        } else {
            Some(Self {
                x,
                y,
                w: right - x,
                h: bottom - y,
            })
        }
    }

    /// Force the rect to fit inside `bounds`, shrinking if necessary.
    #[must_use]
    pub fn clamped_into(&self, bounds: &Self) -> Self {
        let w = self.w.min(bounds.w);
        let h = self.h.min(bounds.h);
        let max_x = bounds.x + bounds.w - w;
        let max_y = bounds.y + bounds.h - h;
        Self {
            x: self.x.clamp(bounds.x, max_x),
            y: self.y.clamp(bounds.y, max_y),
            w,
            h,
        }
    }
}

/// A window's geometry and identity as the compositor sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedWindow {
    /// Identity.
    pub id: WindowId,
    /// On-screen rect.
    pub rect: Rect,
    /// Whether the client has committed a buffer (otherwise it is
    /// configured but not yet drawable).
    pub mapped: bool,
    /// Client-declared title.
    pub title: String,
    /// Client-declared app id.
    pub app_id: String,
}

/// How new windows are positioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cascade {
    /// Offset from the top-left of the work area for the first window.
    pub origin: (i32, i32),
    /// Per-window step.
    pub step: (i32, i32),
    /// How many offsets before the cycle repeats.
    pub period: u32,
}

impl Default for Cascade {
    fn default() -> Self {
        Self {
            origin: (96, 72),
            step: (38, 32),
            period: 10,
        }
    }
}

/// Interactive move in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveDrag {
    /// Window being moved.
    pub id: WindowId,
    /// Pointer offset from the window origin when the drag started, so the
    /// window does not jump under the cursor.
    pub grab_offset: (i32, i32),
}

/// Ordered window stack: bottom at index 0, top at the last index.
#[derive(Debug, Default)]
pub struct WindowStack {
    windows: Vec<ManagedWindow>,
    next_id: u64,
    /// Id of the focused window, if any.
    focused: Option<WindowId>,
    /// Active interactive move.
    drag: Option<MoveDrag>,
    /// Monotonic counter used for cascade placement.
    placed: u32,
}

impl WindowStack {
    /// An empty stack.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of windows, mapped or not.
    #[must_use]
    pub fn len(&self) -> usize {
        self.windows.len()
    }

    /// True when no windows are managed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// All windows, bottom to top.
    #[must_use]
    pub fn windows(&self) -> &[ManagedWindow] {
        &self.windows
    }

    /// The focused window, if the focus still refers to a live window.
    #[must_use]
    pub fn focused(&self) -> Option<&ManagedWindow> {
        self.focused.and_then(|id| self.get(id))
    }

    /// Id of the focused window.
    #[must_use]
    pub fn focused_id(&self) -> Option<WindowId> {
        self.focused.filter(|id| self.get(*id).is_some())
    }

    /// Look a window up by id.
    #[must_use]
    pub fn get(&self, id: WindowId) -> Option<&ManagedWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// The active move drag, if any.
    #[must_use]
    pub fn drag(&self) -> Option<MoveDrag> {
        self.drag
    }

    /// Add a window on top with cascade placement inside `work_area`.
    ///
    /// Returns the new id. The window is *not* focused: the compositor
    /// focuses on first commit so that a client which never draws cannot
    /// steal focus.
    pub fn insert(&mut self, work_area: Rect, size: (i32, i32), cascade: Cascade) -> WindowId {
        let id = WindowId(self.next_id);
        self.next_id += 1;

        let step_index = self.placed % cascade.period.max(1);
        self.placed += 1;
        let x = work_area.x + cascade.origin.0 + cascade.step.0 * step_index as i32;
        let y = work_area.y + cascade.origin.1 + cascade.step.1 * step_index as i32;
        let rect = Rect::new(x, y, size.0, size.1).clamped_into(&work_area);

        self.windows.push(ManagedWindow {
            id,
            rect,
            mapped: false,
            title: String::new(),
            app_id: String::new(),
        });
        id
    }

    /// Remove a window and clear any state that pointed at it.
    ///
    /// Returns the removed window so callers can log what went away.
    pub fn remove(&mut self, id: WindowId) -> Option<ManagedWindow> {
        let index = self.windows.iter().position(|w| w.id == id)?;
        let removed = self.windows.remove(index);
        if self.focused == Some(id) {
            // Focus the next window down the stack, like a normal desktop.
            self.focused = self.windows.last().map(|w| w.id);
        }
        if self.drag.is_some_and(|d| d.id == id) {
            self.drag = None;
        }
        Some(removed)
    }

    /// Move a window to the top of the stack.
    pub fn raise(&mut self, id: WindowId) {
        if let Some(index) = self.windows.iter().position(|w| w.id == id) {
            let window = self.windows.remove(index);
            self.windows.push(window);
        }
    }

    /// Set focus, ignoring ids that are not in the stack.
    pub fn set_focused(&mut self, id: Option<WindowId>) {
        self.focused = id.filter(|id| self.get(*id).is_some());
    }

    /// Raise and focus in one step (what a click does).
    ///
    /// Returns the id if it was a real window.
    pub fn activate(&mut self, id: WindowId) -> Option<WindowId> {
        self.get(id)?;
        self.raise(id);
        self.set_focused(Some(id));
        Some(id)
    }

    /// Record that a real buffer was attached to a window, with its size.
    ///
    /// Returns true when this is the window's *first* buffer, which is the
    /// signal the compositor uses to raise and focus it.
    ///
    /// A `wl_surface.commit()` with no buffer (which every xdg-shell client
    /// sends once, to obtain its initial configure) must NOT come through
    /// here: a window that has never drawn anything is not yet mappable,
    /// and focusing it would steal focus for an invisible client.
    pub fn on_buffer_attached(&mut self, id: WindowId, size: (i32, i32)) -> bool {
        let Some(index) = self.windows.iter().position(|w| w.id == id) else {
            return false;
        };
        let first = !self.windows[index].mapped;
        self.windows[index].mapped = true;
        if size.0 > 0 && size.1 > 0 {
            self.windows[index].rect.w = size.0;
            self.windows[index].rect.h = size.1;
        }
        first
    }

    /// Record client metadata.
    pub fn set_metadata(&mut self, id: WindowId, title: Option<String>, app_id: Option<String>) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            if let Some(t) = title {
                w.title = t;
            }
            if let Some(a) = app_id {
                w.app_id = a;
            }
        }
    }

    /// The topmost window whose rect contains `(x, y)`.
    ///
    /// Iterates top-down, so a partially covered window is not returned
    /// while a window above it owns that pixel.
    #[must_use]
    pub fn hit_test(&self, x: i32, y: i32) -> Option<WindowId> {
        self.windows
            .iter()
            .rev()
            .find(|w| w.rect.contains(x, y))
            .map(|w| w.id)
    }

    /// Begin an interactive move, remembering the grab offset.
    ///
    /// Returns the window rect so the caller can compute the offset, or
    /// `None` if the id is unknown.
    pub fn begin_move(&mut self, id: WindowId, pointer: (i32, i32)) -> Option<Rect> {
        let rect = self.get(id)?.rect;
        self.drag = Some(MoveDrag {
            id,
            grab_offset: (pointer.0 - rect.x, pointer.1 - rect.y),
        });
        Some(rect)
    }

    /// Continue an interactive move to an absolute pointer position.
    ///
    /// Returns true when a drag was active and the window moved.
    pub fn drag_to(&mut self, pointer: (i32, i32), work_area: Rect) -> bool {
        let Some(drag) = self.drag else {
            return false;
        };
        let Some(index) = self.windows.iter().position(|w| w.id == drag.id) else {
            self.drag = None;
            return false;
        };
        let size = (self.windows[index].rect.w, self.windows[index].rect.h);
        let x = pointer.0 - drag.grab_offset.0;
        let y = pointer.1 - drag.grab_offset.1;
        self.windows[index].rect = Rect::new(x, y, size.0, size.1).clamped_into(&work_area);
        true
    }

    /// End any interactive move. Returns the window that was moved.
    pub fn end_move(&mut self) -> Option<WindowId> {
        self.drag.take().map(|d| d.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(0, 0, 1920, 1080)
    }

    fn stack_with(n: usize) -> (WindowStack, Vec<WindowId>) {
        let mut s = WindowStack::new();
        let ids = (0..n)
            .map(|_| s.insert(area(), (800, 600), Cascade::default()))
            .collect();
        (s, ids)
    }

    #[test]
    fn insert_assigns_unique_increasing_ids() {
        let (s, ids) = stack_with(3);
        assert_eq!(ids.len(), 3);
        assert!(ids[0] < ids[1] && ids[1] < ids[2]);
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn insert_cascades_and_clamps_inside_work_area() {
        let mut s = WindowStack::new();
        let c = Cascade {
            origin: (0, 0),
            step: (500, 0),
            period: 10,
        };
        let a = s.insert(Rect::new(0, 0, 1000, 500), (400, 300), c);
        let b = s.insert(Rect::new(0, 0, 1000, 500), (400, 300), c);
        let c2 = s.insert(Rect::new(0, 0, 1000, 500), (400, 300), c);
        assert_eq!(s.get(a).unwrap().rect.x, 0);
        assert_eq!(s.get(b).unwrap().rect.x, 500);
        // Third would be 1000..1400, clamped to 600..1000.
        assert_eq!(s.get(c2).unwrap().rect.x, 600);
    }

    #[test]
    fn insert_never_exceeds_work_area_size() {
        let mut s = WindowStack::new();
        let id = s.insert(Rect::new(0, 0, 300, 200), (1000, 900), Cascade::default());
        let r = s.get(id).unwrap().rect;
        assert!(r.w <= 300 && r.h <= 200);
        assert!(r.x >= 0 && r.y >= 0);
    }

    #[test]
    fn remove_clears_focus_and_drag() {
        let (mut s, ids) = stack_with(2);
        s.set_focused(Some(ids[1]));
        s.begin_move(ids[1], (0, 0));
        let removed = s.remove(ids[1]).unwrap();
        assert_eq!(removed.id, ids[1]);
        assert!(s.drag().is_none());
        assert_eq!(
            s.focused_id(),
            Some(ids[0]),
            "focus falls back to the window below"
        );
    }

    #[test]
    fn remove_unknown_is_none() {
        let (mut s, _) = stack_with(1);
        assert!(s.remove(WindowId(999)).is_none());
    }

    #[test]
    fn raise_moves_to_top() {
        let (mut s, ids) = stack_with(3);
        s.raise(ids[0]);
        assert_eq!(s.windows().last().unwrap().id, ids[0]);
        assert_eq!(s.windows()[0].id, ids[1]);
    }

    #[test]
    fn raise_unknown_is_noop() {
        let (mut s, ids) = stack_with(2);
        s.raise(WindowId(999));
        assert_eq!(s.windows().len(), 2);
        assert_eq!(s.windows().last().unwrap().id, ids[1]);
    }

    #[test]
    fn focus_refuses_unknown_ids() {
        let (mut s, ids) = stack_with(1);
        s.set_focused(Some(WindowId(999)));
        assert_eq!(s.focused_id(), None);
        s.set_focused(Some(ids[0]));
        assert_eq!(s.focused_id(), Some(ids[0]));
    }

    #[test]
    fn activate_raises_and_focuses() {
        let (mut s, ids) = stack_with(3);
        assert_eq!(s.activate(ids[0]), Some(ids[0]));
        assert_eq!(s.focused_id(), Some(ids[0]));
        assert_eq!(s.windows().last().unwrap().id, ids[0]);
        assert_eq!(s.activate(WindowId(999)), None);
    }

    #[test]
    fn hit_test_prefers_topmost() {
        let mut s = WindowStack::new();
        let a = s.insert(
            area(),
            (100, 100),
            Cascade {
                origin: (0, 0),
                step: (0, 0),
                period: 1,
            },
        );
        let b = s.insert(
            area(),
            (100, 100),
            Cascade {
                origin: (0, 0),
                step: (0, 0),
                period: 1,
            },
        );
        // Both at (0,0,100,100): the later one wins.
        assert_eq!(s.hit_test(50, 50), Some(b));
        s.raise(a);
        assert_eq!(s.hit_test(50, 50), Some(a));
    }

    #[test]
    fn hit_test_misses_outside() {
        let (s, _) = stack_with(1);
        assert_eq!(s.hit_test(-5, 10), None);
        assert_eq!(s.hit_test(5000, 5000), None);
    }

    #[test]
    fn hit_test_is_half_open_at_edges() {
        let r = Rect::new(10, 10, 100, 50);
        assert!(r.contains(10, 10));
        assert!(r.contains(109, 59));
        assert!(!r.contains(110, 59), "right edge excluded");
        assert!(!r.contains(109, 60), "bottom edge excluded");
    }

    #[test]
    fn first_buffer_reports_once() {
        let (mut s, ids) = stack_with(1);
        assert!(!s.get(ids[0]).unwrap().mapped, "a new window is not mapped");
        assert!(s.on_buffer_attached(ids[0], (640, 480)));
        assert!(
            !s.on_buffer_attached(ids[0], (640, 480)),
            "second buffer is not first"
        );
        assert!(s.get(ids[0]).unwrap().mapped);
    }

    #[test]
    fn buffer_records_size_but_ignores_degenerate() {
        let (mut s, ids) = stack_with(1);
        s.on_buffer_attached(ids[0], (640, 480));
        assert_eq!(s.get(ids[0]).unwrap().rect.w, 640);
        s.on_buffer_attached(ids[0], (0, 0));
        assert_eq!(
            s.get(ids[0]).unwrap().rect.w,
            640,
            "0x0 must not shrink the window"
        );
    }

    #[test]
    fn buffer_on_unknown_window_is_false() {
        let (mut s, _) = stack_with(1);
        assert!(!s.on_buffer_attached(WindowId(999), (10, 10)));
    }

    #[test]
    fn metadata_updates_independently() {
        let (mut s, ids) = stack_with(1);
        s.set_metadata(ids[0], Some("Terminal".into()), None);
        assert_eq!(s.get(ids[0]).unwrap().title, "Terminal");
        assert_eq!(s.get(ids[0]).unwrap().app_id, "");
        s.set_metadata(ids[0], None, Some("foot".into()));
        assert_eq!(s.get(ids[0]).unwrap().title, "Terminal");
        assert_eq!(s.get(ids[0]).unwrap().app_id, "foot");
    }

    #[test]
    fn drag_moves_window_preserving_grab_offset() {
        let mut s = WindowStack::new();
        let id = s.insert(
            area(),
            (200, 100),
            Cascade {
                origin: (100, 100),
                step: (0, 0),
                period: 1,
            },
        );
        // Grab at (150, 130): offset from origin (100,100) is (50,30).
        s.begin_move(id, (150, 130));
        assert!(s.drag_to((400, 300), area()));
        let r = s.get(id).unwrap().rect;
        assert_eq!((r.x, r.y), (350, 270));
    }

    #[test]
    fn drag_clamps_to_work_area() {
        let mut s = WindowStack::new();
        let work = Rect::new(0, 40, 1000, 700);
        let id = s.insert(
            work,
            (200, 100),
            Cascade {
                origin: (0, 0),
                step: (0, 0),
                period: 1,
            },
        );
        s.begin_move(id, (0, 40));
        s.drag_to((-500, -500), work);
        let r = s.get(id).unwrap().rect;
        assert_eq!((r.x, r.y), (0, 40), "clamped to work area origin");
        s.drag_to((5000, 5000), work);
        let r = s.get(id).unwrap().rect;
        assert_eq!(
            (r.x, r.y),
            (800, 640),
            "clamped so the window stays fully visible"
        );
    }

    #[test]
    fn drag_without_begin_is_noop() {
        let (mut s, _) = stack_with(1);
        assert!(!s.drag_to((10, 10), area()));
    }

    #[test]
    fn drag_of_removed_window_ends_cleanly() {
        let mut s = WindowStack::new();
        let id = s.insert(area(), (100, 100), Cascade::default());
        s.begin_move(id, (0, 0));
        s.remove(id);
        assert!(!s.drag_to((10, 10), area()));
        assert!(s.drag().is_none());
    }

    #[test]
    fn end_move_returns_window_once() {
        let mut s = WindowStack::new();
        let id = s.insert(area(), (100, 100), Cascade::default());
        s.begin_move(id, (0, 0));
        assert_eq!(s.end_move(), Some(id));
        assert_eq!(s.end_move(), None);
    }

    #[test]
    fn begin_move_unknown_is_none() {
        let (mut s, _) = stack_with(1);
        assert!(s.begin_move(WindowId(999), (0, 0)).is_none());
    }

    #[test]
    fn intersection_basics() {
        let a = Rect::new(0, 0, 100, 100);
        assert_eq!(
            a.intersection(&Rect::new(50, 50, 100, 100)),
            Some(Rect::new(50, 50, 50, 50))
        );
        assert_eq!(
            a.intersection(&Rect::new(100, 0, 10, 10)),
            None,
            "touching edges do not overlap"
        );
        assert_eq!(a.intersection(&Rect::new(200, 200, 10, 10)), None);
    }

    #[test]
    fn rect_normalises_degenerate_sizes() {
        let r = Rect::new(0, 0, -5, 0);
        assert_eq!((r.w, r.h), (1, 1));
        assert!(r.contains(0, 0));
    }

    #[test]
    fn centered_at_places_centre_on_point() {
        let r = Rect::centered_at(100, 100, 40, 20);
        assert_eq!((r.x, r.y), (80, 90));
        assert!(r.contains(100, 100));
    }
}
