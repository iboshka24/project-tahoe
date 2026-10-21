# PROGRESS.md — Project Tahoe

> Живой журнал автономной работы. Обновляется после каждой законченной
> задачи. Правило: **в Completed попадает только то, что реально
> проверено командой из раздела Validation.**

Обновлено: 2026-10-21 (сессия 13: MacShell 0.2.8, фото-обои и виджеты — на живом и на установленном).

---

### MacShell 0.2.8 и «как на галерее»: фото-обои + виджеты на обоих системах (сессия 13)

- **ISO из `ecd7899`** (CI run 39613914918, артефакт 12353065779,
  1 773 945 000 B): `tahoe-2026.10.21-x86_64.iso`, `sha256:cba9481d…4425`.
  В образе MacShell/MacLock 0.2.8 (upstream `eebaaad`), фото-обои Yosemite
  (CC0, 1920×1200) и `macwidgets/settings.json` с четырьмя карточками.
- **Живой образ:** загрузка в QEMU с `-nic user`; обои закрыли весь экран
  1280×800, виджеты (Clock/Calendar/Weather/Player) отрисовались, погода
  показала `Yosemite Valley 13 °C` — сетевой путь Open-Meteo работает.
- **Чистая установка:** только клавиатура (`SUPER+I`, 4×`Alt+N`, форма,
  `Alt+N`, `Alt+I`), экран «All done», **ни одной ручной правки**.
- **Диск загрузился сам** до `tahoe-vm login:` (автологина нет — так
  задумано); вход `tahoe`/`tahoe123` → Hyprland + MacShell 0.2.8, обои и
  виджеты на месте. Оттуда сделаны README-скриншоты `tahoe-desktop.png`,
  `tahoe-spotlight.png`, `tahoe-control-center.png`.
- **Проверено внутри установленной системы:** `~/.config/macshell/vendor` →
  `MACSHELL_VERSION=0.2.8`, `MACSHELL_COMMIT=eebaaad…`; `jq -c .widgets
  .config/macwidgets/settings.json` → `clock` large, `calendar` large,
  `weather` medium, `player` medium; `sha256sum
  Pictures/Wallpapers/Tahoe-Dark.png` → `50294fbb…5026` (побайтово равно
  `etc/skel`); `systemctl --failed` → `0 loaded units listed.`;
  `ls /proc/sys/fs/binfmt_misc` → `tahoe-wine-pe`; `pacman -Q jq curl
  playerctl qt6-multimedia swaybg` → 1.8.2-1 / 8.23.0-1 / 2.4.1-5 /
  6.12.0-1 / 1.2.2-1.
- **Осталось честно открытым:** UEFI (только BIOS/MBR), Wine в образе нет,
  GitHub Release нет, `/etc/calamares` остаётся на цели (косметика); на цели
  пусты sync-базы `pacman` — до `pacman -Sy` предупреждение
  `database file for 'core' does not exist`.

Подробности: `docs/iso-installer.md` §10, `docs/iso-session.md` §2a–2b,
`docs/limitations.md` §2. Скриншоты — в `docs/images/`.

---

### Чистая установка финального ISO — и обои, которые не были чёрными (сессия 12)

- **ISO из `b95bb0f`** (CI run 39511318414, артефакт 12306333736):
  `sha256:a9b46634…`, сам ISO — `0e600270…67ed6`. В списке 520 пакетов,
  `swaybg-1.2.2-1` есть, `hyprpaper` нет.
- **Найдено на живом образе:** рабочий стол был **чёрным**. `hyprpaper 0.8.4`
  сменил формат конфига (`wallpaper=,path` → блок `monitor`/`path`) — это
  исправлено, но остался крах: hyprtoolkit/Aquamarine требует GBM/EGL,
  которого нет у QEMU `virtio-vga` без 3D. `chmod 666 /dev/dri/*` не помог:
  `DRM_IOCTL_MODE_CREATE_DUMB` требует DRM master, а не прав файла.
