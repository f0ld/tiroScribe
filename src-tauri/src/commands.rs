//! Commands invoked from the frontend.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use scribe_core::asr::openai_api::{OpenAiAsr, ServerConfig};
use scribe_core::asr::whisper::{list_gpus, GpuInfo};
use scribe_core::export::{self, ExportOptions, Format};
use scribe_core::models::{self, ModelInfo};
use scribe_core::{audio, JobCtx, JobSpec, Transcript};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::queue::{JobView, Queue};

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    gpus: Vec<GpuInfo>,
    /// A neural processing unit is present (the Linux accel subsystem exposes it).
    npu_present: bool,
    ffmpeg: bool,
    models_dir: String,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        gpus: list_gpus(),
        npu_present: Path::new("/dev/accel/accel0").exists(),
        ffmpeg: audio::check_ffmpeg().is_ok(),
        models_dir: models::models_dir().to_string_lossy().into_owned(),
    }
}

/// Transcript passed on the command line, if any.
pub struct InitialFile(pub Option<String>);

#[tauri::command]
pub fn initial_file(initial: State<InitialFile>) -> Option<String> {
    initial.0.clone()
}

#[derive(Serialize)]
pub struct ModelStatus {
    #[serde(flatten)]
    info: ModelInfo,
    installed: bool,
}

#[tauri::command]
pub fn list_models() -> Vec<ModelStatus> {
    models::CATALOG
        .iter()
        .map(|m| ModelStatus {
            info: m.clone(),
            installed: models::is_installed(m),
        })
        .collect()
}

/// Cancellation flags of running downloads, by model id.
#[derive(Default)]
pub struct Downloads(Mutex<HashMap<String, Arc<AtomicBool>>>);

#[derive(Clone, Serialize)]
struct DownloadProgress {
    id: String,
    done: u64,
    total: u64,
}

#[derive(Clone, Serialize)]
struct DownloadFinished {
    id: String,
    error: Option<String>,
}

