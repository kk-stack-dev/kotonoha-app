#!/bin/sh
# Installs or updates Kotonoha on macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/kk-stack-dev/kotonoha-app/main/install.sh | sh
#
# Downloads the latest release with curl, checks it against the release's SHA256SUMS, puts Kotonoha.app
# in /Applications (or ~/Applications when that is not writable) and opens it. Nothing else is changed.

set -eu

REPO="kk-stack-dev/kotonoha-app"
BASE="${KOTONOHA_BASE:-https://github.com/$REPO/releases/latest/download}"
APP_DIR="${KOTONOHA_APP_DIR:-/Applications}"
ZIP="Kotonoha-mac.zip"

fail() {
  printf '\nインストールできませんでした: %s\n' "$1" >&2
  exit 1
}

[ "$(uname -s)" = Darwin ] || fail "これは macOS 用です。Windows では install.ps1 を使ってください"
major=$(sw_vers -productVersion | cut -d. -f1)
[ "$major" -ge 13 ] || fail "macOS 13 以降が必要です（この Mac は $(sw_vers -productVersion)）"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "Kotonoha をダウンロードしています"
curl -fL --progress-bar -o "$tmp/$ZIP" "$BASE/$ZIP" || fail "ダウンロードに失敗しました。インターネットの接続を確かめてください"
curl -fsSL -o "$tmp/SHA256SUMS" "$BASE/SHA256SUMS" || fail "SHA256SUMS を取得できませんでした"

expected=$(awk -v f="$ZIP" '$2 == f { print $1 }' "$tmp/SHA256SUMS")
actual=$(shasum -a 256 "$tmp/$ZIP" | awk '{ print $1 }')
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
  fail "ダウンロードしたファイルが壊れています。もう一度実行してください"
fi

ditto -x -k "$tmp/$ZIP" "$tmp/unpacked"
[ -d "$tmp/unpacked/Kotonoha.app" ] || fail "アプリが見つかりません"

if [ ! -w "$APP_DIR" ]; then
  APP_DIR="$HOME/Applications"
  mkdir -p "$APP_DIR"
fi

# Quit a running copy so it can be replaced
if pgrep -xq Kotonoha; then
  osascript -e 'quit app "Kotonoha"' >/dev/null 2>&1 || true
  sleep 1
fi

rm -rf "$APP_DIR/Kotonoha.app"
mv "$tmp/unpacked/Kotonoha.app" "$APP_DIR/"
echo "インストールしました: $APP_DIR/Kotonoha.app"
open "$APP_DIR/Kotonoha.app"
echo "セットアップ画面の案内に沿って進めてください"