- **Замена:** `swaybg` с `WLR_RENDERER=pixman` — программный рендерер,
  без DRM/GPU. В той же сессии обои появились и закрыли **весь** экран
  1280×800; заодно исчезла чёрная полоса с хвостом консоли, которую
  hyprpaper оставлял на 16:10. Пикер в MacBar.qml теперь тоже перезапускает
  swaybg с pixman, иначе выбранные обои исчезали бы в этой ВМ.
- **Чистая установка:** свежий 24 GiB qcow2, Calamares 3.4.2, только
  клавиатура (`SUPER+I`, 4×`Alt+N`, форма, `Alt+I`). Экран «All done»,
  **ни одной ручной правки** (в отличие от §8 `iso-installer.md`).
- **Диск загрузился сам** до `tahoe-vm login:`; вход `tahoe`/`tahoe123` →
  Hyprland + MacShell с обоями. Внутри: `systemctl --failed` → 0; binfmt
  `active`, правило `:tahoe-wine-pe:M::MZ::/usr/lib/tahoe/tahoe-dispatch:P`;
  `id` → uid 1000 + `wheel`; root `L`; `sudo -n` просит пароль; автологина
  нет; `/dev/vda1 ext4` по UUID; `/boot` с `vmlinuz-linux` и
  `initramfs-linux.img`; `ID=tahoe`; установленный MOTD, live-фраза
  «no password» — 0 вхождений.
- **Осталось честно открытым:** UEFI (только BIOS/MBR), Wine в образе нет,
  GitHub Release нет, `/etc/calamares` остаётся на цели (косметика).

Скриншоты: `Pictures/tahoe/install-final-all-done.png`,
`installed-final-login.png`, `installed-final-desktop.png`,
`installed-final-desktop-clean.png`, `installed-final-binfmt-swaybg.png`,
`installed-final-system.png`, `installed-final-root-locked.png`.
Подробности: `docs/iso-installer.md` §9, `docs/iso-session.md` §2a.

---

### Оболочка нарисована: рендерер, сцены, скриншоты (сессия 7)

- **`tahoe-shell::render`** — программный растеризатор: скруглённые
  прямоугольники со сглаженными углами, градиенты, тени и **настоящий
  текст**. `ab_glyph` (Apache-2.0) разбирает те же TrueType-файлы, что уже
  стоят в системе: у оболочки **порядок предпочтения шрифтов, а не
  зависимость от них**. Нет жирного — молча берётся обычный: десктоп,
  который отказывается стартовать, хуже десктопа с одним весом.
- **`tahoe-shell::scenes`** — рабочий стол, лаунчер и control center.
  Обои рисует `DesignSystem::wallpaper_at` — та же функция, которую
  композитор грузит текстурой в GL, а Swift-эмиттер пишет в SwiftUI.
  Три рендерера, один градиент.
- **Плитки control center называют, к чему они были бы привязаны** —
  NetworkManager, BlueZ, PipeWire, портал Settings, — а не притворяются
  работающими переключателями. Тот же принцип, что модель уверенности в
  UEL, применённый к интерфейсу.
- **`tahoe-preview --scene all`** пишет 5 сцен PNG. Проверено
  `tests/verify-shell-render.py`: 9 независимых утверждений (независимый
  декодер, только `zlib`). Скриншоты: `~/Pictures/tahoe/`.
- **Найден настоящий баг в проверке:** «текст есть» через подсчёт тёмных
  пикселей ошибается **в обе стороны** — в тёмной теме считает фон панели
  (11 166 несуществующих «букв»), в светлой не видит приглушённые пункты
  меню (75). Правильная метрика — контраст относительно доминирующего цвета
  области. Именно она ловит реальный отказ: молча переставший рисовать шрифт.
- **Убрано дублирование:** старые `tahoe-shell::preview` и
  `tests/verify-preview.py` удалены — два рендерера одной дизайн-системы это
  на один больше, чем нужно. Ценные проверки (dock по центру, акцент)
  перенесены в новый верификатор.
