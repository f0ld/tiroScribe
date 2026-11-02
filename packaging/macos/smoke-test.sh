#!/bin/sh
# Installs the built app on a CI machine, starts it and takes a screenshot.
# usage: smoke-test.sh <disk image> <output dir>
set -eu
DMG=$1
OUT=$2
mkdir -p "$OUT"

MOUNT=$(mktemp -d)
hdiutil attach "$DMG" -nobrowse -mountpoint "$MOUNT"
cp -R "$MOUNT/tiroScribe.app" "$OUT/"
hdiutil detach "$MOUNT"
APP=$OUT/tiroScribe.app
for f in MacOS/tiroscribe MacOS/ffmpeg MacOS/ffprobe Resources/THIRD-PARTY-LICENSES.txt; do
  [ -e "$APP/Contents/$f" ] || { echo "missing after install: $f"; exit 1; }
done
codesign --verify --deep --strict "$APP"
EXE=$APP/Contents/MacOS/tiroscribe
FFMPEG=$APP/Contents/MacOS/ffmpeg

# The bundled tools must not need anything from this machine's Homebrew.
for tool in "$FFMPEG" "$APP/Contents/MacOS/ffprobe" "$EXE"; do
  if otool -L "$tool" | grep -E '/opt/homebrew|/usr/local'; then
    echo "$tool links to a library users do not have"
    exit 1
  fi
done

# One second of silence as 16 kHz mono WAV, decoded and encoded the way the app does.
WAV=$OUT/silence.wav
{ printf 'RIFF\044\175\000\000WAVEfmt \020\000\000\000\001\000\001\000\200\076\000\000\000\175\000\000\002\000\020\000data\000\175\000\000'
  head -c 32000 /dev/zero; } > "$WAV"
"$FFMPEG" -nostdin -y -loglevel error -i "$WAV" -ac 1 -ar 16000 -f f32le "$OUT/decoded.raw"
SIZE=$(wc -c < "$OUT/decoded.raw" | tr -d ' ')
[ "$SIZE" = 64000 ] || { echo "ffmpeg decoded the wrong size: $SIZE"; exit 1; }
"$FFMPEG" -nostdin -y -loglevel error -i "$WAV" -vn -ac 1 -c:a aac -b:a 48k "$OUT/encoded.m4a"
[ -s "$OUT/encoded.m4a" ] || { echo 'ffmpeg wrote an empty AAC file'; exit 1; }
echo 'ffmpeg works'

"$EXE" --scribe-list-gpus > "$OUT/gpus.json"
echo "GPU check: $(cat "$OUT/gpus.json")"

"$EXE" > "$OUT/app.log" 2>&1 &
PID=$!
sleep 30
screencapture -x "$OUT/screenshot.png" || echo 'no screenshot'
kill -0 "$PID" || { echo 'the app exited early'; cat "$OUT/app.log"; exit 1; }
echo 'app is running after 30 s'
kill "$PID"
