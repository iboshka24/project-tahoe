# Исследование: Arch-упаковка, Flatpak, AppImage, лицензии

> Дата: 2026-10-13. Источники: wiki.archlinux.org, archlinux.org (пакеты +
> новости), gitlab.archlinux.org, docs.flatpak.org, github.com/AppImage,
> gnu.org/licenses. Полные отчёты — в истории сессии.

## Arch — проверено (пакеты на 2026-10-13)

- PKGBUILD: обязательны `pkgname pkgver pkgrel arch` + функция `package()`.
  Лицензии — SPDX; кастомные — `LicenseRef-*` или `custom:*` + файл в
  `/usr/share/licenses/$pkgname/`. Источники пакетов — `0BSD` (REUSE).
- Репозитории: `core`, `extra`, `multilib` (182 пакета, активен),
  `core-testing`, `extra-testing`, `multilib-testing`, `gnome-unstable`,
  `kde-unstable`. `staging`/`testing`/`community` **удалены 2025-03-01**.
- `mkinitcpio` 42.2-1 (core, GPL-2.0-only) — стандарт; `dracut` 111-1
  (extra) — альтернатива; UKI — 4 поддерживаемых пути, `systemd-ukify`
  262-1 отдельным пакетом; systemd-boot внутри пакета `systemd` 262-1.
- `archiso` 91-1 (extra, **GPL-3.0-or-later**) — официальный инструмент ISO.
  Профиль: `profiledef.sh`, `packages.x86_64`, `airootfs/`, `efiboot/`…
  Команда: `mkarchiso -v -w work -o out profile/`. `build.sh` нет.
  Официальный ISO грузится через syslinux (BIOS) + systemd-boot (UEFI).
- `archinstall` 4.5-1 (extra, GPL-3.0-only) — официальный установщик.
- Reproducible builds: https://reproducible.archlinux.org жив,
  ~79.7% GOOD (12 445 из 15 612, API на 2026-10-13). Инструменты:
  `makerepropkg`, `repro`, `reprotest`.
- `systemd-sysupdate`/`repart`/`creds`/`homed` — внутри пакета `systemd`.

## Flatpak — проверено

- 1.18.4 stable (2026-09-28), **LGPL-2.1-or-later**, OSTree + bubblewrap +
  D-Bus proxy. Манифест: `id` (не `app-id`), `runtime`, `runtime-version`,
  `sdk`, `command`, `modules`, `finish-args`.
- GUI: `--socket=wayland` (+`fallback-x11`), `--device=dri`. Порталы доступны
  без `talk-name`. Для кастомного композитора нужно: сокет Wayland,
  `xdg-shell` (stable), `org.freedesktop.portal.Desktop` на сессионной шине,
  `.portal`-файл + `<desktop>.portals` конфиг, проброс env в D-Bus activation.
- Декорации сервера, `xdg_foreign`, Settings-портал — **опциональны**
  (документировано). Скриншот — реализуем сами или делегируем `-gtk`.
- Бэкенды: `-gtk` (LGPL-2.1), `-gnome`, `-wlr` (MIT, только
  Screenshot+ScreenCast), `-cosmic` (GPL-3.0).

## AppImage — проверено

- Спецификация — Working Draft, MIT, версий релизов нет. Тип 2 — текущий,
  **типа 3 не существует**. `appimaged` заархивирован; активны
  `type2-runtime` и `appimagetool` (MIT).
- `libfuse2` как библиотека больше не требуется (статический рантайм),
  но kernel-FUSE для монтирования нужен; официальный путь без FUSE —
  `--appimage-extract-and-run` / `APPIMAGE_EXTRACT_AND_RUN=1`.
- **Песочницы нет**, сигнатура опциональна и не проверяется рантаймом.

## Лицензии — проверено

- Wine LGPL-2.1-or-later, Darling GPL-3.0-or-later, Flatpak LGPL-2.1-or-later,
  FEX-Emu / box64 / box86 / Smithay / AppImageKit — MIT,
  cosmic-comp GPL-3.0-only, xdg-desktop-portal LGPL-2.1-or-later.
- MIT→GPL в одном бинарнике: нельзя без перелицензирования целого
  (GPLv3 §5(c), FAQ `#IfLibraryIsGPL`, `#GPLWrapper`, `#MereAggregation`).
  MIT→LGPL: можно (§6 LGPL, shared-lib механизм + notices).
- COSMIC: GPL-код + MIT-зависимости — разрешено (асимметрия в пользу GPL).
- Вывод для Tahoe: держать Wine/Darling за границей процесса —
  и юридически чисто, и инженерно правильно. См. LICENSING.md.
