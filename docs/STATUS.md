# STATUS — что готово и что подтверждено

> Снимок состояния Project Tahoe. Каждое утверждение в разделе 2 получено
> **запуском команды на этой машине**, а не замыслом. Всё, что не
> проверялось, вынесено в разделы 4 и 5 и названо прямо.
>
> Дата: 2026-10-15 · ядро 6.18.55-1-cachyos-lts · x86_64 ·
> Rust 1.99.0 · Swift 6.4.1 · коммит `cc03dba`.

---

## 1. Одной командой

```sh
tests/check-all.sh
```

```
15 passed, 0 failed, 1 skipped
```

Единственный skip — iOS SDK, который требует Apple ID. Всё остальное
исполняется.

---

## 2. Подтверждено запуском

### 2.1 Тесты

| Что | Команда | Результат |
|---|---|---|
| Rust, все крейты | `cargo test` | **511 passed, 0 failed** |
| `tahoe-core` — типы домена | `cargo test -p tahoe-core` | 7 |
| `tahoe-format` — детекция форматов | `cargo test -p tahoe-format` | 67 |
| `tahoe-uel` — решения + супервизор | `cargo test -p tahoe-uel` | 68 |
| `tahoe-compat` — binfmt, wine, macos, xtool | `cargo test -p tahoe-compat` | 148 |
| `tahoe-compositor` — композитор | `cargo test -p tahoe-compositor` | 80 |
| `tahoe-shell` — shell, finder, дизайн-система | `cargo test -p tahoe-shell` | 133 |
| `tahoe-diagnostics` — пробы системы | `cargo test -p tahoe-diagnostics` | 8 |
| Swift, токены и модель | `cd apple && swift test` | **21 passed, 0 failed** |
| Сборка | `cargo build --workspace` | ok |
| Линт | `cargo clippy --all-targets` | 0 предупреждений |
| Формат | `cargo fmt --all -- --check` | clean |

### 2.2 Композитор — настоящий Wayland-сервер

| Что | Как проверено | Результат |
|---|---|---|
| Обслуживает клиентов | 8 интеграционных тестов с настоящим `wayland-client` по реальному сокету | проходят |
| Глобалы | `wayland-info` — **сторонняя утилита Weston** | 8 глобалов, `1920x1080@60Hz`, `capabilities: pointer keyboard`, shm `XR24`/`AR24` |
| Окна, фокус, z-order, перетаскивание | юнит-тесты `wm.rs` (24) + интеграционные | проходят |
| Рендер | `tahoe-compositor --backend winit --exit-after 3` | exit 0 |
| Профиль реального TTY | `cargo check -p tahoe-compositor --features tty` | компилируется |

Важная деталь: оконная политика вынесена в `wm.rs` **без типов Smithay**,
поэтому она тестируется без сокета и GPU.

### 2.3 Ядерная диспетчеризация форматов

**Без root, без изменения хоста.** Ключевой результат проекта:

```
$ ./target/debug/tahoe-compat binfmt self-test
registering in a throwaway user namespace:
  :tahoe-selftest:M:0:\xcf\xfa\xed\xfe::/bin/echo:P
exit status: 0
stdout: /tmp/tahoe-compat-selftest/payload.bin /tmp/tahoe-compat-selftest/payload.bin

OK: the kernel dispatched the file to our interpreter.
```

| Что | Результат |
|---|---|
| `binfmt_misc` монтируется в user namespace (Linux 6.7+) | работает на 6.18.55 |
| Регистрация правила без root | работает |
| Ядро реально передаёт файл нашему интерпретатору | работает |
| Запись **не утекает** в хост | проверено сравнением списка записей до и после |
| Хост не изменяется | `/proc/sys/fs/binfmt_misc` содержит только `register`/`status` |

Правило никогда не матчит ELF — есть тест-ловушка: затенение нативного
загрузчика было бы катастрофой.

### 2.4 Определение форматов

Проверено на настоящих системных файлах и на фикстурах:

| Формат | Проверено на |
|---|---|
| ELF | `/bin/ls`, `/usr/bin/cargo` → `elf x86-64` → `Native` / `verified` |
| AppImage type 2 | реальный AppImage на 1 ГБ в домашней папке → `appimage` |
| DMG (UDIF) | `koly`-трейлер разобран целиком, `blkx`-таблица сжатия |
| Java `.class` vs universal binary | `cafebabe` разрешается правилом `file(1)`; при неоднозначности — **отказ**, а не догадка |
| PE, Mach-O (32/64, обе эндианности), universal | таблица `fat_arch` целиком |
| XAR, OLE2/MSI, plist (XML и `bplist00`) | с фикстурами |

