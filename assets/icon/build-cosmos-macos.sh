#!/bin/bash
set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
SOURCE="${ROOT}/assets/icon/cosmos-term.png"
OUTPUT="${ROOT}/assets/macos/Cosmos Term.app/Contents/Resources/cosmos-term.icns"
TEMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/cosmos-term-icon.XXXXXX")
ICONSET="${TEMP_DIR}/cosmos-term.iconset"
mkdir "${ICONSET}"

cleanup() {
  find "${ICONSET}" -type f -delete
  rmdir "${ICONSET}"
  rmdir "${TEMP_DIR}"
}
trap cleanup EXIT

render() {
  local size=$1
  local name=$2
  sips -z "${size}" "${size}" "${SOURCE}" \
    --out "${ICONSET}/${name}" >/dev/null
}

render 16 icon_16x16.png
render 32 icon_16x16@2x.png
render 32 icon_32x32.png
render 64 icon_32x32@2x.png
render 128 icon_128x128.png
render 256 icon_128x128@2x.png
render 256 icon_256x256.png
render 512 icon_256x256@2x.png
render 512 icon_512x512.png
render 1024 icon_512x512@2x.png

iconutil -c icns "${ICONSET}" -o "${OUTPUT}"
echo "${OUTPUT}"
