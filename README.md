# liveScribe

Private, local transcription of interviews for qualitative social research and journalism –
with speaker detection, an audio-synced correction editor and exports for analysis tools.
Inspired by [noScribe](https://github.com/kaixxx/noScribe), written in Rust.

Nothing leaves your computer: no cloud, no account, no telemetry.

## Features

**Transcription**
- Whisper large-v3 (turbo) via whisper.cpp – about 100 languages, plus a German fine-tune
  (primeLine, 4.8 % WER) as the recommended model for German interviews
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

Build installable packages (RPM, DEB, AppImage):

```sh
ui/node_modules/.bin/tauri build
```

On first start, open **Models** and download a Whisper model; the helper models
(~35 MB) are fetched automatically. Models live in `~/.local/share/liveScribe/models`
(override with `LIVESCRIBE_MODELS`).

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

`liveScribe path/to/interview.scribe.json` opens a transcript directly in the editor.

## NPU (experimental)

liveScribe talks to the NPU through a local server that implements OpenAI's
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

GPL-3.0-or-later. Model licences: Whisper (MIT), primeLine German fine-tune (Apache-2.0),
pyannote segmentation (MIT), 3D-Speaker CAM++ (Apache-2.0), Silero VAD (MIT).
