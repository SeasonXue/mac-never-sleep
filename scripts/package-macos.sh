#!/usr/bin/env bash
set -euo pipefail

# 在 macOS 上打包菜单栏应用。Linux CI 不会跑这个脚本。

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "请在 Mac 上运行: ./scripts/package-macos.sh"
  exit 1
fi

cargo build -p never-sleep --release

APP_NAME="熄屏待命"
DIST="$ROOT/dist/${APP_NAME}.app"
MACOS_DIR="$DIST/Contents/MacOS"
RES_DIR="$DIST/Contents/Resources"
BIN="$ROOT/target/release/never-sleep"

rm -rf "$DIST"
mkdir -p "$MACOS_DIR" "$RES_DIR"
cp "$BIN" "$MACOS_DIR/never-sleep"
cp "$ROOT/packaging/Info.plist" "$DIST/Contents/Info.plist"

# 可选 icns
if [[ -f "$ROOT/packaging/AppIcon.icns" ]]; then
  cp "$ROOT/packaging/AppIcon.icns" "$RES_DIR/AppIcon.icns"
fi

chmod +x "$MACOS_DIR/never-sleep"
echo "已生成 $DIST"
echo "打开：open \"$DIST\""
echo "也可把二进制放到 PATH：cp $BIN /usr/local/bin/never-sleep"
