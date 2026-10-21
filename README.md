# Open EndlessWar Launcher

[![CI](https://github.com/delio-no/open-ew-launcher/actions/workflows/ci.yml/badge.svg)](https://github.com/delio-no/open-ew-launcher/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20Windows-blue)]()
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)]()

Быстрый, легкий и открытый лаунчер для игры [Endless War](https://endlesswar.live/) (на базе Lineage 2).  
Работает на **Linux** (через PortProton / Wine) и **Windows**.

---

## 🚀 Скачать и запустить

Готовые файлы доступны на странице **[Releases](https://github.com/delio-no/open-ew-launcher/releases)**.

### Linux
1. Скачайте архив `open-ew-launcher-linux-x86_64.tar.gz`.
2. Распакуйте и запустите:
   ```bash
   tar -xzf open-ew-launcher-linux-x86_64.tar.gz
   ./open-ew-launcher
   ```
3. Укажите папку с клиентом игры (или пустую папку, если нужно скачать клиент с нуля).

### Windows
1. Скачайте архив `open-ew-launcher-windows-x86_64.zip`.
2. Распакуйте в любое удобное место и запустите `open-ew-launcher.exe`.

---

## ⚡ Особенности

- **Быстрая и полная проверка**: сверка файлов за 1-2 секунды или глубокая проверка CRC32 на всех ядрах процессора.
- **Чистая установка и докачка**: при обрыве соединения или закрытии лаунчера докачивает только оставшиеся файлы.
- **Серверы обновлений**: выбор официальных зеркал, добавление своих адресов и встроенный пинг серверов (кнопка «Выбрать самый быстрый»).
- **Автообновление**: проверка и скачивание файлов в один клик при старте.
- **Готов к Linux**: автоматически запускает игру через PortProton с нужными параметрами совместимости.

---

## 🛠 Сборка из исходников

Если вы хотите собрать лаунчер самостоятельно, потребуется [Rust](https://rustup.rs/):

```bash
# Клонировать репозиторий
git clone https://github.com/delio-no/open-ew-launcher.git
cd open-ew-launcher

# Собрать релизную версию
cargo build --release
```

- **Linux**: готовый файл будет в `target/release/open-ew-launcher`
- **Windows**: готовый файл будет в `target/release/open-ew-launcher.exe`

---

## 📂 Файл настроек

Все настройки сохраняются автоматически в `launcher.json`:
- **Linux**: `~/.config/open-ew-launcher/launcher.json`
- **Windows**: `%APPDATA%\open-ew-launcher\launcher.json`

*(Папку с файлом можно открыть в один клик прямо из лаунчера в левом нижнем углу)*.

---

## 📄 Лицензия

Проект распространяется под лицензией [MIT](LICENSE).
