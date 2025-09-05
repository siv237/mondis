#!/usr/bin/env bash
# Mondis installer: builds binaries and enables tray autostart for current user
set -euo pipefail

# Colors
Y="\033[33m"; G="\033[32m"; R="\033[31m"; Z="\033[0m"

echo -e "${Y}==> Mondis installer starting...${Z}"

# Resolve repo root (this script is in scripts/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
echo -e "${Y}Repo root:${Z} ${REPO_ROOT}"

# Ensure required base tools
need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo -e "${R}Missing required command:$Z $1" >&2
    return 1
  fi
}

# Configure access to I2C devices for DDC/CI without sudo
configure_i2c_access() {
  echo -e "${Y}==> Configuring I2C access (DDC/CI)...${Z}"
  # Create udev rule to ensure group i2c and mode 0660 for /dev/i2c-*
  if has_cmd sudo; then
    echo 'KERNEL=="i2c-[0-9]*", GROUP="i2c", MODE="0660"' | sudo tee /etc/udev/rules.d/45-ddc-i2c.rules >/dev/null || true
    sudo udevadm control --reload || true
    sudo udevadm trigger || true
  else
    echo -e "${R}sudo not found; skipping udev rule creation. Create /etc/udev/rules.d/45-ddc-i2c.rules manually.${Z}"
  fi

  # Add the invoking user to i2c group
  TARGET_USER="${SUDO_USER:-$USER}"
  if id -nG "$TARGET_USER" | grep -qw i2c; then
    echo -e "${G}User already in i2c group:${Z} $TARGET_USER"
  else
    if has_cmd sudo; then
      sudo usermod -aG i2c "$TARGET_USER" || true
      echo -e "${Y}Added user to i2c group:${Z} $TARGET_USER (log out/in to take effect in new sessions)"
    else
      echo -e "${R}sudo not found; cannot add $TARGET_USER to i2c group automatically.${Z}"
    fi
  fi

  # Apply group/permissions to existing I2C device nodes for immediate use
  if has_cmd sudo; then
    sudo bash -lc 'shopt -s nullglob; for d in /dev/i2c-*; do chgrp i2c "$d" 2>/dev/null || true; chmod g+rw "$d" 2>/dev/null || true; done'
  fi
}

has_cmd() { command -v "$1" >/dev/null 2>&1; }

install_system_deps() {
  echo -e "${Y}==> Installing system dependencies (requires sudo)...${Z}"
  if ! has_cmd sudo; then
    echo -e "${R}sudo not found. Please install sudo or run dependencies install manually.${Z}"
    return 1
  fi
  if [ -r /etc/os-release ]; then
    . /etc/os-release
    ID_LIKE_LOWER=$(echo "${ID_LIKE:-}" | tr '[:upper:]' '[:lower:]')
    ID_LOWER=$(echo "${ID:-}" | tr '[:upper:]' '[:lower:]')
    case "$ID_LOWER" in
      ubuntu|debian|linuxmint|pop|neon|zorin|elementary|kali)
        echo -e "${Y}Detected Debian/Ubuntu family${Z}"
        sudo apt-get install -y \
          build-essential pkg-config curl git \
          libgtk-4-dev libdbus-1-dev libxrandr-dev
        # Optional runtime tool for hardware brightness via DDC/CI
        if ! dpkg -s ddcutil >/dev/null 2>&1; then
          echo -e "${Y}Note:${Z} You may want to 'sudo apt-get install -y ddcutil' for hardware brightness (optional)."
        fi
        ;;
      fedora|rhel|rocky|almalinux|centos)
        echo -e "${Y}Detected Fedora/RHEL family${Z}"
        sudo dnf -y groupinstall "Development Tools" || true
        sudo dnf -y install \
          gcc gcc-c++ make pkgconf-pkg-config curl git \
          gtk4-devel glib2-devel pango-devel gdk-pixbuf2-devel cairo-devel \
          libX11-devel libXrandr-devel dbus-devel ddcutil
        ;;
      arch|manjaro|endeavouros|arco|garuda)
        echo -e "${Y}Detected Arch family${Z}"
        sudo pacman -Syu --noconfirm
        sudo pacman -S --noconfirm --needed \
          base-devel pkgconf curl git \
          gtk4 glib2 pango gdk-pixbuf2 cairo libx11 libxrandr dbus ddcutil
        ;;
      opensuse*|suse|sle)
        echo -e "${Y}Detected openSUSE/SLE family${Z}"
        sudo zypper -n refresh
        sudo zypper -n install -t pattern devel_C_C++ || true
        sudo zypper -n install \
          gcc gcc-c++ make pkg-config curl git \
          gtk4-devel glib2-devel pango-devel gdk-pixbuf-devel cairo-devel \
          libX11-devel libXrandr-devel dbus-1-devel ddcutil
        ;;
      *)
        # Try by ID_LIKE
        if echo "$ID_LIKE_LOWER" | grep -q debian; then
          echo -e "${Y}Detected Debian-like via ID_LIKE${Z}"
          sudo apt-get install -y \
            build-essential pkg-config curl git \
            libgtk-4-dev libdbus-1-dev libxrandr-dev
          if ! command -v ddcutil >/dev/null 2>&1; then
            echo -e "${Y}Note:${Z} Consider installing ddcutil for DDC/CI support (optional)."
          fi
        elif echo "$ID_LIKE_LOWER" | grep -q rhel; then
          echo -e "${Y}Detected RHEL-like via ID_LIKE${Z}"
          sudo dnf -y groupinstall "Development Tools" || true
          sudo dnf -y install \
            gcc gcc-c++ make pkgconf-pkg-config curl git \
            gtk4-devel glib2-devel pango-devel gdk-pixbuf2-devel cairo-devel \
            libX11-devel libXrandr-devel dbus-devel ddcutil
        else
          echo -e "${R}Unsupported distro. Please install GTK4 dev stack and build tools manually.${Z}"
          return 1
        fi
        ;;
    esac
  else
    echo -e "${R}/etc/os-release not found. Cannot detect distro.${Z}"
    return 1
  fi
}

