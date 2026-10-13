# Исследование: Wine / Darling / binfmt_misc

> Дата: 2026-10-13. Источники: kernel.org docs, winehq.org, archlinux.org +
> ArchWiki, docs.darlinghq.org, github.com/darlinghq/darling, man7.org.
> Полный отчёт — в истории сессии; здесь — факты, на которые опирается
> архитектура.

## Wine — проверено

- Stable **11.0** (2026-01-13), dev **11.19** (2026-10-02). Главное: NTSync
  (модуль ядра с 6.14) и **завершение нового WoW64**.
- Лицензия **LGPL-2.1-or-later**. CrossOver — проприетарная обёртка поверх;
  Proton — BSD-3-клей + LGPL Wine; wine-staging — патчсет.
- Новый WoW64 (эксперимент с 9.0, полная поддержка в 11.0): Wine работает
  как **64-битный** процесс, 32-битные PE — через thunking. **multilib не
  нужен.** `wine64` удалён, `WINEARCH=win32` deprecated.
- Известное ограничение: падение производительности 32-битных приложений
  с прямым OpenGL (bugs.winehq.org #55981).
- Arch: новость 2025-06-16 «Transition to the new WoW64 wine» — зависимость
  от multilib **удалена** для `wine`/`wine-staging` (extra, 11.19-1).
  Arch ставит `/usr/lib/binfmt.d/wine.conf` → `:DOSWin:M::MZ::/usr/bin/wine:`.
- Кросс-арх: сам Wine CPU не эмулирует. x86 на ARM64 — FEX-Emu (MIT),
  box64/box86 (MIT), Hangover; Wine 10.0 — ARM64EC.

## Darling — проверено

- Что это: Mach-O загрузчик (`mldr`) + userspace kernel server
  (`darlingserver`: Mach IPC, POSIX, Darwin syscalls) + реимплементация
  фреймворков (Foundation, AppKit на Cocotron, CoreAudio).
  Лицензия **GPL-3.0-or-later**.
- Релиз v0.1.20260608 (2026-06-09), последний коммит master 2026-09-06.
  Активен, но медленно (~1–8 коммитов/мес), 393 открытых issue.
- **Модуль ядра удалён в 2022** (релиз 2022-07-04, darlingserver merge).
  Darling — полностью userspace. Отдельный репозиторий LKM мёртв с 2020.
- Незашифрованный факт от мейнтейнеров: GUI AppKit beyond hello-world
  в основном не работает; Xcode как IDE не запускается (тулчейн — да);
  Swift-тулчейн работает, Combine/CryptoKit/SwiftUI — нет; Metal —
  начальный бэкенд через Vulkan.
- `.app` запускаются из префикса напрямую, `.dmg` монтируется через
  внутренний `hdiutil`, `.pkg` — через внутренний `installer`.
- ARM64 — PR #1753 открыт, но мейнтейнер отказался мержить.

## binfmt_misc — проверено (доки ядра)

- Монтирование, `register`-синтаксис `:name:type:offset:magic:mask:interp:flags`,
  лимиты (1920 символов строка, magic в первых 128 байтах, интерпретатор ≤127).
- Флаги P/O/C/F (+ новые T/L/D/B в 7.3). Порядок: **позже добавлен — раньше совпадает**.
- Маунт в не-начальных userns — с ядра 6.7.
- systemd-binfmt: `systemd-binfmt.service` + `/usr/lib/binfmt.d/*.conf`.
- Настоящие CVE: CVE-2026-68186, CVE-2026-74618.
  **CVE-2021-22505 — это Netfilter, не binfmt_misc.**
- binfmt_misc — байтовый матчер, не парсер: нельзя выразить предикаты на
  `e_type`/`e_machine`, `PE\0\0` обычно вне 128-байтного окна, `cafebabe`
  коллидирует с Java `.class`. Поэтому решения — в userspace (ADR-0007).

## Не проверено

- Ставит ли Darling binfmt-запись (документированный путь — `darling <bin>`).
- Внутреннее устройство `hdiutil` (hfsplus/dmg2img?).
- Матрица совместимости конкретных приложений под Wine/Darling.