### 2.5 UEL — решения и честность

```
$ ./target/debug/tahoe-uel probe /bin/ls
/bin/ls: format=elf arch=x86-64
  strategy=Native
  confidence=verified

$ ./target/debug/tahoe-uel probe x.dmg
  strategy=Inspect { kind: Dmg, hint: "mount read-only and probe the .app inside" }
  confidence=verified

$ ./target/debug/tahoe-uel probe java8-class
  strategy=Rejected { reason: UnknownFormat }
  confidence=refused
```

`Inspect` — отдельный исход: «знаем точно, что это, и что с этим делать», а
не «не поняли, отказываемся».

### 2.6 Супервизор процессов

| Свойство | Как проверено |
|---|---|
| Никакого shell | аргумент `$(touch …)` передан буквально, файл **не создан** |
| Гигиена окружения | `env_clear` + белый список 22 переменных; leak-canary не доходит до ребёнка |
| Тайм-аут | `--timeout 0.3` убил `sleep 30` за 304 мс, `timed_out: true` |
| Убийство группы процессов | внук **не переживает** тайм-аут (есть тест) |
| Ограниченный захват вывода | усечение сообщается, писатель не получает EPIPE |
| Структурированный журнал | одна строка JSON на запуск, экранирование проверено |
| Накладные расходы | **~1.5–2.2 мс** на запуск (измеряется `tahoe-bench`) |

Накладные расходы были **4.7 мс** (444% от bare fork+exec) из-за опроса с
`sleep(5ms)`. Нашёл бенчмарк, исправил блокирующим `wait()` в потоке.
Отключение захвата вывода, вопреки ожиданию, **не** помогло — это тоже
записано.

### 2.7 Дизайн-система: Rust → Swift без расхождений

| Проверка | Результат |
|---|---|
| Сгенерированный Swift свежий | `tahoe-design-export --check` → up to date |
| Он **компилируется** | `swift build --target TahoeDesign` → ok |
| Значения не расходятся | Rust-тест разбирает сгенерированный Swift обратно и сверяет каждый `RGBA`, отступ, радиус, метрику |
| Swift-сторона подтверждает | `swift test` → 13 тестов токенов |
| Все 11 Swift-файлов парсятся | `swiftc -parse` (не требует SDK) |

Три независимые проверки в обе стороны. Расхождение палитры стало
структурно невозможным.

### 2.8 Рендер рабочего стола — с настоящим текстом

```
$ python3 tests/verify-shell-render.py ~/Pictures/tahoe/tahoe-desktop-dark.png
  [ok  ] canonical size — 1600x1000
  [ok  ] chunk framing — [b'IHDR', b'IDAT', b'IEND']
  [ok  ] not a flat fill — 725 distinct sampled colours
  [ok  ] menu-bar text is present — 669 contrasting pixels
  [ok  ] dock contents are present — 19164 contrasting pixels
  [ok  ] wallpaper brightens downward — 115 -> 509
  [ok  ] the panel strip is distinct from the work area — 111 vs 149
  [ok  ] dock is horizontally centred — centre 799, want 800
  [ok  ] the accent colour is used — 410 accent-ish pixels

  ALL SHELL RENDER CHECKS PASSED
```

9 утверждений, проверенных **независимым декодером** (только `zlib`) — не тем
кодом, что рисует. Плюс 30+ юнит-тестов на сам рендерер и сцены.

**Проверка «текст есть» — не про темноту.** Первая версия считала тёмные
пиксели и ошибалась в обе стороны: в тёмной теме панель сама тёмная, поэтому
счётчик измерял фон (11 166 «букв», которых нет), а в светлой приглушённые
пункты меню слишком светлые, чтобы попасть в счёт (75). Меряется контраст
относительно доминирующего цвета области, и это ловит реальный отказ —
молча переставший рисовать шрифт.

5 сцен: рабочий стол, лаунчер и control center в тёмной теме, рабочий стол и
лаунчер в светлой. Команда и файлы — `docs/shell.md`.

### 2.9 Discovery (Wine)### 2.9 Discovery (Wine)