- **PNG без сжатия** — честно: stored-блоки deflate, 6.4 МБ на кадр.
  `tests/png-compress.py` ужимает те же пиксели до ~70 КБ, потерянность
  проверяется побайтовым сравнением распакованных строк. Кодировщик — не
  самая интересная часть десктопа.
- **Чего нет:** ни одного `wl_surface`. Подключение к композитору через
  layer-shell — следующая задача. `docs/shell.md` прямо говорит, что
  картинка — это рендер раскладки, а не снимок сессии, и какие отказы
  рендер показать не может.

### COSMIC-тема и нативный диспетчер (сессия 4)

- **`docs/cosmic-theming.md`** + `tahoe-cosmic-theme`: **99 файлов** темы для
  COSMIC. Патчить десктоп не нужно — он читает оформление из RON-файлов и
  системные дефолты из `/usr/share/cosmic/`. Маппинг из той же дизайн-системы,
  поэтому COSMIC, композитор Tahoe и SwiftUI-приложение читают одну палитру.
  32-цветная палитра **выводится** (нейтрали по оси фон→текст, акценты
  поворотом тона), а не выдумывается.
- **Найден настоящий баг:** цвета в RON должны быть **в кавычках**
  (`"#9E9E9E40"`); неквотированный `#RRGGBBAA` — вообще не значение RON,
  тема молча откатилась бы к дефолтам. Плюс `window_hint`/`shade` —
  это `Option`, нужен `Some(...)`.
- **`tahoe-dispatch`** — интерпретатор для `binfmt_misc`, который
  **заменяет себя через `execv`**. Цепочка запуска: `ядро → диспетчер → wine`
  (2 звена вместо 4). Диспетчер заново определяет формат и отказывает, если
  магия совпала не с тем: `cafebabe` — это ещё и Java-класс.
- **`tests/dispatch-e2e.sh`: 7 проверок**, сквозной запуск с подставным wine
  в namespace. Подтверждено: `./installer.exe` без обёртки, аргументы
  доходят по порядку, shell-скрипт идёт нативно, Java-класс отвергнут с
  указанием реального формата, хост не изменён.

### Настоящий .exe запущен под настоящим Wine (сессия 5)

- **Wine 11.0 установлен без root** — как user-level Flatpak
  (`org.winehq.Wine//stable-25.08`). `sudo` на машине требует пароль;
  flatpak под юзером его не требует.
- **Собран настоящий Windows PE** здесь же: `clang --target=x86_64-pc-windows-msvc`
  + `lld-link` против самодельной `kernel32.lib`. MinGW не нужен — clang и
  lld-link есть в Arch.
- **Полная цепочка подтверждена:** ядро (`binfmt_misc`) → `tahoe-dispatch`
  (`execv`) → Wine → программа → `tahoe-pe-ok`, exit 0.
  `tests/dispatch-e2e.sh`: **8 проверок, 0 падений**.
- `tahoe-uel run hello.exe` → `format=pe`, стратегия `Wine`,
  `confidence=layer:wine`, `exit_code: 0`, 4.3 с (первый запуск, создаётся префикс).
- `docs/native-execution.md` — точное определение: какие из пяти смыслов
  «нативного» достижимы (четыре), и почему пятый невозможен в принципе.

### Proton вместо/рядом с Wine (сессия 6)

- **`tahoe-compat::proton`** — слой Proton, отдельный от Wine, потому что
  Proton не «лучший Wine», а другой: его `run` запускает заглушку
  `steam.exe`, а не программу. 22 теста.
- **Выбор по таблице импортов PE**, а не глобально: `tahoe-format::pe`
  разбирает Import Directory (RVA→offset через таблицу секций). Тесты
  проверяют парсер на **настоящих PE от линкера**. 18 тестов.
