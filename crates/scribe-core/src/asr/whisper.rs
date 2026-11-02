//! Local Whisper inference via whisper.cpp (CPU, or GPU through Vulkan).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context};
use whisper_rs::{
    DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext,
    WhisperContextParameters,
};

use super::{initial_prompt, AsrBackend, AsrOptions, AsrOutput, Device, Quality, RawWord};
use crate::audio::SAMPLE_RATE;
use crate::job::{JobCtx, Stage};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GpuInfo {
    pub index: i32,
    pub name: String,
    pub memory_mb: u64,
}

/// Asks a child process to list the GPUs, see [`list_gpus`].
const GPU_PROBE_ARG: &str = "--scribe-list-gpus";

/// Lets a child process started by [`list_gpus`] do its work and exit. Every executable that
/// calls `list_gpus` must call this first thing in `main`.
pub fn handle_gpu_probe() {
    if std::env::args().nth(1).as_deref() == Some(GPU_PROBE_ARG) {
        // A crash here is an expected outcome and must not show the user an error dialog.
        #[cfg(windows)]
        unsafe {
            extern "system" {
                fn SetErrorMode(mode: u32) -> u32;
            }
            const SEM_FAILCRITICALERRORS: u32 = 0x0001;
            const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
            SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
        }
        println!("{}", serde_json::to_string(&query_gpus()).unwrap_or_default());
        std::process::exit(0);
    }
}

/// GPUs usable by whisper.cpp. Empty when built without the `vulkan` feature or if none work.
///
/// ggml throws an uncaught C++ exception from this query where no Vulkan driver is installed,
/// which would abort the whole program, so it runs in a child process.
pub fn list_gpus() -> Vec<GpuInfo> {
    let output = std::env::current_exe()
        .and_then(|exe| crate::audio::background_command(exe).arg(GPU_PROBE_ARG).output());
    match output {
        Ok(out) if out.status.success() => serde_json::from_slice(&out.stdout).unwrap_or_default(),
        Ok(out) => {
            log::warn!("no usable Vulkan GPU ({})", out.status);
            Vec::new()
        }
        Err(e) => {
            log::warn!("could not check for GPUs: {e}");
            Vec::new()
        }
    }
}

fn query_gpus() -> Vec<GpuInfo> {
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

/// Alignment heads for DTW word timing. Files we cannot identify go without DTW, because
/// wrong heads would give wrong times; fine-tunes keep the architecture of their base model.
fn dtw_preset(model_path: &Path) -> Option<DtwModelPreset> {
    let name = model_path.file_name()?.to_string_lossy().to_lowercase();
    if name.contains("large-v3-turbo") {
        Some(DtwModelPreset::LargeV3Turbo)
    } else if name.contains("large-v3") {
        Some(DtwModelPreset::LargeV3)
    } else if name.contains("small") && !name.contains(".en") {
        Some(DtwModelPreset::Small)
    } else {
        None
    }
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
                    anyhow::bail!("this build of tiroScribe has no GPU support");
                }
                params.use_gpu(true).gpu_device(*index);
                // Flash attention speeds up GPUs but excludes DTW timing, which matters more
                // here. It also changes some recognised words, neither clearly better nor worse.
                params.flash_attn(dtw_preset(model_path).is_none());
            }
            Device::Npu => anyhow::bail!("Whisper.cpp cannot run on the NPU; use the NPU backend"),
        }
        if let Some(model_preset) = dtw_preset(model_path) {
            params.dtw_parameters(DtwParameters {
                mode: DtwMode::ModelPreset { model_preset },
                ..Default::default()
            });
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
        let (mut words, language) = self.decode(samples, opts, opts.language.as_deref(), true, job)?;

        // Each window is decoded with the text before it as context, which lets a sentence
        // repeat itself indefinitely. whisper.cpp only notices repeats within one window, so
        // the affected audio is decoded again without context. That also drops the initial
        // prompt there, but the alternative is losing the speech altogether.
        let loops = find_loops(&words);
        if !loops.is_empty() {
            let total = samples.len() as f64 / SAMPLE_RATE as f64;
            let mut fixed = Vec::with_capacity(words.len());
            let mut kept = 0;
            for (first, last) in loops {
                let start = words[first].start;
                let end = words.get(last).map_or(total, |w| w.start);
                log::warn!("repeated text at {start:.1}-{end:.1} s, decoding that part again");
                let at = |t: f64| ((t * SAMPLE_RATE as f64) as usize).min(samples.len());
                let (retry, _) =
                    self.decode(&samples[at(start)..at(end)], opts, Some(&language), false, job)?;
                fixed.extend_from_slice(&words[kept..first]);
                fixed.extend(retry.into_iter().map(|mut w| {
                    w.start += start;
                    w.end += start;
                    w
                }));
                kept = last;
            }
            fixed.extend_from_slice(&words[kept..]);
            words = fixed;
            if !find_loops(&words).is_empty() {
                log::warn!("repeated text remains after decoding again");
            }
        }
        Ok(AsrOutput { words, language })
    }
}

