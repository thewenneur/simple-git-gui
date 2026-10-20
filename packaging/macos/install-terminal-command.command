#!/bin/sh
# Shipped in the .dmg as "Install Terminal Command": double-click it to add the
# `simple-git-gui` command, so `simple-git-gui .` opens the current folder.
# Installs in /usr/local/bin (asks for your password if needed), or in
# ~/.local/bin for an account without administrator rights.
set -u

launcher='#!/bin/sh
# Opens Simple Git; with a folder, opens that repository.
if [ $# -eq 0 ]; then
    exec open -n -a "Simple Git"
fi
dir=$(cd "$1" 2>/dev/null && pwd) || { echo "simple-git-gui: not a folder: $1" >&2; exit 1; }
exec open -n -a "Simple Git" --args "$dir"
'

tmp=$(mktemp)
printf '%s' "$launcher" > "$tmp"
chmod 755 "$tmp"

target=/usr/local/bin/simple-git-gui
if { mkdir -p /usr/local/bin 2>/dev/null && cp "$tmp" "$target" 2>/dev/null; } ||
    { echo "Your password is needed to install the command in /usr/local/bin."; \
      sudo mkdir -p /usr/local/bin && sudo cp "$tmp" "$target" && sudo chmod 755 "$target"; }; then
    :
else
    # No administrator rights: install for this account only.
    target="$HOME/.local/bin/simple-git-gui"
    mkdir -p "$HOME/.local/bin"
    cp "$tmp" "$target"
    if ! grep -q 'simple-git-gui: PATH' "$HOME/.zprofile" 2>/dev/null; then
        printf '\nexport PATH="$HOME/.local/bin:$PATH"  # simple-git-gui: PATH\n' >> "$HOME/.zprofile"
    fi
fi
rm -f "$tmp"

echo
echo "Done: $target"
echo "Open a new Terminal window and type: simple-git-gui ."
if ! [ -d "/Applications/Simple Git.app" ] && ! [ -d "$HOME/Applications/Simple Git.app" ]; then
    echo "Remember to drag Simple Git onto Applications first."
fi