- **Четыре настоящих бага найдены запуском Proton** (не чтением):
  1. `protonfixes` извлекает цифры из `STEAM_COMPAT_DATA_PATH` —
     путь без цифр даёт `IndexError`;
  2. `protonfixes` требует `PWD` с `/steamapps` внутри;
  3. Proton не создаёт каталог данных сам, а `filelock` его требует;
  4. `PWD` не было в белом списке окружения супервизора.
- **Итог:** `tahoe-uel run game.exe` → `Strategy::Proton` → **exit 0**,
  префикс создан, NTSync активен.
- `docs/proton.md` — включая честную поправку: Proton для игр, не для
  десктопных приложений.

## Снимок состояния

Актуальная сводка «что готово и что подтверждено» — в
[`STATUS.md`](STATUS.md). Она собирается из свежего прогона
`tests/check-all.sh`, а не из памяти.

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
- ~~`tahoe-shell::preview`~~: удалён (сессия 7) — его заменил
  `tahoe-shell::render` с настоящим текстом. Бинарь `tahoe-preview` теперь
  рендерит 5 сцен в PNG, `--design` печатает токены. Проверка —
  `tests/verify-shell-render.py` (**независимый декодер**, только `zlib`):
  9 утверждений о реальных визуальных свойствах.

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

### Apple-приложения на Linux (сессия 3)