impl WhisperAsr {
    /// Runs whisper.cpp once. Returns the words and the detected or forced language.
    fn decode(
        &self,
        samples: &[f32],
        opts: &AsrOptions,
        language: Option<&str>,
        use_context: bool,
        job: &JobCtx,
    ) -> anyhow::Result<(Vec<RawWord>, String)> {
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
        params.set_language(Some(language.unwrap_or("auto")));
        params.set_token_timestamps(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        if !use_context {
            params.set_n_max_text_ctx(0);
        }
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
        Ok((collect_words(&state, eot), language))
    }
}

/// Longest phrase, in words, that [`find_loops`] looks for.
const LOOP_MAX_PHRASE: usize = 60;
/// Loops this close together, in words, are treated as one.
const LOOP_MERGE_GAP: usize = 5;

/// Word ranges `first..last` where the same phrase repeats more than people normally do.
/// People do say "sehr, sehr, sehr" or restart a few words. A false match only costs time,
/// since the audio is decoded again and real repetition comes back.
fn find_loops(words: &[RawWord]) -> Vec<(usize, usize)> {
    let keys: Vec<String> = words
        .iter()
        .map(|w| w.text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect())
        .collect();
    let mut loops: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < keys.len() {
        let mut longest = 0;
        for n in 1..=LOOP_MAX_PHRASE.min((keys.len() - i) / 2) {
            let mut reps = 1;
            while keys.get(i + (reps + 1) * n - 1).is_some()
                && keys[i..i + n] == keys[i + reps * n..i + (reps + 1) * n]
            {
                reps += 1;
            }
            let looping = (n >= 5 && reps >= 2) || (n >= 3 && reps >= 3) || reps >= 8;
            if looping {
                longest = longest.max(n * reps);
            }
        }
        if longest == 0 {
            i += 1;
            continue;
        }
        match loops.last_mut() {
            Some(prev) if i - prev.1 <= LOOP_MERGE_GAP => prev.1 = i + longest,
            _ => loops.push((i, i + longest)),
        }
        i += longest;
    }
    loops
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

/// DTW places tokens consistently this much (seconds) after the speech they belong to;
/// measured against clean speaker changes.
const DTW_LAG: f64 = 0.15;
/// Longest time a word may claim when its end comes from the start of the next word.
const DTW_MAX_WORD: f64 = 1.5;
/// Duration of a segment's last word if nothing better is known.
const DTW_LAST_WORD: f64 = 0.3;

fn collect_words(state: &whisper_rs::WhisperState, eot: i32) -> Vec<RawWord> {
    let mut words = Vec::new();
    let mut cur = WordBuilder::default();
    for segment in state.as_iter() {
        // Segment times are reliable bounds for the (noisier) token times.
        let seg_start = segment.start_timestamp() as f64 / 100.0;
        let seg_end = segment.end_timestamp() as f64 / 100.0;
        let first_word = words.len();
        let mut has_dtw = false;
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
            // t_dtw is negative where DTW is off or could not place the token.
            let raw_t0 = if data.t_dtw >= 0 {
                has_dtw = true;
                data.t_dtw as f64 / 100.0 - DTW_LAG
            } else {
                data.t0 as f64 / 100.0
            };
            let t0 = raw_t0.clamp(seg_start, seg_end);
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
        if has_dtw {
            dtw_word_ends(&mut words[first_word..], seg_end);
        }
    }
    cur.flush(&mut words);
    words
}

/// DTW only gives start times, so a word lasts until the next one starts.
fn dtw_word_ends(words: &mut [RawWord], seg_end: f64) {
    for i in 0..words.len() {
        let start = words[i].start;
        let end = match words.get(i + 1) {
            Some(next) => next.start.min(start + DTW_MAX_WORD),
            None => words[i].end.max(start + DTW_LAST_WORD).min(seg_end),
        };
        words[i].end = end.max(start);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(start: f64) -> RawWord {
        RawWord {
            text: "x".into(),
            start,
            end: start,
            conf: 1.0,
        }
    }

    fn text(s: &str) -> Vec<RawWord> {
        s.split_whitespace()
            .map(|t| RawWord { text: t.into(), start: 0.0, end: 0.0, conf: 1.0 })
            .collect()
    }

    #[test]
    fn finds_repeated_sentences() {
        let looped = format!("Das war so. {} Ende.", "Der Hund läuft durch den Park. ".repeat(4));
        assert_eq!(find_loops(&text(&looped)), [(3, 27)]);
    }

    #[test]
    fn merges_loops_next_to_each_other() {
        let a = "eins zwei drei vier fünf ".repeat(2);
        let b = "sechs sieben acht neun zehn ".repeat(2);
        assert_eq!(find_loops(&text(&format!("{a}{b}"))), [(0, 20)]);
    }

    #[test]
    fn keeps_normal_repetition() {
        assert!(find_loops(&text("sehr, sehr, sehr, sehr gut")).is_empty());
        assert!(find_loops(&text("ich weiß nicht, ich weiß nicht, was das ist")).is_empty());
        assert!(find_loops(&text("ja ja ja nein")).is_empty());
    }

    #[test]
    fn dtw_ends_come_from_the_next_word() {
        let mut words = [w(1.0), w(1.4), w(9.0)];
        dtw_word_ends(&mut words, 9.1);
        assert_eq!(words[0].end, 1.4);
        assert_eq!(words[1].end, 1.4 + DTW_MAX_WORD); // long pause: does not stretch across it
        assert_eq!(words[2].end, 9.1); // capped by the segment
    }

    #[test]
    fn dtw_only_for_known_models() {
        let preset = |n: &str| dtw_preset(Path::new(n));
        assert!(matches!(preset("/m/ggml-large-v3-turbo-german.bin"), Some(DtwModelPreset::LargeV3Turbo)));
        assert!(matches!(preset("ggml-large-v3.bin"), Some(DtwModelPreset::LargeV3)));
        assert!(preset("my-own-model.bin").is_none());
    }
}
