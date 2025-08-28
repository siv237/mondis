# Протокол 06 — Скрипты установки и удаления

Статус: DONE

Цель: обеспечить простую установку и удаление Mondis на множестве Linux‑дистрибутивов с минимальными ручными шагами.

Сделано (scripts/install.sh):
- Определение дистрибутива через `/etc/os-release` и установка системных зависимостей:
  - Debian/Ubuntu: `apt-get install ... libgtk-4-dev libdbus-1-dev libxrandr-dev` (+ подсказка по `ddcutil`).
  - Fedora/RHEL: `dnf` (включая группы «Development Tools», `gtk4-devel`, `dbus-devel`, `ddcutil`).
  - Arch: `pacman` (`gtk4`, `libxrandr`, `dbus`, `ddcutil`, base-devel и др.).
  - openSUSE/SLE: `zypper` (паттерн `devel_C_C++`, `gtk4-devel`, `dbus-1-devel`, `ddcutil`).
  - Фоллбэк по `ID_LIKE` для похожих семейств.
- Настройка доступа к I2C для DDC/CI:
  - Создание udev‑правила `/etc/udev/rules.d/45-ddc-i2c.rules` (требует `sudo`) с группой `i2c` и правами `0660` для `/dev/i2c-*`.
  - Добавление пользователя в группу `i2c`, применение прав к уже существующим узлам устройств.
- Установка Rust (rustup) при отсутствии `cargo`.
- Сборка релизных бинарников: `mondis-tray` и `mondis-panel-direct` (`cargo build --release -p mondis-tray -p mondis-panel-direct`).
- Установка бинарников в `~/.local/bin` и добавление заметки о PATH.
- Создание автозапуска `~/.config/autostart/mondis-tray.desktop` и лаунчера `~/.local/share/applications/mondis-tray.desktop`.
- Генерация вспомогательного скрипта `~/.local/bin/mondis-tray-start` и автозапуск `mondis-tray`, если он ещё не запущен.

Удаление (scripts/uninstall.sh):
- Удаление автозапуска `~/.config/autostart/mondis-tray.desktop`.
- Опциональное удаление бинарников `~/.local/bin/mondis-tray` и `~/.local/bin/mondis-panel-direct` с интерактивным подтверждением.

Примечания:
- Скрипты используют `sudo` для системных действий (пакеты, udev, группы). При его отсутствии выводят информативные подсказки.
- Установка безопасна для пользователя: все артефакты помещаются в домашний каталог, изменения системы — только в пределах udev‑правила и групп (через `sudo`).