| Что | Как проверено |
|---|---|
| Обнаружение приложений | 3 раскладки Start Menu, вложенные папки, дедупликация |
| Парсинг `.lnk` | заголовок + `StringData`, включая враждебные длины |
| `.desktop` от `winemenubuilder` | `Name`, `Exec`, `Icon`, `StartupWMClass`, `NoDisplay` |
| Разделение префиксов | запись из чужого префикса отвергается |
| `Exec` без shell | `$(touch …)` остаётся текстом, файл не создаётся |
| NTSync | наличие `/dev/ntsync` и версия ядра сообщаются **раздельно** |

```
$ ./target/debug/tahoe-compat wine apps
prefix /home/ibrohim/.wine (not initialised):
  (none)
0 application(s)
```

Пустой список — правильный ответ: Wine не установлен.

---

## 3. Инструменты, которые есть

| Команда | Что делает |
|---|---|
| `tests/check-all.sh` | все проверки одной командой (17 + 1 skip) |
| `tests/swift-env.sh` | ставит Swift 6.4 **без root**, лечит несовместимость с Arch |
| `tests/verify-shell-render.py` | независимая проверка рендера: текст, градиент, dock, акцент |
| `tests/png-compress.py` | ужимает PNG с 6.4 МБ до ~70 КБ, пиксели не меняются |
| `tahoe-preview --scene all` | рисует 5 сцен рабочего стола в PNG |
| `tahoe-compositor` | `--backend headless` / `winit` |
| `tahoe-uel` | `probe` / `plan` / `run` / `host` |
| `tahoe-compat` | `binfmt` / `wine` / `mac` / `apple` / `runtime` |
| `tahoe-preview` | рендер оформления в PPM, `--design` печатает токены |
| `tahoe-design-export` | генерация Swift из Rust, `--check` для CI |
| `tahoe-bench` | измерения с честной границей «чужое не измеряем» |

---

## 4. Написано, но НЕ проверено запуском

### 4.1 Windows — PE запущен под Wine **и** под Proton

Подтверждено запуском: настоящий PE, собранный здесь `clang`+`lld-link`,
проходит полную цепочку и под **Wine 11.0** (`tahoe-pe-ok`, exit 0), и под
**Proton-CachyOS** (exit 0, 24.9 с, префикс создан, NTSync активен).
Выбор между ними делается по **таблице импортов** файла: `d3d11`/`dxgi`/
`vulkan-1`/`steam_api` → Proton, обычные `shell32`/`comctl32` → Wine.
Подробности — `docs/native-execution.md` и `docs/proton.md`.

22 ноября: Wine 11.0 поставлен user-level Flatpak'ом без root, и **настоящий
Windows PE** (собранный здесь `clang`+`lld-link`) прошёл полную цепочку
**ядро → tahoe-dispatch → Wine → программа** с выводом `tahoe-pe-ok` и
exit 0. `tests/dispatch-e2e.sh`: 8/8. Подробности — `docs/native-execution.md`.

Осталось непроверенным: **ни одно реальное Windows-приложение** не
запускалось — только программа, написанная нами. Совместимость с чужим
софтом — это Wine, и измерять её должен Wine.

Проверено всё *до* `execve`: распознавание, выбор стратегии, построение
команды (`wine <path>`, `wine msiexec /i`), переменные префикса, супервизор.
Не проверено ровно одно звено: что Wine действительно запустит программу.

Также не проверено: создание префикса, реальный `.lnk` от инсталлятора
(фикстуры синтезированы по MS-SHLLINK), `msiexec` на настоящем `.msi`.

### 4.2 macOS

**Ни один Mach-O не запускался.** Darling не установлен.

Не проверено: монтирование DMG (разбор `koly` и `blkx` есть, извлечения
нет), APFS (нет в mainline), настоящий `.app` с реальной macOS, инвокация
`darling <program>` (взята из README, не наблюдалась).

### 4.3 iOS / SwiftUI

| Что | Состояние |
|---|---|
| Генерация токенов | **проверено** |
| Компиляция токенов | **проверено** |
| Синтаксис всех 11 файлов | **проверено** |
| Тесты модели и токенов | **проверено**, 21 тест |
| Компиляция view-файлов | **нет** — SwiftUI на Linux не существует, только `-parse` |
| Сборка iOS `.app` | **нет** — SDK требует Apple ID |
| Запуск на устройстве | **нет** — нужен физический iPhone |

Семантическая ошибка в теле view (правильный синтаксис, неверный тип)
пройдёт незамеченной. Это цена отсутствия SDK.

### 4.4 Дистрибутив

Профиль `iso/` написан, `mkarchiso` не запускался. PKGBUILDs синтаксически
проверены (`makepkg --printsrcinfo`, `bash -n`), но не собирались.

