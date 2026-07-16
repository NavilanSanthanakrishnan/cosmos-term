#!/bin/bash
set -euo pipefail

TARGET_DIR=${1:-target}
DIST_DIR=${2:-dist}
APP_NAME="Cosmos Term.app"
APP_PATH="${DIST_DIR}/${APP_NAME}"
TEMPLATE="assets/macos/${APP_NAME}"

if [[ ! -x "${TARGET_DIR}/release/wezterm-gui" ]]; then
  echo "Missing ${TARGET_DIR}/release/wezterm-gui; run the release build first." >&2
  exit 1
fi

mkdir -p "${DIST_DIR}"
rm -rf "${APP_PATH}"
ditto "${TEMPLATE}" "${APP_PATH}"
mkdir -p "${APP_PATH}/Contents/MacOS" "${APP_PATH}/Contents/Resources"

install -m 0755 "${TARGET_DIR}/release/wezterm-gui" \
  "${APP_PATH}/Contents/MacOS/cosmos-term-gui"
install -m 0755 "${TARGET_DIR}/release/wezterm" \
  "${APP_PATH}/Contents/MacOS/cosmos-term"
install -m 0755 "${TARGET_DIR}/release/wezterm-mux-server" \
  "${APP_PATH}/Contents/MacOS/cosmos-term-mux-server"

if [[ -x "${TARGET_DIR}/release/strip-ansi-escapes" ]]; then
  install -m 0755 "${TARGET_DIR}/release/strip-ansi-escapes" \
    "${APP_PATH}/Contents/MacOS/strip-ansi-escapes"
fi

mkdir -p "${APP_PATH}/Contents/Resources/shell-integration"
sed \
  -e 's/WEZTERM_SHELL_/COSMOS_TERM_SHELL_/g' \
  -e 's/wezterm set-working-directory/cosmos-term set-working-directory/g' \
  assets/shell-integration/wezterm.sh \
  > "${APP_PATH}/Contents/Resources/shell-integration/cosmos-term.sh"
install -m 0644 assets/cosmos/cosmos.lua \
  "${APP_PATH}/Contents/Resources/cosmos.lua"
install -m 0644 assets/cosmos/keyboard-anchor.lua \
  "${APP_PATH}/Contents/Resources/keyboard-anchor.lua"

VERSION=$(git -c core.abbrev=8 show -s \
  --format=%cd-%h --date=format:%Y%m%d-%H%M%S)
plutil -replace CFBundleShortVersionString -string "${VERSION}" \
  "${APP_PATH}/Contents/Info.plist"
plutil -replace CFBundleVersion -string "$(date +%Y%m%d%H%M%S)" \
  "${APP_PATH}/Contents/Info.plist"

codesign --force --deep --sign - "${APP_PATH}"
codesign --verify --deep --strict "${APP_PATH}"

echo "${APP_PATH}"
