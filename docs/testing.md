# Тестовая стратегия — Project Tahoe

## Принцип

Каждый уровень проверяется тем инструментом, который ему подходит.
Никаких «успешных тестов», которых не было.

## Уровни

| Уровень | Что | Как | Где |
|---|---|---|---|
| L1 | Юнит-тесты типов | `cargo test -p tahoe-core` | `crates/tahoe-core` |
| L2 | Парсер форматов на синтетических + реальных фикстурах | `cargo test -p tahoe-format` (40+ тестов) | `crates/tahoe-format`, `fixtures/` |
| L3 | Логика UEL: probe → strategy + confidence | `cargo test -p tahoe-uel` | `crates/tahoe-uel` |
| L4 | Модель shell: dock, workspaces, launcher index | `cargo test -p tahoe-shell` | `crates/tahoe-shell` |
| L5 | Пробы diagnostics (только чтение) | `cargo test -p tahoe-diagnostics` | `crates/tahoe-diagnostics` |
| L6 | Компиляция композитора против Smithay | `cargo build -p tahoe-compositor` (+ `cargo check` с TTY-фичами) | `crates/tahoe-compositor` |
| L7 | Линт и формат | `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check` | корень |
| L8 | Лицензионный гейт | `cargo deny` / ручная проверка: нет GPL в графе зависимостей first-party крейтов | CI |

## Фикстуры

`fixtures/` содержит:

- `generate.py` — генератор синтетических бинарников (ELF32/64 LE/BE,
  PE32/PE32+, Mach-O 32/64, fat, AppImage-заглушка, скрипты, `.class`
  vs fat-коллизия) — запускается без привилегий, детерминирован;
- `README.md` — что означает каждый фикстур и какой тест его читает.

Реальные системные файлы (`/bin/ls`, `/usr/lib/libc.so.6`) используются
только как *дополнительная* ручная проверка, не как обязательный тест
(пути различаются между машинами).

## Критерии готовности компонента

Компонент считается готовым, когда:

1. `cargo test -p <имя>` зелёный;
2. `clippy` без предупреждений;
3. публичный API задокументирован;
4. `docs/research/` или ADR фиксирует внешние факты, на которые он опирается.

## Что пока НЕ тестируется (честно)

- Запуск Wine/Darling (нет в окружении, см. research).
- Сборка ISO (`archiso` не установлен на референсной машине).
- Сборка PKGBUILDs (нужен чистый chroot).
- Рендеринг композитора (нужен GPU/TTY или вложенный сеанс; этап 1).
