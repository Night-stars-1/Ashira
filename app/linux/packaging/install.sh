#!/bin/sh
# Installs this Ashira build for the current user: the files under ~/.local/share/ashira,
# a launcher in ~/.local/bin and a menu entry. Run it from the unpacked folder:
#   ./install.sh            install or update
#   ./install.sh --remove   uninstall
set -eu

here=$(cd "$(dirname "$0")" && pwd)
data="${XDG_DATA_HOME:-$HOME/.local/share}"
dest="$data/ashira"
bin="$HOME/.local/bin"
entry="$data/applications/dev.ashira.ashira_app.desktop"

if [ "${1:-}" = "--remove" ]; then
  rm -rf "$dest" "$bin/ashira-app" "$entry"
  echo "Ashira 已卸载"
  exit 0
fi

# Replaced as a whole so files a newer build dropped do not linger.
rm -rf "$dest.new"
mkdir -p "$dest.new" "$bin" "$data/applications"
cp -R "$here/ashira_app" "$here/data" "$here/lib" "$dest.new/"
rm -rf "$dest"
mv "$dest.new" "$dest"
ln -sf "$dest/ashira_app" "$bin/ashira-app"

sed -e "s|@EXEC@|$dest/ashira_app|" -e "s|@ICON@|$dest/data/app_icon.png|" \
  "$here/dev.ashira.ashira_app.desktop" > "$entry"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" >/dev/null 2>&1 || true

echo "Ashira 已安装到 $dest"
echo "从应用菜单打开，或运行 ashira-app（需要 $bin 在 PATH 中）"