---

## 5. Не работает и не заработает

| Ограничение | Почему |
|---|---|
| Драйверы ядра Windows, античит с kernel-компонентом | У Wine нет ядра. Это не пробел Tahoe. |
| WDDM, DirectStorage | Не реализовано в Wine. |
| arm64 Mach-O | `mldr.c` в Darling содержит `#error TODO: aarch64`; upstream отказался мержить PR. |
| Xcode как IDE, SwiftUI/Combine/CryptoKit | README Darling говорит прямо; фреймворки отсутствуют или заглушки. |
| Запуск iOS `.app` на Linux | Линкует UIKit. Ни Darling, ни что-либо ещё его не реализует. |
| Симулятор iOS на Linux | xtool документирует `--simulator` только под macOS. |
| APFS через ядро | Не в mainline; только read-only FUSE вне ядра. |
| Свой загрузчик PE/Mach-O | ADR-0005: это переписывание того, что уже работает. |
| Патч ядра | ADR-0008: 6.7 и 7.3 уже дают то, что дал бы патч. |

---

## 6. Что нужно от человека

| Блокер | Действие | Что откроется |
|---|---|---|
| Wine | `pacman -S wine` (root) | первая настоящая запись в матрице совместимости вместо «не измерено» |
| iOS SDK | скачать `Xcode.xip` под своим Apple ID (~7 ГБ) | `tahoe-compat apple build apple` соберёт настоящий `.app` |
| Darling | сборка из исходников (долго) | запуск Mach-O, монтирование DMG |
| TTY-бэкенд | `seatd` или группа `video` + свободный VT | композитор на реальном экране |
| ISO | `pacman -S archiso` | загружаемый образ |

---

## 7. Дерево репозитория

```
project-tahoe/
├── crates/            6 крейтов, ~21 000 строк Rust, 511 тестов
│   ├── tahoe-core         типы домена, ноль зависимостей и I/O
│   ├── tahoe-format       ELF/PE/Mach-O/AppImage/DMG/XAR/OLE2/plist/script
│   ├── tahoe-uel          probe → strategy → confidence → supervisор
│   ├── tahoe-compat       binfmt, wine, macos, xtool
│   ├── tahoe-compositor   Smithay 0.7, headless + winit
│   ├── tahoe-shell        модель shell, finder, дизайн-система
│   └── tahoe-diagnostics  пробы только для чтения
├── apple/             SwiftUI-приложение, ~2 500 строк Swift, 21 тест
├── docs/              12 документов, 10 ADR
├── iso/               профиль archiso
├── packages/          PKGBUILDs
├── binfmt.d/          правила для системной установки
└── tests/             check-all.sh, swift-env.sh, verify-shell-render.py
```

---

## 8. Оценка готовности по слоям

| Слой | Готовность | Комментарий |
|---|---|---|
| Фундамент, документация, процесс | **готов** | 10 ADR, лицензионная политика, честная матрица |
| Детекция форматов | **готов** | 12 форматов, все с тестами |
| UEL: решения и честность | **готов** | модели уверенности, отказы с причиной |
| Супервизор процессов | **готов** | измеримо быстрый, без shell, с таймаутом |
| Ядерная диспетчеризация | **готов** | без root, тесты, хост не тронут |
| Дизайн-система | **готова** | Rust + генерируемый Swift, три проверки |
| Композитор | **работает**, протоколы частично | нет курсора, resize, maximise, Xwayland, dmabuf |
| Оболочка | **модель готова**, GUI нет | ни одного `wl_surface` |
| Finder | **ядро готово**, GUI нет | операции с файлами полностью функциональны |
| Слои совместимости | **интеграция готова**, запуска не было | не хватает одного звена на каждой стороне |
| Дистрибутив | **заготовка** | ISO не собран |

---

## 9. Три самых полезных следующих шага

1. **`pacman -S wine`** — и `tahoe-uel run <file>.exe`. Всё до `execve`
   покрыто тестами; отсутствует ровно одно подтверждение. Это превратит
   «не измерено» в первую реальную запись.
2. **Панель в layer-shell** — композитор уже обрабатывает
   `zwlr_layer_shell_v1` и отдаёт корректный размер, дизайн-система и
   `wine`-discovery готовы. Это первый видимый рабочий стол.
3. **`archiso`** — собрать образ и загрузиться в VM. Тогда всё
   вышеперечисленное перестаёт быть набором крейтов.