/// Downloads a model plus any missing essential helper models (VAD, diarization).
#[tauri::command]
pub fn download_model(app: AppHandle, downloads: State<Downloads>, id: String) -> CmdResult<()> {
    let model = models::find(&id).ok_or_else(|| format!("unknown model {id}"))?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut map = downloads.0.lock().unwrap();
        if map.contains_key(&id) {
            return Ok(()); // already downloading
        }
        map.insert(id.clone(), cancel.clone());
    }
    std::thread::spawn(move || {
        let ctx = JobCtx::new(cancel, Arc::new(|_| {}));
        let wanted = std::iter::once(model).chain(models::CATALOG.iter().filter(|m| m.essential));
        let mut result = Ok(());
        let mut last = Instant::now();
        for m in wanted {
            if models::is_installed(m) {
                continue;
            }
            let r = models::download(m, &ctx, |done, total| {
                if last.elapsed().as_millis() > 200 || done == total {
                    last = Instant::now();
                    let _ = app.emit(
                        "download-progress",
                        DownloadProgress {
                            id: m.id.to_string(),
                            done,
                            total,
                        },
                    );
                }
            });
            if let Err(e) = r {
                result = Err(format!("{e:#}"));
                break;
            }
        }
        app.state::<Downloads>().0.lock().unwrap().remove(model.id);
        let _ = app.emit(
            "download-finished",
            DownloadFinished {
                id: model.id.to_string(),
                error: result.err(),
            },
        );
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_download(downloads: State<Downloads>, id: String) {
    if let Some(flag) = downloads.0.lock().unwrap().get(&id) {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Checks that the NPU transcription server is reachable; returns its models.
#[tauri::command]
pub async fn probe_npu(config: ServerConfig) -> CmdResult<Vec<String>> {
    tauri::async_runtime::spawn_blocking(move || OpenAiAsr::new(config).probe())
        .await
        .map_err(err)?
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn enqueue_job(queue: State<Arc<Queue>>, spec: JobSpec) -> CmdResult<u64> {
    if !spec.media.is_file() {
        return Err(format!("{} does not exist", spec.media.display()));
    }
    Ok(queue.push(spec))
}

#[tauri::command]
pub fn list_jobs(queue: State<Arc<Queue>>) -> Vec<JobView> {
    queue.views()
}

#[tauri::command]
pub fn cancel_job(queue: State<Arc<Queue>>, id: u64) {
    queue.cancel(id);
}

#[tauri::command]
pub fn remove_job(queue: State<Arc<Queue>>, id: u64) {
    queue.remove(id);
}

#[tauri::command]
pub fn default_output_path(media: PathBuf) -> String {
    scribe_core::transcript::default_transcript_path(&media)
        .to_string_lossy()
        .into_owned()
}

/// Loads a transcript. If the recording is no longer at its stored path but a file of the same
/// name sits next to the transcript (folder moved or shared), that one is used.
#[tauri::command]
pub fn load_transcript(path: PathBuf) -> CmdResult<Transcript> {
    let mut t = Transcript::load(&path).map_err(|e| format!("{e:#}"))?;
    let media = Path::new(&t.media.path);
    if !media.is_absolute() || !media.exists() {
        let beside = media
            .file_name()
            .and_then(|name| path.parent().map(|dir| dir.join(name)));
        if let Some(candidate) = beside.filter(|c| c.exists()) {
            t.media.path = candidate.to_string_lossy().into_owned();
        }
    }
    Ok(t)
}

#[tauri::command]
pub fn save_transcript(path: PathBuf, transcript: Transcript) -> CmdResult<()> {
    transcript.save(&path).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn export_transcript(
    transcript: Transcript,
    format: Format,
    options: ExportOptions,
    path: PathBuf,
) -> CmdResult<()> {
    let text = export::render(&transcript, format, &options);
    std::fs::write(&path, text).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Formats WebKitGTK plays with the usual GStreamer plugins, and their MIME types.
const PLAYABLE: &[(&str, &str)] = &[
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("ogg", "audio/ogg"),
    ("oga", "audio/ogg"),
    ("opus", "audio/ogg"),
    ("flac", "audio/flac"),
];
/// Larger files (typically video) are transcoded to a compact audio-only copy instead.
const MAX_DIRECT_BYTES: u64 = 150 * 1024 * 1024;

/// Returns the audio for the editor as raw bytes; the first header line is its MIME type.
///
/// Audio goes through IPC and plays from a `blob:` URL because WebKitGTK cannot reliably
/// stream media from custom URL schemes. Unsupported or large media (e.g. video) is
/// transcoded once to Ogg/Opus in the cache directory; `force` does that for any format.
#[tauri::command]
pub async fn prepare_playback(
    app: AppHandle,
    media: PathBuf,
    force: bool,
) -> CmdResult<tauri::ipc::Response> {
    let ext = media
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let size = std::fs::metadata(&media)
        .map_err(|e| format!("{}: {e}", media.display()))?
        .len();
    let direct = PLAYABLE
        .iter()
        .find(|(e, _)| *e == ext)
        .filter(|_| !force && size <= MAX_DIRECT_BYTES);
    let cache = app.path().app_cache_dir().map_err(err)?.join("playback");

    let (path, mime) = match direct {
        Some((_, mime)) => (media, *mime),
        None => {
            // Cache key: FNV-1a of the path plus size, so a replaced file is re-encoded.
            let key = format!("{}:{size}", media.display())
                .bytes()
                .fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ b as u64).wrapping_mul(0x100000001b3)
                });
            let dst = cache.join(format!("{key:016x}.ogg"));
            if !dst.exists() {
                let dst = dst.clone();
                tauri::async_runtime::spawn_blocking(move || audio::make_playback_copy(&media, &dst))
                    .await
                    .map_err(err)?
                    .map_err(|e| format!("{e:#}"))?;
            }
            (dst, "audio/ogg")
        }
    };
    let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::with_capacity(data.len() + 32);
    out.extend_from_slice(mime.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(&data);
    Ok(tauri::ipc::Response::new(out))
}
