#!/usr/bin/env bash
# DarkShadow's Shell (Rust) — installer for Arch, CachyOS, Fedora, Debian/Ubuntu (NixOS: use the flake, see README).
#   ./install.sh            build, install to ~/.local, hook into Hyprland
#   ./install.sh --no-deps  skip the package manager
#   ./install.sh --uninstall
set -euo pipefail

cd "$(dirname "$0")"
BIN="$HOME/.local/bin"
SHARE="$HOME/.local/share/dshell"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}"
DEPS=1
for a in "$@"; do
  case "$a" in
    --no-deps) DEPS=0 ;;
    --uninstall)
      rm -f "$BIN/dshell"; rm -rf "$SHARE"
      echo "Removed dshell. Your config in $CONF/dshell stays; remove the require(\"dshell\") / source line from Hyprland yourself."
      exit 0 ;;
    -h|--help) sed -n 2,6p "$0"; exit 0 ;;
  esac
done

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }

if [ -f /etc/NIXOS ]; then
  echo "NixOS: add the flake module instead (see README.md, section NixOS)."; exit 1
fi

if [ "$DEPS" = 1 ]; then
  say "Installing build and runtime packages"
  if command -v pacman >/dev/null; then
    sudo pacman -S --needed --noconfirm rust pkgconf wayland libxkbcommon dbus wireplumber libpulse cava curl fontconfig xdg-utils grim slurp wf-recorder wl-clipboard cliphist
  elif command -v dnf >/dev/null; then
    sudo dnf install -y cargo rust pkgconf-pkg-config wayland-devel libxkbcommon-devel dbus-devel wireplumber pulseaudio-utils cava curl fontconfig xdg-utils grim slurp wf-recorder wl-clipboard cliphist
  elif command -v apt-get >/dev/null; then
    sudo apt-get install -y cargo pkg-config libwayland-dev libxkbcommon-dev libdbus-1-dev wireplumber pulseaudio-utils cava curl fontconfig xdg-utils grim slurp wf-recorder wl-clipboard cliphist
  else
    echo "Unknown package manager: install cargo, pkg-config, wayland, libxkbcommon, dbus, wireplumber, pactl, cava, curl yourself."
  fi
fi

command -v cargo >/dev/null || { echo "cargo is missing (install Rust: https://rustup.rs)"; exit 1; }

say "Building (release)"
cargo build --release --locked

say "Installing to $BIN and $SHARE"
mkdir -p "$BIN" "$SHARE"
install -m755 target/release/dshell "$BIN/dshell"
rm -rf "$SHARE/wallpapers" "$SHARE/config"
cp -r assets/wallpapers "$SHARE/"
cp -r config "$SHARE/"

WALL="$HOME/Pictures/Wallpapers"
if [ ! -d "$WALL" ] || [ -z "$(ls -A "$WALL" 2>/dev/null)" ]; then
  say "Copying the wallpapers to $WALL"
  mkdir -p "$WALL"
  find assets/wallpapers -name '*.jpg' ! -name default.jpg -exec cp {} "$WALL/" \;
fi

if [ ! -f "$CONF/dshell/config.toml" ]; then
  "$BIN/dshell" config >/dev/null && say "Wrote $CONF/dshell/config.toml"
fi

# Hyprland: 0.55+ uses hyprland.lua, older ones hyprland.conf
if [ -f "$CONF/hypr/hyprland.lua" ]; then
  cp config/hypr/dshell.lua "$CONF/hypr/dshell.lua"
  if ! grep -q 'require("dshell")' "$CONF/hypr/hyprland.lua"; then
    cp "$CONF/hypr/hyprland.lua" "$CONF/hypr/hyprland.lua.bak-dshell"
    printf '\n-- DarkShadow'"'"'s Shell (Rust)\nrequire("dshell")\n' >> "$CONF/hypr/hyprland.lua"
    say "Hyprland: added require(\"dshell\") (backup: hyprland.lua.bak-dshell)"
  fi
elif [ -f "$CONF/hypr/hyprland.conf" ]; then
  cp config/hypr/dshell.conf "$CONF/hypr/dshell.conf"
  if ! grep -q 'dshell.conf' "$CONF/hypr/hyprland.conf"; then
    cp "$CONF/hypr/hyprland.conf" "$CONF/hypr/hyprland.conf.bak-dshell"
    printf '\n# DarkShadow'"'"'s Shell (Rust)\nsource = %s/hypr/dshell.conf\n' "$CONF" >> "$CONF/hypr/hyprland.conf"
    say "Hyprland: added source = dshell.conf (backup: hyprland.conf.bak-dshell)"
  fi
fi

if [ -f "$CONF/driftwm/config.toml" ]; then
  say "driftwm: merge config/driftwm/dshell.toml into $CONF/driftwm/config.toml (autostart, blur rules, keys)"
fi

case ":$PATH:" in *":$BIN:"*) ;; *) echo "Add $BIN to your PATH." ;; esac
say "Done. Log out and back in (or run: dshell &). Keys: ALT+Space launcher, SUPER+N control center, SUPER+W wallpapers."
