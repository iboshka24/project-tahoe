# Проверка сборки Smithay — фактические результаты

> Дата: 2026-10-13. Машина: CachyOS, kernel 6.18.55-1-cachyos-lts,
> rustc/cargo 1.99.0, systemd 262. Все команды выполнялись без привилегий,
> ничего в систему не устанавливалось (только `cargo` в `/tmp/opencode`).

## P0. Окружение

- Wayland-стек в pkg-config: wayland-server/client/cursor 1.26.0,
  wayland-protocols 1.49, xkbcommon 1.13.2, libinput 1.32.0, libseat 0.9.3,
  libudev 262, libdrm 2.4.134, gbm (mesa 26.2.4), egl, gl, glib 2.88,
  dbus 1.16, cairo 1.18, pango 1.58, pipewire 1.6.9 — всё на месте.
- Сессия: systemd, loginctl seat0/tty1, сеть поднята.
- crates.io API отдавал 403, но sparse-индекс (`index.crates.io`) и
  `static.crates.io` — 200, `cargo generate-lockfile`/`build` работали.

## P1. Минимальный профиль (`backend_winit`, `wayland_frontend`, `desktop`)

```
[dependencies]
smithay = { version = "0.7", default-features = false,
            features = ["backend_winit", "wayland_frontend", "desktop"] }
```

- `cargo generate-lockfile`: 205 пакетов.
- `cargo build`: **успех за 1м42с**.
- Разрешённые версии (из вывода сборки): wayland-server 0.31.14,
  wayland-protocols 0.32.13, wayland-protocols-wlr/plasma/misc 0.3.12,
  calloop 0.13.0 + 0.14.5, x11rb 0.13.2, wayland-cursor 0.31.14.

## P2. Полный TTY-профиль

```
features = ["backend_drm","backend_gbm","backend_egl","backend_libinput",
  "backend_session_libseat","backend_udev","backend_x11","backend_winit",
  "desktop","renderer_gl","renderer_multi","wayland_frontend","xwayland",
  "use_system_lib"]
```

- `cargo build`: **успех за 2м07с** (drm 0.14.1, gbm 0.18.0 поверх системного libgbm).
- Вывод: полный стек реального TTY (DRM/KMS + libinput + libseat + udev)
  компилируется на референсной машине без доустановок.

## P3. Состояние хоста (для UEL/diagnostics)

- `binfmt_misc`: смонтирован (`/proc/sys/fs/binfmt_misc`, `enabled`),
  **записей нет** (только `register`/`status`).
- Ядро 6.18: `max_user_namespaces = 62217` (userns разрешены);
  `max_binfmt_misc_interpreters` отсутствует (ядро без 7.3-флагов — ожидаемо).
- Установлены: `flatpak 1:1.18.4-1.1`, `fuse2`, `bwrap`, `mkinitcpio 42.2-1`,
  `systemd 262-1`, `base`, `binutils`, `fakeroot`.
- Отсутствуют: `wine`, `darling`, `qemu-user-static`, `box64`, `fex`,
  `archiso`, `dracut`, `systemd-sysupdate`. UEL обязана это обнаруживать,
  а не предполагать.
- GPU: `/dev/dri/{card1,renderD128}` присутствует.
- Реальный AppImage (`Amoeba-0.1.48-linux-x64.AppImage`): magic
  `7f 45 4c 46 02 01 01 00 41 49 02 00` — ELF64 + `AI\x02` на смещении 8,
  что подтверждает детектор AppImage в `tahoe-format`.

## Вывод

Зависимости Smithay 0.7 совместимы с референсным окружением. Пин
`smithay = "0.7"`, MSRV 1.87 (запас до 1.99 локально). Пробные крейты лежали
в `/tmp/opencode/smithay-probe*` и в репозиторий не входят.
