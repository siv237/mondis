#!/usr/bin/env bash
# Mondis uninstaller: removes autostart and optionally binaries
set -euo pipefail

Y="\033[33m"; G="\033[32m"; R="\033[31m"; Z="\033[0m"

echo -e "${Y}==> Mondis uninstaller starting...${Z}"

INSTALL_BIN="$HOME/.local/bin"
AUTOSTART_DIR="$HOME/.config/autostart"
DESKTOP_FILE="$AUTOSTART_DIR/mondis-tray.desktop"

if [ -f "$DESKTOP_FILE" ]; then
  rm -f "$DESKTOP_FILE"
  echo -e "${G}Removed autostart entry:${Z} $DESKTOP_FILE"
else
  echo -e "${Y}Autostart entry not found:${Z} $DESKTOP_FILE"
fi

# Remove binaries (ask user)
for bin in mondis-tray mondis-panel-direct; do
  if [ -f "$INSTALL_BIN/$bin" ]; then
    read -r -p "Remove $INSTALL_BIN/$bin? [y/N] " ans || true
    if [[ "${ans:-}" =~ ^[Yy]$ ]]; then
      rm -f "$INSTALL_BIN/$bin"
      echo -e "${G}Removed:${Z} $INSTALL_BIN/$bin"
    fi
  fi
done

echo -e "${G}==> Mondis uninstallation complete.${Z}"
