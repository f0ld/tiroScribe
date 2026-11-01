#!/bin/sh
# Builds the ffmpeg and ffprobe that ship with the app: LGPL, every decoder, and only the
# encoders tiroScribe uses. usage: build-ffmpeg.sh <output dir>
# FFMPEG_EXTRA_CONFIGURE adds platform-specific configure flags.
set -eu
VERSION=8.1.3
SHA256=7138d28c96d9d3e3af4ee3d8cad72741f8ffb40da90c1112235dea3ecd3178a3
OUT=$1
TARBALL=ffmpeg-$VERSION.tar.xz

mkdir -p "$OUT" /tmp/ffmpeg-build
cd /tmp/ffmpeg-build
curl -fsSLO "https://ffmpeg.org/releases/$TARBALL"
echo "$SHA256  $TARBALL" | sha256sum -c -
tar -xJf "$TARBALL"
cd "ffmpeg-$VERSION"
./configure \
  --disable-debug --disable-doc --disable-ffplay --disable-network --disable-autodetect \
  --disable-shared --enable-static --pkg-config-flags=--static \
  --enable-libopus \
  --disable-encoders --enable-encoder=libopus,pcm_f32le \
  --disable-muxers --enable-muxer=ogg,pcm_f32le \
  --disable-devices --disable-hwaccels ${FFMPEG_EXTRA_CONFIGURE:-}
make -j"$(nproc)"
# Corresponding source and build recipe for the release, as the LGPL requires.
cp "../$TARBALL" "$OUT/"
EXE=$(ls ffmpeg.exe >/dev/null 2>&1 && echo .exe || true)
cp "ffmpeg$EXE" "ffprobe$EXE" "$OUT/"
strip "$OUT/ffmpeg$EXE" "$OUT/ffprobe$EXE"
