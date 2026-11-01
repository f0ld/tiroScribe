#!/bin/sh
# Builds the release AppImage inside the container from Containerfile.
# usage: build-release.sh <source dir> <output dir>
set -eu
SRC=$1
OUT=$2
WORK=/work/src
HERE=$(dirname "$(readlink -f "$0")")

mkdir -p "$WORK" "$OUT"
# Work on a copy, so that the container's node_modules and builds stay out of the checkout.
tar -C "$SRC" --exclude=./target --exclude=./samples --exclude=./dist --exclude=./.git \
    --exclude=node_modules --exclude=./ui/dist --exclude=./src-tauri/binaries -cf - . | tar -C "$WORK" -xf -
cd "$WORK"

TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
FFMPEG=/work/ffmpeg
[ -x "$FFMPEG/ffmpeg" ] || sh "$HERE/build-ffmpeg.sh" "$FFMPEG"
mkdir -p src-tauri/binaries
cp "$FFMPEG/ffmpeg" "src-tauri/binaries/ffmpeg-$TRIPLE"
cp "$FFMPEG/ffprobe" "src-tauri/binaries/ffprobe-$TRIPLE"

npm --prefix ui ci
export CARGO_TARGET_DIR=/work/target APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=true
export GSTREAMER_PLUGINS_DIR=/usr/lib/x86_64-linux-gnu/gstreamer-1.0
export GSTREAMER_HELPERS_DIR=/usr/lib/x86_64-linux-gnu/gstreamer1.0/gstreamer-1.0
BUNDLE=$CARGO_TARGET_DIR/release/bundle/appimage
build() { ui/node_modules/.bin/tauri build --config src-tauri/tauri.release.conf.json; }

# The list of bundled system libraries needs a built AppImage, and is then bundled by a second pass.
python3 "$HERE/third_party_licenses.py" THIRD-PARTY-LICENSES.txt
build
rm -rf /tmp/appdir && mkdir /tmp/appdir
(cd /tmp/appdir && "$(ls "$BUNDLE"/*.AppImage)" --appimage-extract >/dev/null)
python3 "$HERE/third_party_licenses.py" THIRD-PARTY-LICENSES.txt /tmp/appdir/squashfs-root
build

cp "$BUNDLE"/*.AppImage THIRD-PARTY-LICENSES.txt "$FFMPEG"/ffmpeg-*.tar.xz "$OUT/"
cp "$HERE/build-ffmpeg.sh" "$OUT/"
(cd "$OUT" && sha256sum ./*.AppImage > SHA256SUMS)
