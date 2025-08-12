# Mondis

**Mondis** — это утилита на Rust для управления параметрами мониторов в Linux. На данном этапе реализована панель для регулировки яркости экрана.

## Реализованные возможности

- **Регулировка яркости**:
  - **Программная**: Изменение яркости экрана средствами операционной системы.
  - **Аппаратная (DDC/CI)**: Прямое управление яркостью мониторов, которые поддерживают протокол DDC/CI.

## Планируемые возможности

- Управление расположением мониторов (XRandR / Wayland).
- Точная настройка DPI.
- Управление контрастностью и цветовой температурой.
- Интеграция в системный трей для быстрого доступа.

## Структура проекта

Проект представляет собой Rust-воркспейс и разделен на несколько крейтов (пакетов):

- `mondis-core`: Ядро проекта, содержит основную логику и общие структуры данных.
- `mondis-ddc`: Модуль для взаимодействия с мониторами по протоколу DDC/CI.
- `mondis-x11`: Модуль для интеграции с X11/XRandR.
- `mondis-panel-direct`: Основное приложение с графическим интерфейсом.

## Установка

### 1. Установка Rust

Для сборки проекта вам понадобится `rustc` и `cargo`. Рекомендуемый способ установки — через `rustup`.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Эта команда скачает и запустит `rustup-init`, который установит последнюю стабильную версию Rust.

### 2. Системные зависимости

После установки Rust необходимо установить системные зависимости. Названия пакетов могут отличаться в разных дистрибутивах Linux.

**Для Debian / Ubuntu:**
```bash
# Основные инструменты для сборки
sudo apt install -y build-essential pkg-config curl git

# Зависимости для GUI (GTK4)
sudo apt install -y libgtk-4-dev

# Утилиты для взаимодействия с мониторами
# Программная яркость опирается на xrandr (нужен бинарник),
# аппаратная — на доступ к /dev/i2c-*
sudo apt install -y x11-xserver-utils i2c-tools

# (Опционально) ddcutil как внешняя утилита для диагностики
sudo apt install -y ddcutil
```

Примечания по зависимостям:
- Проект использует GTK4. Пакеты GTK3 (например, libgtk-3-dev) не требуются.
- Библиотека `libxrandr-dev` не нужна: используется именно утилита `xrandr` (бинарь),
  а не заголовки XRandR. Убедитесь, что `xrandr` доступен в PATH.
- Для аппаратного управления по DDC/CI требуется доступ к I2C-устройствам
  (`/dev/i2c-*`). На Debian/Ubuntu добавьте пользователя в группу `i2c`:
  ```bash
  sudo usermod -aG i2c $USER
  # затем выйдите и зайдите в сессию, либо выполните re-login
  ```
  При необходимости можно добавить udev-правило (пример):
  ```bash
  echo 'KERNEL=="i2c-[0-9]*", MODE="0660", GROUP="i2c"' | sudo tee /etc/udev/rules.d/60-i2c.rules
  sudo udevadm control --reload-rules && sudo udevadm trigger
  ```

**Для Arch Linux / Manjaro:**
```bash
sudo pacman -S --needed base-devel pkgconf gtk4 xorg-xrandr i2c-tools git
```

**Для Fedora:**
```bash
sudo dnf install -y @"Development Tools" @"Development Libraries" \
    gtk4-devel pkgconf-pkg-config xrandr i2c-tools git
```

**Для CentOS 8/9 Stream (RHEL 8/9):**
```bash
# Включить EPEL (для i2c-tools и сопутствующих пакетов)
sudo dnf install -y epel-release

# Включить дополнительный репозиторий с библиотеками
# CentOS Stream 8:
sudo dnf config-manager --set-enabled powertools
# CentOS Stream 9 / RHEL 9:
sudo dnf config-manager --set-enabled crb

# Наборы инструментов для сборки
sudo dnf groupinstall -y "Development Tools" "Development Libraries"

# Зависимости
sudo dnf install -y gtk4-devel pkgconf-pkg-config xrandr i2c-tools git
```

## Сборка и запуск

1.  **Клонируйте репозиторий:**
    ```bash
    git clone https://github.com/siv237/mondis.git
    cd mondis
    ```

2.  **Сборка проекта:**

    *   Для отладочной версии:
        ```bash
        cargo build
        ```
    *   Для оптимизированной релизной версии:
        ```bash
        cargo build --release
        ```

3.  **Запуск приложения:**

    Основной исполняемый файл находится в пакете `mondis-panel-direct`.
    Для отладки:
    ```bash
    cargo run -p mondis-panel-direct
    ```
    Для релизного запуска:
    ```bash
    cargo run -p mondis-panel-direct --release
    ```

4.  **Быстрая проверка кода:**

    Для проверки кода на ошибки без полной компиляции:
    ```bash
    cargo check
    ```

## Лицензия

Этот проект распространяется под лицензией [MIT](./LICENSE).

## Примечания по окружению (X11 / Wayland)

- Программная регулировка яркости через `xrandr` работает в X11-сессии.
- В Wayland-сессиях `xrandr` недоступен; понадобится альтернативная интеграция
  (например, протоколы порталов/композитора). В текущей версии под Wayland
  доступны только аппаратные методы через DDC/CI (если есть доступ к I2C).
