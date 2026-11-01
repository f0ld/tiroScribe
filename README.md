# tiroScribe

Private, local transcription of interviews for qualitative social research and journalism –
with speaker detection, an audio-synced correction editor and exports for analysis tools.
Inspired by [noScribe](https://github.com/kaixxx/noScribe), written in Rust.

Your recordings and transcripts stay on your computer: no cloud, no account, no telemetry.
The only network use is downloading models from Hugging Face and GitHub when you ask for them,
and, if you choose the NPU backend, sending audio to the server you configure. The default
address is on your own machine; a remote address would send your interviews there.

> **Early software.** Automatic transcripts contain errors, including wrong words and wrong
> speakers. Check them against the recording before you quote or analyse them. It has been
> developed and tested on Fedora with an AMD GPU; please report problems on other systems.

## Install (Linux)

Download the AppImage from the [releases page](https://github.com/f0ld/tiroScribe/releases), then:

```sh
chmod +x tiroScribe_*_amd64.AppImage
./tiroScribe_*_amd64.AppImage
```

Everything it needs is included, except the models, which it offers to download on first start.
Requirements: 64-bit x86 Linux with glibc 2.39 or newer (e.g. Ubuntu 24.04, Debian 13, Fedora 40), a
normal desktop, and FUSE 2 for running AppImages (on Ubuntu 24.04 and later: `sudo apt install libfuse2t64`;
or add `--appimage-extract-and-run`). A GPU with Vulkan
drivers is strongly recommended. The CPU works too, but is much slower. Allow about 2 GB of disk
space for a model. Very long recordings have not been tested much yet.

## Install (Windows)

> **Windows support is new.** It is built and started automatically on a test machine, but
> transcription and audio playback have not been tested on real Windows PCs yet.

Download `tiroScribe_*_x64-setup.exe` from the [releases page](https://github.com/f0ld/tiroScribe/releases)
and run it. The installer is not signed yet, so Windows may warn you: choose **More info → Run anyway**.
You can compare the file with `SHA256SUMS` on the releases page first.

Requirements: 64-bit Windows 10 or 11. The installer fetches Microsoft's WebView2 if it is missing. A
GPU with Vulkan drivers is strongly recommended; without one, tiroScribe runs on the CPU, which is much slower.

## Features

**Transcription**
- Whisper large-v3 (turbo) via whisper.cpp – about 100 languages, plus a German fine-tune
  (primeLine; its authors report 4.77 % WER on their test set) as the recommended model for
  German interviews
- Runs on the **GPU** (Vulkan: AMD, Intel, NVIDIA), the **CPU**, or the **NPU** through a local
  OpenAI-compatible server (AMD Ryzen AI via Lemonade/FastFlowLM – experimental)
- **Speaker detection** (pyannote segmentation 3.0 + 3D-Speaker CAM++ embeddings, via sherpa-onnx);
  give the number of speakers for best results, or let it estimate
- Silence detection (Silero VAD) before recognition – stops Whisper from inventing text in pauses
- **Verbatim mode** keeps “äh”, “ähm”, repetitions; otherwise fillers are removed
- **Vocabulary**: names and technical terms to spell correctly
- Overlapping speech, word timestamps and per-word confidence are kept
- Transcribe only a part of a recording (start/stop time)
- **Queue**: line up many recordings, e.g. overnight
- Any audio/video format ffmpeg can read

**Editor**
- Click a word to hear it; current word highlighted during playback; auto-follow
- Foot-pedal style keyboard control (f4/f5transkript habits): `F4` play/pause with
  automatic rewind on resume, `F3`/`F5` ±5 s, `F7`/`F8` speed
- Uncertain words underlined; `F9` jumps to the next one
- Double-click to correct a paragraph – word timings are realigned, so sync keeps working
- Split (`Alt`+click or `Alt`+`Enter`) and merge paragraphs, reassign speakers
- Rename, add and merge speakers; speaking-time share per speaker
- Search & replace, undo/redo, autosave (atomic writes)

**Export**
- HTML (opens in Word/LibreOffice), plain text, WebVTT, SRT
- Timestamps as `[00:01:38]` (noScribe style) or `#00:01:38-0#` (f4/MAXQDA style)
- Pause notation after Dresing & Pehl: `(.)` `(..)` `(...)` `(5)`; overlap as `//…//`

## Setup (Fedora)

```sh
sudo dnf install ffmpeg cmake clang-devel vulkan-headers vulkan-loader-devel glslc \
                 webkit2gtk4.1-devel gtk3-devel libsoup3-devel openssl-devel nodejs
npm --prefix ui install
```

Run the app in development mode (starts Vite and the Tauri window):

```sh
ui/node_modules/.bin/tauri dev
```

Development builds use the `ffmpeg` on your `PATH`. Build installable packages (RPM, DEB, AppImage):

```sh
ui/node_modules/.bin/tauri build
```

Release AppImages are built in a container, which also compiles the bundled ffmpeg. GitHub Actions
does this when a `v*` tag is pushed, and you can run the same build locally:

```sh
podman build -t tiroscribe-build packaging/linux
podman run --rm --security-opt label=disable --userns=keep-id:uid=0,gid=0 -v "$PWD:/src:ro" \
    -v "$PWD/dist:/out" -v tiroscribe-work:/work tiroscribe-build \
    sh /src/packaging/linux/build-release.sh /src /out
```

On first start, open **Models** and download a Whisper model; the helper models
(~35 MB) are fetched automatically. Models live in `~/.local/share/tiroScribe/models`
(override with `TIROSCRIBE_MODELS`).

## Command line

The same pipeline runs headless – useful for batch jobs and servers:

```sh
cargo run --release -p scribe-core --features cli --bin scribe-cli -- info
cargo run --release -p scribe-core --features cli --bin scribe-cli -- download large-v3-turbo-german
cargo run --release -p scribe-core --features cli --bin scribe-cli -- \
    transcribe interview.m4a -l de -s 2 -d gpu        # → interview.scribe.json
cargo run --release -p scribe-core --features cli --bin scribe-cli -- \
    export interview.scribe.json -f html -o interview.html
```

`tiroScribe path/to/interview.scribe.json` opens a transcript directly in the editor.

## Privacy notes

Recordings the editor cannot play directly, and files over 150 MB, are converted to an audio copy
in the app's cache folder (on Linux `~/.cache/io.github.f0ld.tiroscribe/playback`). Copies are deleted after
30 days without use, or at any time under **Settings → Cached audio**. Transcripts are saved where
you choose, next to the recording by default.

## NPU (experimental)

tiroScribe talks to the NPU through a local server that implements OpenAI's
`/audio/transcriptions` endpoint, sending ~28 s chunks cut at quiet points. On AMD Ryzen AI
(XDNA 2) under Linux that is [Lemonade Server](https://lemonade-server.ai) with the FastFlowLM
backend, which needs the `amdxdna` driver (kernel 7.0+), NPU firmware ≥ 1.1 and – on Fedora –
building XRT/FastFlowLM from source. Configure the server URL and model name under
**Settings → NPU** and use *Test connection*. Any other compatible server (whisper.cpp's
`whisper-server`, speaches, …) works the same way.

This backend has not yet been tested against a real NPU server. Speaker detection always runs
on the CPU – it takes a few seconds per hour of audio.

## Accuracy notes

Measured on a 6-minute German two-person interview (Radeon 890M, precise mode, turbo model):
transcription + diarization in ~65 s (≈ 6× real time). With the speaker count given,
CAM++ embeddings mislabel about 1.6 % of speech time; WeSpeaker ResNet34 (the usual
default elsewhere) got ~9 %, so CAM++ is the default here. These numbers come from one
recording with two clearly different voices, so expect less on harder audio.

Speakers are assigned in stages: DTW word timing (about 20–30 % slower than flash attention on
the GPU; used for the catalog's large-v3 and small models), then diarization turns, smoothing
inside sentences, and a voice check that moves whole sentences only when they clearly sound
like another speaker. `examples/realign.rs` and `examples/voicecheck.rs` re-run those steps. `cargo run --release -p scribe-core
--example diar` runs diarization alone for experimenting with models and thresholds.

## Architecture

```
crates/scribe-core     Rust library + scribe-cli
  audio.rs             ffmpeg decoding to 16 kHz mono, playback copies
  vad.rs               speech detection; packs speech, maps timestamps back exactly
  asr/whisper.rs       whisper.cpp (CPU / Vulkan GPU), word timing & confidence
  asr/openai_api.rs    NPU / server backend
  diarize.rs           sherpa-onnx pyannote + embedding clustering
  align.rs             words → speaker turns (overlap, sentence-aware gap handling)
  export.rs            HTML / TXT / VTT / SRT
  pipeline.rs          decode → VAD → ASR → diarize → align → .scribe.json
src-tauri              desktop shell: job queue, model downloads, commands
ui                     Svelte 5 frontend (editor, queue, models, settings)
```

The `.scribe.json` transcript stores speakers, paragraphs and every word with start/end time,
confidence and overlap flag. All exports are rendered from it.

Tests: `cargo test -p scribe-core` and `npm --prefix ui test`.

## Roadmap

- DOCX export and transcription rule presets (simple / extended Dresing & Pehl)
- Direct MAXQDA / ATLAS.ti exports with timestamp links
- German UI translation
- Anonymisation (pseudonymise names and places)
- Resume interrupted jobs
- Verify the NPU backend on Ryzen AI with Lemonade

## Licence

GPL-3.0-or-later. Releases include `THIRD-PARTY-LICENSES.txt`, and the source of the bundled
FFmpeg (LGPL) is attached to each release. Model licences: Whisper (MIT), primeLine German fine-tune (Apache-2.0),
pyannote segmentation (MIT), 3D-Speaker CAM++ (Apache-2.0), Silero VAD (MIT).
