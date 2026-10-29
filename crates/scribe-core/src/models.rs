//! Catalog of downloadable models and where they live on disk.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Serialize;

use crate::job::JobCtx;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelKind {
    Whisper,
    Vad,
    Segmentation,
    Embedding,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: &'static str,
    pub kind: ModelKind,
    pub name: &'static str,
    pub description: &'static str,
    pub url: &'static str,
    pub file: &'static str,
    pub size: u64,
    /// Required for every transcription (downloaded automatically).
    pub essential: bool,
}

pub const CATALOG: &[ModelInfo] = &[
    ModelInfo {
        id: "large-v3-turbo",
        kind: ModelKind::Whisper,
        name: "Whisper large-v3 turbo",
        description: "Best balance of speed and accuracy for ~100 languages. Recommended.",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        file: "ggml-large-v3-turbo.bin",
        size: 1_624_555_275,
        essential: false,
    },
    ModelInfo {
        id: "large-v3-turbo-german",
        kind: ModelKind::Whisper,
        name: "Whisper large-v3 turbo – German",
        description: "Fine-tuned on German speech by primeLine (4.8 % WER). Best choice for German interviews.",
        url: "https://huggingface.co/cstr/whisper-large-v3-turbo-german-ggml/resolve/main/ggml-model.bin",
        file: "ggml-large-v3-turbo-german.bin",
        size: 1_624_555_275,
        essential: false,
    },
    ModelInfo {
        id: "large-v3",
        kind: ModelKind::Whisper,
        name: "Whisper large-v3",
        description: "Highest accuracy, about 4× slower than turbo. Needs ~4 GB of (V)RAM.",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
        file: "ggml-large-v3.bin",
        size: 3_095_033_483,
        essential: false,
    },
    ModelInfo {
        id: "large-v3-turbo-q5",
        kind: ModelKind::Whisper,
        name: "Whisper large-v3 turbo (quantised)",
        description: "Turbo at a third of the size; slightly less accurate. Good for laptops.",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        file: "ggml-large-v3-turbo-q5_0.bin",
        size: 574_041_195,
        essential: false,
    },
    ModelInfo {
        id: "small",
        kind: ModelKind::Whisper,
        name: "Whisper small",
        description: "Fast draft quality, useful for testing.",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        file: "ggml-small.bin",
        size: 487_601_967,
        essential: false,
    },
    ModelInfo {
        id: "silero-vad",
        kind: ModelKind::Vad,
        name: "Silero VAD v5.1.2",
        description: "Detects speech; prevents hallucinated text in silent passages.",
        url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin",
        file: "ggml-silero-v5.1.2.bin",
        size: 885_098,
        essential: true,
    },
    ModelInfo {
        id: "pyannote-segmentation-3.0",
        kind: ModelKind::Segmentation,
        name: "pyannote segmentation 3.0",
        description: "Finds speaker changes and overlapping speech.",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx",
        file: "pyannote-segmentation-3.0.onnx",
        size: 5_992_913,
        essential: true,
    },
    ModelInfo {
        id: "campplus-common",
        kind: ModelKind::Embedding,
        name: "3D-Speaker CAM++ (common, advanced)",
        description: "Voice fingerprints used to tell speakers apart. Trained on 200k speakers.",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx",
        file: "3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx",
        size: 28_281_164,
        essential: true,
    },
];

pub fn find(id: &str) -> Option<&'static ModelInfo> {
    CATALOG.iter().find(|m| m.id == id)
}

/// `~/.local/share/liveScribe/models` on Linux. Overridable via `LIVESCRIBE_MODELS`.
pub fn models_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LIVESCRIBE_MODELS") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("liveScribe")
        .join("models")
}

pub fn path_of(model: &ModelInfo) -> PathBuf {
    models_dir().join(model.file)
}

pub fn is_installed(model: &ModelInfo) -> bool {
    std::fs::metadata(path_of(model))
        .map(|m| m.len() == model.size)
        .unwrap_or(false)
}

/// Resolves a model id from the catalog or a path to a custom ggml file.
pub fn resolve_whisper(id_or_path: &str) -> anyhow::Result<PathBuf> {
    if let Some(m) = find(id_or_path) {
        if m.kind != ModelKind::Whisper {
            bail!("{id_or_path} is not a Whisper model");
        }
        let p = path_of(m);
        if !is_installed(m) {
            bail!("the model \"{}\" is not downloaded yet", m.name);
        }
        return Ok(p);
    }
    let p = PathBuf::from(id_or_path);
    if p.is_file() {
        Ok(p)
    } else {
        bail!("unknown model \"{id_or_path}\"")
    }
}

/// Path to the installed model of the given kind (the first one in the catalog).
pub fn installed_of_kind(kind: ModelKind) -> anyhow::Result<PathBuf> {
    CATALOG
        .iter()
        .filter(|m| m.kind == kind)
        .find(|m| is_installed(m))
        .map(path_of)
        .with_context(|| format!("no {kind:?} model is installed – download it first"))
}

/// Downloads a model with progress (`bytes_done`, `bytes_total`). Resumable via `.part` file.
pub fn download(
    model: &ModelInfo,
    job: &JobCtx,
    mut on_progress: impl FnMut(u64, u64),
) -> anyhow::Result<PathBuf> {
    let dest = path_of(model);
    if is_installed(model) {
        return Ok(dest);
    }
    std::fs::create_dir_all(models_dir())?;
    let part = dest.with_extension("part");
    let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    let mut req = ureq::get(model.url);
    if have > 0 && have < model.size {
        req = req.header("Range", &format!("bytes={have}-"));
    }
    let mut resp = req
        .call()
        .with_context(|| format!("downloading {}", model.url))?;
    let resumed = resp.status() == 206;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part)?;
    let mut done = if resumed { have } else { 0 };

    let mut reader = resp.body_mut().as_reader();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        if job.is_cancelled() {
            bail!(crate::job::Cancelled);
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        on_progress(done, model.size);
    }
    file.flush()?;
    drop(file);
    verify_size(&part, model.size)?;
    std::fs::rename(&part, &dest)?;
    Ok(dest)
}

fn verify_size(path: &Path, expected: u64) -> anyhow::Result<()> {
    let got = File::open(path)?.metadata()?.len();
    if got != expected {
        bail!("download incomplete ({got} of {expected} bytes) – please retry");
    }
    Ok(())
}
