# ROADMAP.md — Project Tahoe

> Честный план: что реально сделано, что следующее, что сознательно отложено.
> Обновлено: 2026-10-13. Статусы: ✅ сделано · 🟡 написано, не проверено ·
> 🔴 не начато.

---

## Этап 0 — Фундамент (текущий)

| # | Задача | Статус |
|---|---|---|
| 0.1 | Аудит окружения, исследование upstream | ✅ |
| 0.2 | Проверка сборки Smithay 0.7 (winit + полный TTY-стек) | ✅, см. `docs/research/2026-10-13-smithay-build-verification.md` |
| 0.3 | `tahoe-core`, `tahoe-format`, `tahoe-uel` + тесты | ✅ |
| 0.4 | `tahoe-compositor` — минимальный каркас, компилируется | ✅ (`cargo build -p tahoe-compositor`) |
| 0.5 | `tahoe-shell` (модель), `tahoe-diagnostics` (пробы) | ✅ |
| 0.6 | `iso/` профиль archiso, `packages/` PKGBUILDs, `binfmt.d/` | 🟡 файлы есть, сборка ISO и пакетов не выполнялась |
| 0.7 | Документация: ARCHITECTURE, LICENSING, SECURITY, CONTRIBUTING, ADR | ✅ |

Критерий выхода из этапа 0: `cargo test` зелёный на референсной машине,
`cargo build -p tahoe-compositor` успешен, ни одного заявления об
непроверенном успехе в документации.

## Этап 1 — Композитор оживает (следующий)

1. `tahoe-compositor --backend winit`: открыть окно, отрисовать фон,
   открыть тестовый xdg-shell клиент (`weston-terminal`, `foot`).
   Критерий: скриншот с двумя окнами рядом, фокус переключается кликом.
2. Протоколы по одному, каждый с ручным тестом: `xdg-shell` →
   `layer-shell` (панель) → `seat` (клавиатура/мышь, раскладки) →
   `shm`/`dmabuf` → `fractional-scale`/`viewporter`.
3. TTY-бэкенд на тестовой машине: `backend_drm` + `libinput` + `libseat`,
   переключение VT, многомониторность через `wlr-output-management`
   (если понадобится).
4. Xwayland — только после стабильного нативного стека.

## Этап 2 — UEL становится исполняемым

1. Супервизор процессов: `fork`/`exec`, pipes, cgroup-лимиты, таймауты,
   запись результата каждого запуска (успех/неуспех + `ProbeReport`).
2. Интеграция Wine: поиск `wine` в PATH, создание префикса,
   `Strategy::Wine` → реальный запуск тестового PE (например,
   `notepad.exe` из префикса). Метрика: честная таблица совместимости.
3. Интеграция Darling: только если Darling собран и работает на хосте;
   до этого — `Confidence::Layer` + ясный отказ с инструкцией.
4. DMG-пайплайн: read-only монтирование → поиск `.app` → probe Mach-O.
   Никогда — исполнение DMG.

## Этап 3 — Оболочка

1. `tahoe-shell` view на smithay-client-toolkit: Dock поверх
   layer-shell, Launcher с индексом `.desktop`-файлов.
2. Finder-подобный ФМ как обычное приложение + портал FileChooser.
3. Control Center и Settings: сначала моки панелей, потом реальные
   бэкенды (NetworkManager, PipeWire, `org.freedesktop.impl.portal.Settings`
   — он опционален по спецификации, тёмная тема без него тоже работает).

## Этап 4 — Дистрибутив

1. Сборка `iso/` через `mkarchiso` в чистом chroot, загрузка в VM,
   установка через `archinstall`-совместимый скрипт.
2. Репозиторий `tahoe-*` пакетов, подпись ключом, `pacman -Syu` путь.
3. `systemd-sysupdate` для базового образа — только как опция, после
   стабильного ISO.

## Сознательно НЕ делаем

- Свой загрузчик PE/Mach-O (см. ADR-0005).
- Свой пакетный менеджер (см. ADR-0003).
- Связывание GPL-кода в наши бинарники (см. LICENSING.md §3).
- Обещания совместимости без измерений (см. ARCHITECTURE.md §6).
