//! Catalog of downloadable models and where they live on disk.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Serialize;
use sha2::{Digest, Sha256};

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
    /// SHA-256 of the file, checked after every download.
    pub sha256: &'static str,
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
        sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
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
        sha256: "6eb2e025198a6cbac7bdb1e86e278f5de002e583aae7bdfcf5183ef8da16decd",
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
        sha256: "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
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
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
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
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
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
        sha256: "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf",
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
        sha256: "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079",
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
        sha256: "aa3cfc16963a10586a9393f5035d6d6b57e98d358b347f80c2a30bf4f00ceba2",
        size: 28_281_164,
        essential: true,
    },
];

pub fn find(id: &str) -> Option<&'static ModelInfo> {
    CATALOG.iter().find(|m| m.id == id)
}

/// `~/.local/share/tiroScribe/models` on Linux. Overridable via `TIROSCRIBE_MODELS`.
pub fn models_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("TIROSCRIBE_MODELS") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tiroScribe")
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
    verify_sha256(&part, model.sha256)?;
    std::fs::rename(&part, &dest)?;
    Ok(dest)
}

/// A corrupt or tampered file is removed, so that a retry starts over.
fn verify_sha256(path: &Path, expected: &str) -> anyhow::Result<()> {
    let got = sha256_hex(path)?;
    if got != expected {
        let _ = std::fs::remove_file(path);
        bail!("the downloaded file failed verification (SHA-256 {got}) – please retry");
    }
    Ok(())
}

fn sha256_hex(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn verify_size(path: &Path, expected: u64) -> anyhow::Result<()> {
    let got = File::open(path)?.metadata()?.len();
    if got != expected {
        bail!("download incomplete ({got} of {expected} bytes) – please retry");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_rejects_and_removes_wrong_files() {
        let dir = std::env::temp_dir().join(format!("scribe-sha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("model.part");
        std::fs::write(&file, b"abc").unwrap();
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(sha256_hex(&file).unwrap(), abc);
        verify_sha256(&file, abc).unwrap();
        assert!(verify_sha256(&file, "0000").is_err());
        assert!(!file.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_model_has_a_full_checksum() {
        for m in CATALOG {
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
        }
    }
}
