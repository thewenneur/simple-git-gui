#!/bin/sh
# Installs Simple Git for the current user (no root needed):
# the program in ~/.local/bin, plus a menu entry and its icon.
# ./install.sh --uninstall removes them.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
id=io.github.thewenneur.simple-git-gui
bin="$HOME/.local/bin"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
apps="$data/applications"
icons="$data/icons/hicolor"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin/simple-git-gui" "$apps/$id.desktop" \
        "$icons/256x256/apps/$id.png" "$icons/scalable/apps/$id.svg"
    echo "Simple Git removed."
    exit 0
fi

mkdir -p "$bin" "$apps" "$icons/256x256/apps" "$icons/scalable/apps"
install -m 755 "$here/simple-git-gui" "$bin/simple-git-gui"
install -m 644 "$here/$id.png" "$icons/256x256/apps/$id.png"
install -m 644 "$here/$id.svg" "$icons/scalable/apps/$id.svg"
# Full path: ~/.local/bin is not always in the PATH of the desktop session.
sed "s|^Exec=simple-git-gui|Exec=\"$bin/simple-git-gui\"|" "$here/$id.desktop" > "$apps/$id.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$apps" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$icons" 2>/dev/null || true

# The `simple-git-gui` command: most distributions put ~/.local/bin in the
# PATH at login when it exists; add it otherwise.
case ":$PATH:" in
    *":$bin:"*) ;;
    *)
        if ! grep -q 'simple-git-gui: PATH' "$HOME/.profile" 2>/dev/null; then
            printf '\nexport PATH="$HOME/.local/bin:$PATH"  # simple-git-gui: PATH\n' >> "$HOME/.profile"
        fi
        echo "Log out and back in (or open a new login shell) for the simple-git-gui command."
        ;;
esac

echo "Simple Git installed: find it in your applications menu, or type 'simple-git-gui .' in a terminal to open the current folder."
command -v git >/dev/null || echo "Note: Git is not installed yet. Install it with your package manager (for example: sudo apt install git)."