# 1) System dependencies (build tools, GTK4 dev, ddcutil)
install_system_deps || echo -e "${Y}Dependency installation skipped/failed; continuing if already satisfied...${Z}"

# 1b) Configure I2C access for DDC/CI
configure_i2c_access || true

# 2) Ensure Rust toolchain
if ! command -v cargo >/dev/null 2>&1; then
  echo -e "${Y}Rust (cargo) not found. Installing rustup...${Z}"
  need_cmd curl || { echo -e "${R}curl required to install rustup${Z}"; exit 1; }
  # Non-interactive install rustup to ~/.cargo
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  export PATH="$HOME/.cargo/bin:$PATH"
  echo -e "${G}Rust installed.${Z}"
else
  echo -e "${G}Rust found:${Z} $(cargo --version)"
fi

# 3) Build required crates in release
cd "$REPO_ROOT"
echo -e "${Y}Building mondis-tray and mondis-panel-direct (release)...${Z}"
"$HOME/.cargo/bin/cargo" build --release -p mondis-tray -p mondis-panel-direct

# 4) Install binaries to ~/.local/bin
INSTALL_BIN="$HOME/.local/bin"
mkdir -p "$INSTALL_BIN"
install -m 0755 "$REPO_ROOT/target/release/mondis-tray" "$INSTALL_BIN/" || true
# panel binary is optional; install if exists
if [ -f "$REPO_ROOT/target/release/mondis-panel-direct" ]; then
  install -m 0755 "$REPO_ROOT/target/release/mondis-panel-direct" "$INSTALL_BIN/" || true
fi

# 5) Create autostart .desktop for tray
AUTOSTART_DIR="$HOME/.config/autostart"
mkdir -p "$AUTOSTART_DIR"
DESKTOP_FILE="$AUTOSTART_DIR/mondis-tray.desktop"

ICON_NAME="display-brightness" # uses system icon theme
cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=Mondis Tray
Comment=Mondis system tray icon
Exec=$INSTALL_BIN/mondis-tray
TryExec=$INSTALL_BIN/mondis-tray
Icon=$ICON_NAME
Terminal=false
X-GNOME-Autostart-enabled=true
OnlyShowIn=XFCE;X-Cinnamon;GNOME;KDE;LXQt;LXDE;
X-KDE-autostart-after=panel
EOF

echo -e "${G}Installed binaries to:${Z} $INSTALL_BIN"
echo -e "${G}Created autostart entry:${Z} $DESKTOP_FILE"

# 5b) Create Applications menu launcher for manual start
APP_DIR="$HOME/.local/share/applications"
mkdir -p "$APP_DIR"
APP_DESKTOP_FILE="$APP_DIR/mondis-tray.desktop"
cat > "$APP_DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=Mondis Tray
Comment=Mondis system tray icon
Exec=$INSTALL_BIN/mondis-tray
TryExec=$INSTALL_BIN/mondis-tray
Icon=$ICON_NAME
Terminal=false
Categories=Utility;
StartupNotify=false
EOF
echo -e "${G}Created application launcher:${Z} $APP_DESKTOP_FILE"

# 6) Suggest adding ~/.local/bin to PATH if missing
case ":$PATH:" in
  *":$HOME/.local/bin:"*) : ;;
  *)
    echo -e "${Y}Note:${Z} ~/.local/bin is not in PATH. Add this line to your shell profile:"
    echo "  export PATH=\"$HOME/.local/bin:\$PATH\""
    ;;
esac

# 6b) Provide helper script to manually start tray if needed
START_HELPER="$INSTALL_BIN/mondis-tray-start"
cat > "$START_HELPER" <<'EOSH'
#!/usr/bin/env bash
set -euo pipefail
if command -v pgrep >/dev/null 2>&1 && pgrep -u "$USER" -x mondis-tray >/dev/null 2>&1; then
  echo "mondis-tray is already running"
  exit 0
fi
nohup "$HOME/.local/bin/mondis-tray" >/dev/null 2>&1 &
disown || true
echo "mondis-tray started"
EOSH
chmod +x "$START_HELPER"
echo -e "${G}Helper to start tray manually:${Z} $START_HELPER"

# 7) Start tray now if not already running
if command -v pgrep >/dev/null 2>&1; then
  if ! pgrep -u "$USER" -x mondis-tray >/dev/null 2>&1; then
    echo -e "${Y}Starting mondis-tray now...${Z}"
    nohup "$INSTALL_BIN/mondis-tray" >/dev/null 2>&1 &
    disown || true
  else
    echo -e "${G}mondis-tray is already running; not starting a second instance.${Z}"
  fi
fi

echo -e "${G}==> Mondis installation complete.${Z}"
echo -e "${Y}Tip:${Z} You can start the tray manually via: $START_HELPER or from your application menu (Mondis Tray)."
