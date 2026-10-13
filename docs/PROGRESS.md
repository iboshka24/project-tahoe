# PROGRESS.md — Project Tahoe

> Живой журнал автономной работы. Обновляется после каждой законченной
> задачи. Правило: **в Completed попадает только то, что реально
> проверено командой из раздела Validation.**

Обновлено: 2026-10-14.

---

## Completed

### Фундамент

- 6 крейтов, 7 ADR, 4 research-записки, Arch-профиль `iso/`, PKGBUILDs,
  `binfmt.d/`, лицензионная политика (`LICENSING.md`).
- `tahoe-format`: парсер ELF/PE/Mach-O/AppImage/DMG/скриптов; 45 тестов,
  включая честную неоднозначность `0xcafebabe` (Java class vs fat Mach-O).

### Композитор (Этап 1) — работает

- **Оконная логика вынесена в `wm.rs`** без типов Smithay: z-order, фокус,
  hit-test, каскадная раскладка, перетаскивание с clamp. 24 юнит-теста.
- **`state.rs`**: реальные обработчики Smithay 0.7 — `CompositorHandler`,
  `ShmHandler`, `SeatHandler`, `OutputHandler`, `DataDeviceHandler`,
  `SelectionHandler`, `XdgShellHandler`, `WlrLayerShellHandler`.
  Зарегистрированы глобалы: `wl_compositor`, `wl_subcompositor`, `wl_shm`,
  `wl_seat`, `wl_output`, `wl_data_device_manager`, `xdg_wm_base`,
  `zwlr_layer_shell_v1`.
- **Headless-бэкенд**: настоящий Wayland-сервер без дисплея (для тестов и CI).
- **winit-бэкенд**: вложенное окно, реальный рендер — градиентные обои
  (`MemoryRenderBuffer`), полоса панели, окна клиентов по z-order.
- **Верифицировано сторонним клиентом**: `wayland-info` (утилита Weston)
  подключается и подтверждает все глобалы, режим `1440x900@60Hz`,
  `capabilities: pointer keyboard`, форматы shm `XR24`/`AR24`.
- Устранён реальный баг: `clamp(320, area.w)` паниковал на экранах < 320px.
- Устранён реальный баг: окно считалось «mapped» до первого буфера, из-за
  чего невидимый клиент мог забрать фокус.

### UEL: супервизор (Этап 4, основное) — работает

- `tahoe-uel::supervisor`: `plan_command` (чистая функция) + `spawn`
  (реальный запуск). 38 тестов, все на настоящих процессах.
- **Никакого shell**: argv передаётся прямо в `execve`. Тест с аргументом
  `$(touch …)` подтверждает, что файл не создаётся.
- **Тайм-аут убивает группу процессов**: `process_group(0)` + `killpg`,
  поэтому внуки не выживают. Тест «внук не должен пережить тайм-аут».
- **Гигиена окружения**: `env_clear()` + белый список из 22 переменных.
  Тест подтверждает, что `TAHOE_LEAK_CANARY` не доходит до ребёнка.
- **Ограниченный захват вывода**: ребёнок не может заставить лаунчер
  аллоцировать бесконечно; усечение сообщается и не приводит к EPIPE.
- **Структурированный журнал**: `LaunchRecord::to_log_line()` — одна
  строка JSON с форматом, стратегией, confidence, командой и исходом.
- CLI: `tahoe-uel run` / `plan` / `probe` / `host`.

Найдено и исправлено CLI-прогоном:

- `#!/usr/bin/env python3` трактовался как один путь `/usr/bin/env python3`
  (не существующий файл). Добавлен `tahoe_format::split_shebang`,
  `Strategy::Script` теперь несёт `interpreter_args`; скрипт реально
  запускается (проверено).

### Дизайн-система и превью (Этап 3, начало)

- `tahoe-shell::design`: цвета, градиент обоев, отступы, радиусы, метрики;
  светлая и тёмная темы, общие метрики. 22 теста.
- `tahoe-shell::preview`: программный композитор рабочего стола в RGB.
- Бинарь `tahoe-preview`: рендерит PPM, `--design` печатает токены.
- `tests/verify-preview.py` — **независимая** проверка PPM/PNG (только
  `zlib`): 11 утверждений о реальных визуальных свойствах.

---

## In Progress

Этап 5 — Tahoe Finder: модель навигации, вкладки, боковая панель,
файловые операции с обработкой ошибок. Тестируемо без GUI.

---

## Blocked

| Блокер | Что нужно | Обходной путь сейчас |
|---|---|---|
| Wine не установлен | `pacman -S wine` (нужен root) | UEL честно возвращает `LayerMissing` |
| Darling не собран | сборка из исходников, долго | То же |
| `archiso` не установлен | `pacman -S archiso` | Профиль написан, ISO не собран |
| Нет `seatd`/доступа к DRM как user | seat + права | TTY-бэкенд только компилируется |
| `wayland-info` не видит `zwlr_layer_shell` клиента | — | layer-shell обрабатывается, но GUI-панели ещё нет |

---

## Next

1. **Этап 4 — UEL супервизор**: `tahoe-uel::supervisor` с `spawn`,
   таймаутами, capture вывода, структурированным журналом и корректной
   обработкой сигналов. Полностью тестируемо без Wine (`/bin/sh`).
2. **Layer-shell панель**: клиент на `smithay-client-toolkit`, который
   рисует настоящую панель в зарезервированной полосе.
3. **Этап 5 — Finder**: модель навигации, боковая панель, вкладки,
   операции с файлами с обработкой ошибок. Тестируемо без GUI.
4. Лаунчер: индекс `.desktop`, запуск через UEL, отображение ошибок.
5. ISO: `mkarchiso` в чистом chroot.

---

## Validation

Все команды выполняются из корня репозитория. Ничего из этого не требует
root и не меняет систему.

```sh
cargo test                       # 258 тестов
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build -p tahoe-compositor  # winit
cargo check -p tahoe-compositor --features tty

# Реальный протокольный тест сторонним клиентом
./target/debug/tahoe-compositor --backend headless \
    --socket /tmp/tahoe-wi.sock --size 1440x900 --exit-after 20 &
WAYLAND_DISPLAY=/tmp/tahoe-wi.sock wayland-info

# Реальный рендер (откроется окно на 3 секунды)
./target/debug/tahoe-compositor --backend winit --exit-after 3

# Визуальная проверка оформления
python3 tests/verify-preview.py
```

### Фактические результаты последнего прогона

| Проверка | Результат |
|---|---|
| `cargo test` | 258 тестов, 0 падений |
| `cargo test -p tahoe-compositor --test wayland_client` | 8/8, реальный `wayland-client` |
| `wayland-info` против headless | все 8 глобалов, режим, seat, shm-форматы |
| `--backend winit --exit-after 3` | exit 0, окно открылось и закрылось |
| `python3 tests/verify-preview.py` | 11/11 утверждений |
| `cargo clippy --all-targets -- -D warnings` | см. Validation выше |
