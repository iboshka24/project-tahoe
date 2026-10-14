# PROGRESS.md — Project Tahoe

> Живой журнал автономной работы. Обновляется после каждой законченной
> задачи. Правило: **в Completed попадает только то, что реально
> проверено командой из раздела Validation.**

Обновлено: 2026-10-14 (сессия 2: Universal Native Execution).

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

### Tahoe Finder (Этап 5, ядро) — работает

- `tahoe-shell::finder`: листинг каталога с детерминированным порядком
  (каталоги первыми, регистронезависимо), флаг скрытости для любой
  вложенности, классификация симлинков **без перехода по ним**, детект
  битых ссылок.
- `History`/`Tab`: back/forward/up с корректной семантикой стека (новый
  переход обрезает forward-историю, как в браузере). Проваленный переход
  не сдвигает вкладку и не засоряет историю.
- `Sidebar` + `XdgUserDirs::parse`: реальные XDG-каталоги из
  `user-dirs.dirs` (с расширением `$HOME`), а не захардкоженный список.
  Отсутствующие каталоги показываются, но помечаются `exists=false`.
- `copy`/`move_entry`/`delete`/`tree_size`/`tree_entry_count` +
  `OperationQueue` и `PendingDelete` (подтверждение с числом элементов).
- **54 теста**, каждый — в своей временной директории; ничего
  пользовательского не затрагивается.

Отвергается явно: перезапись без `Overwrite::Replace`; копирование или
перемещение каталога внутрь себя или своего поддерева (иначе — рекурсия
до заполнения диска); удаление корня и путей с `..`.

Найдено тестом и исправлено: скрытость файла внутри `.config/app/`
не определялась, если скрыт не непосредственный родитель, а дед.
Теперь проверяются все компоненты пути (чистая работа со строкой,
без syscall).

### Universal Native Execution (сессия 2)

- **`tahoe-compat`** — новый крейт: `binfmt` (ядерная диспетчеризация),
  `wine` (префиксы, обнаружение, `.lnk`, `winemenubuilder`), `macos`
  (Mach-O, universal, `.app`, plist, DMG). 115 тестов.
- **`tahoe-format`** расширен: `koly`-трейлер UDIF целиком, `blkx`-таблица
  сжатия, XAR, OLE2/MSI, plist, `fat_arch`-таблица, дизамбигуация
  `0xcafebabe` по правилу `file(1)`. 67 тестов.
- **UEL**: новые стратегии `WineInstaller`, `MacBundle`, `Inspect`;
  `Inspect` — «знаем что это и что делать», а не отказ. 68 тестов.
- **Супервизор ускорен**: накладные расходы **4.7 мс → ~1.5 мс** на запуск.
  Найдено и исправлено бенчмарком, а не догадкой.
- **`tahoe-bench`** — измерительный стенд с честной границей: измеряем
  своё, не приписываем себе производительность Wine.
- **5 документов**: `universal-execution-research.md`,
  `kernel-integration.md`, `windows-compatibility.md`,
  `macos-compatibility.md`, `compatibility-matrix.md`; ADR-0008..0010.
- **Ключевой проверенный результат**: `binfmt_misc` работает без root
  внутри user namespace, ядро реально диспетчеризует файл нашему
  интерпретатору, и хост не меняется. Есть сквозной тест.

## In Progress

Этап 3 — GUI оболочки: панель в layer-shell, Dock, Launcher.

---

## Blocked

| Блокер | Что нужно | Обходной путь сейчас |
|---|---|---|
| Wine не установлен | `pacman -S wine` (нужен root) | UEL честно возвращает `LayerMissing`; ни один PE не запускался |
| Darling не собран | сборка из исходников, долго | Ни один Mach-O не запускался |
| DMG не монтируется | нужен внешний инструмент (7-Zip/apfs-fuse) | Трейлер и `blkx` разобраны, содержимое не извлекается |
| `archiso` не установлен | `pacman -S archiso` | Профиль написан, ISO не собран |
| Нет `seatd`/доступа к DRM как user | seat + права | TTY-бэкенд только компилируется |
| `wayland-info` не видит `zwlr_layer_shell` клиента | — | layer-shell обрабатывается, но GUI-панели ещё нет |

---

## Next

1. **Доказать запуск на живых Wine и Darling.** Всё до `execve` уже
   покрыто тестами; отсутствует ровно одно звено — реальный запуск.
   `pacman -S wine`, затем `tahoe-uel run <file>.exe`.
2. **Layer-shell панель**: клиент на `smithay-client-toolkit`, который
   рисует настоящую панель в зарезервированной полосе. Композитор уже
   обрабатывает `zwlr_layer_shell_v1` и отдаёт корректный size.
2. **Dock и Launcher**: рендер состояния `Dock`/`LauncherIndex` в
   `wl_shm`-буфер; запуск приложений через `tahoe-uel` с показом
   `Confidence` и ошибок.
3. **Окно Finder**: навигация, вкладки, боковая панель из `finder::Sidebar`,
   операции через `OperationQueue` с прогрессом.
4. **Wine на реальной машине**: `pacman -S wine` (нужен root) → проверить
   `Strategy::Wine` end-to-end на настоящем PE.
5. ISO: `mkarchiso` в чистом chroot.

---

## Validation

Все команды выполняются из корня репозитория. Ничего из этого не требует
root и не меняет систему.

```sh
cargo test                       # 457 тестов
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

### Фактические результаты финального прогона

| Проверка | Команда | Результат |
|---|---|---|
| Все тесты | `cargo test` | **312 passed, 0 failed** |
| Интеграция с настоящим клиентом | `cargo test -p tahoe-compositor --test wayland_client` | 8/8 |
| Линт | `cargo clippy --all-targets` | 0 warnings, 0 errors |
| Формат | `cargo fmt --all -- --check` | clean |
| Сборка бинарей | `cargo build --bins` | ok |
| Сторонний клиент | `wayland-info` против headless | 8 глобалов, `1920x1080@60Hz`, `capabilities: pointer keyboard` |
| Реальный рендер | `--backend winit --exit-after 3` | exit 0 |
| Оформление | `python3 tests/verify-preview.py` | 11/11 |
| Запуск скрипта | `tahoe-uel run py.py` | `#!/usr/bin/env python3` → exit 0 |
| Тайм-аут | `tahoe-uel run --timeout 0.3 slow.sh` | `"timed_out":true`, 300 ms |
| Отказ | `tahoe-uel run x.dmg` | exit 1, «mount it read-only … never execute it» |
| Инъекция | `run args.sh '$(touch …)'` | аргумент передан буквально, файл не создан |
| TTY-профиль | `cargo check -p tahoe-compositor --features tty` | ok (не подключён к запуску) |

### Проверки, которые НЕ выполнялись

Перечислено, чтобы не создавать ложного впечатления:

- запуск Wine или Darling (не установлены, установка требует root);
- сборка ISO (`mkarchiso` не установлен);
- сборка пакетов из `packages/` (нужен чистый chroot);
- работа TTY-бэкенда на реальном VT (нет seatd и прав на DRM);
- запуск PE/Mach-O файлов — ни одного;
- рендеринг композитора на GPU (winit-путь проверен как «запускается и
  завершается», но пиксели GL не снимались).