- **`docs/apple-development.md`** + `apple/` — реальное SwiftUI-приложение
  (`TahoeGallery`), собираемое под iOS на Linux через
  [xtool](https://github.com/xtool-org/xtool) (MIT, ~10k звёзд, активен).
- **Единая дизайн-система**: Rust — источник истины, Swift **генерируется**
  (`tahoe-design-export`). Три независимые проверки: регенерация с
  сравнением байт, обратный разбор сгенерированного Swift с сравнением
  значений, и `swift test` на стороне Swift. Расхождение палитры стало
  структурно невозможным.
- **`tahoe-compat::xtool`**: определение предусловий в правильном порядке
  (сначала Swift, потом SDK — иначе пользователя отправят качать 7 ГБ),
  разбор `xtool.yml`, построение команды сборки, инспекция результата.
  CLI: `apple toolchain|project|build|inspect`.
- **Три настоящих бага, найденных реалистичной фикстурой iOS-бандла:**
  1. iOS-бандлы **плоские** (`Foo.app/Info.plist`), без `Contents/` —
     парсер знал только macOS-раскладку и отверг бы всё, что собирает xtool;
  2. iOS пишет `MinimumOSVersion`, macOS — `LSMinimumSystemVersion`,
     читался только второй;
  3. самозакрывающийся `<dict/>` не парсился, а он есть в каждом
     `UILaunchScreen`.
  Плюс четвёртый: `peek_tag` останавливался на `/`, из-за чего `</array>`
  выглядел как пустой открывающий тег.

### Swift реально собран и протестирован (сессия 3, продолжение)

- **Swift 6.4.1 установлен без root** (`tests/swift-env.sh`, Ubuntu-сборка в
  `~/.local/share/swift-toolchain`). Это закрыло три дыры в проверенности.
- **Найдена и обойдена несовместимость Arch**: сборка для Ubuntu линкует
  `libncurses.so.6`, а Arch собирает ncurses только в wide-варианте.
  Наивное лечение через `LD_LIBRARY_PATH` работает для `swift --version`
  и **падает на линковке**, потому что SwiftPM не пробрасывает окружение
  в дочерний `swiftc`. Симлинк положен внутрь RUNPATH тулчейна — переменные
  не нужны вовсе.
- **Сгенерированный Swift компилируется** (`swift build --target TahoeDesign`).
- **Все 11 Swift-файлов парсятся** (`swiftc -parse`, не требует SDK).
- **`swift test`: 21 тест проходит** — 13 токенов + 8 модели. Модель
  намеренно без SwiftUI, поэтому контракт поиска проверяется теми же
  правилами, что и Rust-`LauncherIndex`: кросс-языковая проверка поведения.
- **Компиляция нашла настоящий баг**: генератор называл сэмплер
  `darkWallpaperColor`, а тест вызывал `wallpaperColor`. Ничто, кроме
  компилятора, это не поймало бы.
- `tests/check-all.sh` расширен: **15 проверок, 0 падений, 1 skip**
  (skip — только iOS SDK за Apple ID).

### Установщик (2026-10-20)

- Calamares 3.4.2 из AUR в CI, 22 конфига модулей, очистка live-состояния
  (`passwd -l root`, удаление `tahoe`, sudoers/autologin/archiso-хуков),
  ядро возвращается в `/boot` перед `initcpio`.
- Установка в QEMU (BIOS, 24 GiB qcow2) доведена до конца; установленная
  система загрузилась с диска в Hyprland + MacShell. Прогон потребовал двух
  ручных правок, потому что его squashfs был собран до добавления пакетов;
  обе правки и ещё пять дефектов — в `docs/iso-installer.md` §8.
- Не проверено: тот же прогон без рук на пересобранном ISO, UEFI.

## In Progress

Этап 3 — GUI оболочки. **Рендерер и сцены готовы** (сессия 7): 5 сцен в
`~/Pictures/tahoe/`, проверены `tests/verify-shell-render.py`. Осталось
подключить их к композитору: `Canvas` в `wl_shm`-буфер — это `memcpy`,
поэтому визуальная часть уже есть, а новой остаётся только панель в
`zwlr_layer_shell_v1` и обработка ввода.

---

## Blocked

| Блокер | Что нужно | Обходной путь сейчас |
|---|---|---|
| ~~Wine не установлен~~ | **Снят** | Wine 11.0 поставлен как user-Flatpak без root; настоящий PE отработал под Wine и под Proton, exit 0 |
| Darling не собран | сборка из исходников, долго | Ни один Mach-O не запускался |
| DMG не монтируется | нужен внешний инструмент (7-Zip/apfs-fuse) | Трейлер и `blkx` разобраны, содержимое не извлекается |
| `archiso` не установлен | `pacman -S archiso` | Профиль написан, ISO не собран |
| Нет `seatd`/доступа к DRM как user | seat + права | TTY-бэкенд только компилируется |
| Нет клиента-панели для `zwlr_layer_shell` | нужен клиент на `smithay-client-toolkit` | Протокол обрабатывается; оболочка рисуется только в файл |

---

## Next

1. **Подключить рендерер к композитору.** `Canvas` в `wl_shm`-буфер —
   это `memcpy`; визуальная часть готова. Первый шаг — панель в
   `zwlr_layer_shell_v1` (композитор уже отдаёт корректный size).
2. **Ввод.** Панель, dock и лаунчер должны получать события seat; сейчас
   рендер статичен.
3. **Dock и Launcher вживую**: рендер состояния `Dock`/`LauncherIndex`,
   запуск приложений через `tahoe-uel` с показом `Confidence` и ошибок.
4. **Окно Finder**: навигация, вкладки, боковая панель из `finder::Sidebar`,
   операции через `OperationQueue` с прогрессом.
5. **Darling**: собрать и прогнать настоящий Mach-O — единственная
   оставшаяся дыра в «нативном» запуске.
6. ISO: `mkarchiso` в чистом chroot.

---

## Validation

Все команды выполняются из корня репозитория. Ничего из этого не требует
root и не меняет систему.

```sh
# Одна команда: сборка, тесты, clippy, fmt, свежесть сгенерированного Swift,
# рендер рабочего стола, ядерная диспетчеризация, Apple-проект, бенчмарк.
tests/check-all.sh

cargo test                       # 511 тестов
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
python3 tests/verify-shell-render.py ~/Pictures/tahoe/tahoe-desktop-dark.png
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
| Оформление | `python3 tests/verify-…` (см. ниже) | см. §2.8 |
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
