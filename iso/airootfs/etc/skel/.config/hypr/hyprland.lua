-- SPDX-License-Identifier: GPL-3.0-or-later
--
-- Tahoe live session — Hyprland 0.55+ (Lua config) with the MacShell desktop.
--
-- The desktop is MacShell (menu bar + Dock + Spotlight + widgets), vendored
-- from https://github.com/0-ss/MacShell — see iso/macshell-vendor.md for the
-- pinned commit and the MIT notice. The MacShell autostart block below is
-- exactly what upstream's install.sh writes between the two markers; keep
-- those markers intact so upstream's installer can recognise it later.
--
-- Everything here uses the documented `hl.*` API (Lua stubs live in
-- /usr/share/hypr/stubs/hl.meta.lua). No hyprlang syntax in this file:
-- Hyprland 0.55+ reads hyprland.lua and silently ignores hyprland.conf.

-- ── environment ───────────────────────────────────────────────────────────
hl.env("QT_WAYLAND_DISABLE_WINDOWDECORATION", "1")
hl.env("GDK_BACKEND", "wayland,x11,*")

-- ── sane defaults ─────────────────────────────────────────────────────────
hl.config({
    general = {
        gaps_in = 5,
        gaps_out = 14,
        border_size = 1,
        layout = "dwindle",
    },
    decoration = {
        rounding = 10,
        -- MacShell's Liquid Glass needs blur enabled in the compositor.
        blur = { enabled = true, size = 8, passes = 3, new_optimizations = true },
    },
    input = {
        numlock_by_default = true,
        follow_mouse = 1,
        touchpad = { natural_scroll = true },
    },
    misc = {
        -- we ship our own wallpaper through hyprpaper, not Hyprland's own
        force_default_wallpaper = 0,
        disable_hyprland_logo = true,
        -- lets a restarted MacLock take over a lock if it ever died
        allow_session_lock_restore = true,
    },
    xwayland = { force_zero_scaling = true },
})

-- ── autostart ─────────────────────────────────────────────────────────────
hl.on("hyprland.start", function()
    -- wallpaper daemon; MacShell's picker falls back to it (awww > swww >
    -- hyprpaper > swaybg). The default image is shipped in ~/Pictures.
    hl.exec_cmd("hyprpaper")
    -- MacShell: waits for the compositor, never starts twice, restarts the
    -- shell if it crashes.
    hl.exec_cmd("sh ~/.local/bin/macshell-start")
end)

-- ── MacShell: Liquid Glass layer rules + keybinds ──────────────────────────
-- >>> MacShell (added by install.sh) >>>  (markers match upstream install.sh)
hl.layer_rule({ name = "macdock",  match = { namespace = "macdock"  }, blur = true, ignore_alpha = 0.2,  no_anim = true })
hl.layer_rule({ name = "macspot",  match = { namespace = "macspot"  }, blur = true, ignore_alpha = 0.05, no_anim = true })
hl.layer_rule({ name = "macbarbg", match = { namespace = "macbarbg" }, blur = true, ignore_alpha = 0.05, no_anim = true })
hl.layer_rule({ name = "macshot",  match = { namespace = "macshot"  }, no_anim = true })

local qs = "qs -c MacShell ipc call "
hl.bind("SUPER + V",          hl.dsp.exec_cmd(qs .. "spotlight clipboard"))
hl.bind("SUPER + period",     hl.dsp.exec_cmd(qs .. "spotlight emoji"))
hl.bind("SUPER + Space",      hl.dsp.exec_cmd(qs .. "spotlight toggle"))
hl.bind("ALT + TAB",          hl.dsp.exec_cmd(qs .. "switcher next"))
hl.bind("ALT + SHIFT + TAB",  hl.dsp.exec_cmd(qs .. "switcher prev"))
hl.bind("CTRL + ALT + W",     hl.dsp.exec_cmd(qs .. "wallpapers toggle"))
hl.bind("PRINT",              hl.dsp.exec_cmd(qs .. "screenshot region"))
hl.bind("SUPER + L",          hl.dsp.exec_cmd("sh ~/.config/quickshell/MacLock/lock-via-ipc.sh"))
-- <<< MacShell <<<

-- ── window management (macOS-ish habits, Hyprland dispatchers) ─────────────
hl.bind("SUPER + Return",      hl.dsp.exec_cmd("kitty"))
hl.bind("SUPER + Q",           hl.dsp.window.close())
hl.bind("ALT + F4",            hl.dsp.window.close())
hl.bind("SUPER + F",           hl.dsp.window.float())
hl.bind("SUPER + M",           hl.dsp.window.fullscreen({ mode = "maximized" }))
hl.bind("SUPER + SHIFT + M",   hl.dsp.window.fullscreen())
hl.bind("SUPER + C",           hl.dsp.window.center())
hl.bind("SUPER + Left",        hl.dsp.focus({ direction = "l" }))
hl.bind("SUPER + Right",       hl.dsp.focus({ direction = "r" }))
hl.bind("SUPER + Up",          hl.dsp.focus({ direction = "u" }))
hl.bind("SUPER + Down",        hl.dsp.focus({ direction = "d" }))
hl.bind("SUPER + SHIFT + Left",  hl.dsp.window.move({ direction = "l" }))
hl.bind("SUPER + SHIFT + Right", hl.dsp.window.move({ direction = "r" }))
hl.bind("SUPER + SHIFT + Up",    hl.dsp.window.move({ direction = "u" }))
hl.bind("SUPER + SHIFT + Down",  hl.dsp.window.move({ direction = "d" }))

-- 1..5 workspaces, in and out (Lua beats spelling out ten binds)
for i = 1, 5 do
    local ws = tostring(i)
    hl.bind("SUPER + " .. ws,           hl.dsp.focus({ workspace = ws }))
    hl.bind("SUPER + SHIFT + " .. ws,   hl.dsp.window.move({ workspace = ws }))
end

-- reload: SUPER+SHIFT+R
hl.bind("SUPER + SHIFT + R",   hl.dsp.reload_config())

-- ── window rules ──────────────────────────────────────────────────────────
-- GTK's pickers and the Qt/zenity file dialogs float, so they do not end up
-- tiling behind the Dock.
hl.window_rule({ name = "float-zenity",  match = { class = "zenity" },  float = true })
hl.window_rule({ name = "float-kdialog", match = { class = "kdialog" }, float = true })
-- xdg-desktop-portal's screencast / screenshot chooser
hl.window_rule({ name = "float-portal",  match = { title = "Screencast" }, float = true })
