//! Local Whisper inference via whisper.cpp (CPU, or GPU through Vulkan).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::{initial_prompt, AsrBackend, AsrOptions, AsrOutput, Device, Quality, RawWord};
use crate::job::{JobCtx, Stage};

#[derive(Debug, Clone, serde::Serialize)]
pub struct GpuInfo {
    pub index: i32,
    pub name: String,
    pub memory_mb: u64,
}

/// GPUs usable by whisper.cpp. Empty when built without the `vulkan` feature.
pub fn list_gpus() -> Vec<GpuInfo> {
    #[cfg(feature = "vulkan")]
    {
        whisper_rs::vulkan::list_devices()
            .into_iter()
            .map(|d| GpuInfo {
                index: d.id,
                name: d.name,
                memory_mb: (d.vram.total / (1024 * 1024)) as u64,
            })
            .collect()
    }
    #[cfg(not(feature = "vulkan"))]
    {
        Vec::new()
    }
}

/// Routes whisper.cpp / ggml logging into the `log` crate instead of stderr.
pub fn install_logging() {
    whisper_rs::install_logging_hooks();
}

pub struct WhisperAsr {
    model_path: PathBuf,
    device: Device,
    ctx: WhisperContext,
}

impl WhisperAsr {
    pub fn load(model_path: &Path, device: &Device) -> anyhow::Result<Self> {
        let mut params = WhisperContextParameters::default();
        match device {
            Device::Cpu => {
                params.use_gpu(false);
            }
            Device::Gpu { index } => {
                if !cfg!(feature = "vulkan") {
                    anyhow::bail!("this build of liveScribe has no GPU support");
                }
                params.use_gpu(true).gpu_device(*index);
                // Flash attention is a sizeable speed-up on GPUs and does not affect accuracy.
                params.flash_attn(true);
            }
            Device::Npu => anyhow::bail!("Whisper.cpp cannot run on the NPU; use the NPU backend"),
        }
        let ctx = WhisperContext::new_with_params(model_path, params)
            .map_err(|e| anyhow!("{e}"))
            .with_context(|| format!("loading Whisper model {}", model_path.display()))?;
        Ok(Self {
            model_path: model_path.to_path_buf(),
            device: device.clone(),
            ctx,
        })
    }

    /// Whether this instance can be reused for the given model and device.
    pub fn matches(&self, model_path: &Path, device: &Device) -> bool {
        self.model_path == model_path && &self.device == device
    }
}

impl AsrBackend for WhisperAsr {
    fn describe(&self) -> String {
        let dev = match &self.device {
            Device::Cpu => "CPU".to_string(),
            Device::Gpu { index } => format!("GPU {index}"),
            Device::Npu => "NPU".to_string(),
        };
        format!("whisper.cpp {} ({dev})", whisper_rs::get_whisper_version())
    }

    fn transcribe(
        &mut self,
        samples: &[f32],
        opts: &AsrOptions,
        job: &JobCtx,
    ) -> anyhow::Result<AsrOutput> {
        let mut state = self.ctx.create_state().map_err(|e| anyhow!("{e}"))?;

        let strategy = match opts.quality {
            Quality::Fast => SamplingStrategy::Greedy { best_of: 1 },
            Quality::Precise => SamplingStrategy::BeamSearch {
                beam_size: 5,
                patience: -1.0,
            },
        };
        let mut params = FullParams::new(strategy);
        params.set_n_threads(opts.threads.max(1));
        params.set_language(Some(opts.language.as_deref().unwrap_or("auto")));
        params.set_token_timestamps(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        let prompt = initial_prompt(opts);
        if !prompt.is_empty() {
            params.set_initial_prompt(&prompt);
        }

        let progress_job = job.clone();
        params.set_progress_callback_safe(move |pct: i32| {
            progress_job.report(Stage::Transcribing, pct as f32 / 100.0);
        });
        // whisper-rs 0.16's `set_abort_callback_safe` casts its user data to the wrong type
        // (undefined behaviour that randomly aborts GPU runs), so wire the flag up directly.
        let cancel = job.cancel_flag();
        unsafe {
            params.set_abort_callback(Some(abort_if_cancelled));
            params.set_abort_callback_user_data(Arc::as_ptr(&cancel) as *mut c_void);
        }

        let result = state.full(params, samples);
        drop(cancel); // must outlive `full`, which reads it through the raw pointer
        job.check()?;
        result.map_err(|e| anyhow!("Whisper failed: {e}"))?;

        let language = whisper_rs::get_lang_str(state.full_lang_id_from_state())
            .unwrap_or("und")
            .to_string();
        let eot = self.ctx.token_eot();
        let words = collect_words(&state, eot);
        Ok(AsrOutput { words, language })
    }
}

unsafe extern "C" fn abort_if_cancelled(user_data: *mut c_void) -> bool {
    // SAFETY: `user_data` points into the `Arc<AtomicBool>` held alive across `state.full`.
    unsafe { (*(user_data as *const AtomicBool)).load(Ordering::Relaxed) }
}

/// Accumulates one word from consecutive sub-word tokens.
#[derive(Default)]
struct WordBuilder {
    bytes: Vec<u8>,
    start: f64,
    end: f64,
    probs: Vec<f32>,
}

impl WordBuilder {
    fn flush(&mut self, out: &mut Vec<RawWord>) {
        if self.bytes.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&self.bytes).trim().to_string();
        if !text.is_empty() {
            let conf = self.probs.iter().sum::<f32>() / self.probs.len().max(1) as f32;
            out.push(RawWord {
                text,
                start: self.start,
                end: self.end.max(self.start),
                conf,
            });
        }
        *self = WordBuilder::default();
    }
}

fn collect_words(state: &whisper_rs::WhisperState, eot: i32) -> Vec<RawWord> {
    let mut words = Vec::new();
    let mut cur = WordBuilder::default();
    for segment in state.as_iter() {
        // Segment times are reliable bounds for the (noisier) token times.
        let seg_start = segment.start_timestamp() as f64 / 100.0;
        let seg_end = segment.end_timestamp() as f64 / 100.0;
        for i in 0..segment.n_tokens() {
            let Some(token) = segment.get_token(i) else {
                continue;
            };
            if token.token_id() >= eot {
                continue; // special and timestamp tokens
            }
            let Ok(bytes) = token.to_bytes() else {
                continue;
            };
            let data = token.token_data();
            let t0 = (data.t0 as f64 / 100.0).clamp(seg_start, seg_end);
            let t1 = (data.t1 as f64 / 100.0).clamp(t0, seg_end);
            // A token starting with a space begins a new word. Multi-byte characters (umlauts)
            // can be split across tokens, so the text is assembled from raw bytes.
            if bytes.first() == Some(&b' ') || cur.bytes.is_empty() {
                cur.flush(&mut words);
                cur.start = t0;
            }
            cur.bytes.extend_from_slice(bytes);
            cur.end = t1;
            cur.probs.push(token.token_probability());
        }
        // Whisper segments never split a word.
        cur.flush(&mut words);
    }
    cur.flush(&mut words);
    words
}
