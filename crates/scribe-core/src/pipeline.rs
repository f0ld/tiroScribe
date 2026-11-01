//! End-to-end transcription: decode → detect speech → recognise → diarize → align.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::align::build_segments_with_voice;
use crate::asr::openai_api::{OpenAiAsr, ServerConfig};
use crate::asr::whisper::WhisperAsr;
use crate::asr::{is_filler, AsrBackend, AsrOptions, Device, Quality, RawWord};
use crate::audio::{self, TimeRange};
use crate::diarize::{self, Speakers, VoiceEmbedder};
use crate::job::{JobCtx, Stage};
use crate::models::{self, ModelKind};
use crate::transcript::{EngineInfo, MediaInfo, Transcript, FORMAT_VERSION};
use crate::vad::{self, Packed};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSpec {
    pub media: PathBuf,
    pub output: PathBuf,
    #[serde(default)]
    pub range: TimeRange,
    /// ISO-639-1, or `None` for automatic detection.
    pub language: Option<String>,
    /// Catalog id or path to a ggml model file. Ignored for the NPU.
    pub model: String,
    pub device: Device,
    #[serde(default)]
    pub quality: Quality,
    #[serde(default)]
    pub verbatim: bool,
    #[serde(default)]
    pub vocabulary: Vec<String>,
    #[serde(default)]
    pub speakers: Speakers,
    /// Skip silence before recognition (strongly recommended).
    #[serde(default = "yes")]
    pub vad: bool,
    /// Used when `device` is the NPU.
    #[serde(default)]
    pub npu: ServerConfig,
}

fn yes() -> bool {
    true
}

pub fn default_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get().min(8) as i32)
        .unwrap_or(4)
}

/// Keeps loaded models between jobs; reloading a large model takes several seconds.
#[derive(Default)]
pub struct Engines {
    whisper: Option<WhisperAsr>,
    npu: Option<OpenAiAsr>,
}

impl Engines {
    fn backend(&mut self, spec: &JobSpec) -> anyhow::Result<&mut dyn AsrBackend> {
        if spec.device == Device::Npu {
            self.whisper = None; // free memory, the server does the work
            return Ok(self.npu.insert(OpenAiAsr::new(spec.npu.clone())));
        }
        let path = models::resolve_whisper(&spec.model)?;
        if !self
            .whisper
            .as_ref()
            .is_some_and(|w| w.matches(&path, &spec.device))
        {
            self.whisper = None; // drop first so two large models never coexist in memory
            self.whisper = Some(WhisperAsr::load(&path, &spec.device)?);
        }
        Ok(self.whisper.as_mut().unwrap())
    }
}

pub fn run(spec: &JobSpec, engines: &mut Engines, job: &JobCtx) -> anyhow::Result<Transcript> {
    let threads = default_threads();
    job.report(Stage::Loading, 0.0);
    let vad_model = if spec.vad {
        Some(models::installed_of_kind(ModelKind::Vad)?)
    } else {
        None
    };
    let diar_models = match spec.speakers {
        Speakers::Off => None,
        _ => Some((
            models::installed_of_kind(ModelKind::Segmentation)?,
            models::installed_of_kind(ModelKind::Embedding)?,
        )),
    };

    job.report(Stage::Decoding, 0.0);
    let duration = audio::probe_duration(&spec.media).unwrap_or(0.0);
    let audio = audio::decode(&spec.media, spec.range)?;
    job.check()?;

    job.report(Stage::DetectingSpeech, 0.0);
    let packed = match &vad_model {
        Some(m) => vad::detect_and_pack(&audio.samples, m, threads)?,
        None => Packed::identity(&audio.samples),
    };
    job.check()?;

    let opts = AsrOptions {
        language: spec.language.clone().filter(|l| !l.is_empty() && l != "auto"),
        quality: spec.quality,
        verbatim: spec.verbatim,
        vocabulary: spec.vocabulary.clone(),
        threads,
    };
    let (words, language, backend_name) = if packed.samples.is_empty() {
        (Vec::new(), opts.language.clone().unwrap_or("und".into()), String::new())
    } else {
        job.report(Stage::Loading, 0.5);
        let backend = engines.backend(spec)?;
        let name = backend.describe();
        job.report(Stage::Transcribing, 0.0);
        let out = backend.transcribe(&packed.samples, &opts, job)?;
        let words = postprocess(out.words, &packed, audio.offset, spec.verbatim);
        (words, out.language, name)
    };
    job.check()?;

    job.report(Stage::Diarizing, 0.0);
    let turns = match &diar_models {
        Some((seg, emb)) => {
            let mut turns = diarize::diarize(&audio.samples, seg, emb, spec.speakers, threads)?;
            for t in &mut turns {
                t.start += audio.offset;
                t.end += audio.offset;
            }
            turns
        }
        None => Vec::new(),
    };
    job.check()?;
    for t in &turns {
        log::debug!("turn {:7.2} {:7.2} speaker {}", t.start, t.end, t.speaker);
    }

    job.report(Stage::Finishing, 0.0);
    let embedder = match &diar_models {
        Some((_, emb)) if !turns.is_empty() => Some(VoiceEmbedder::new(emb, threads)?),
        _ => None,
    };
    let (speakers, segments) = build_segments_with_voice(&words, &turns, |start, end| {
        let at = |t: f64| (((t - audio.offset) * 16_000.0).max(0.0) as usize).min(audio.samples.len());
        embedder.as_ref()?.embed(&audio.samples[at(start)..at(end)])
    });
    let transcript = Transcript {
        version: FORMAT_VERSION,
        media: MediaInfo {
            path: std::fs::canonicalize(&spec.media)
                .unwrap_or_else(|_| spec.media.clone())
                .to_string_lossy()
                .into_owned(),
            duration: if duration > 0.0 {
                duration
            } else {
                audio.offset + audio.duration()
            },
            range_start: audio.offset,
        },
        language,
        engine: EngineInfo {
            backend: backend_name,
            model: model_label(spec),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        },
        speakers,
        segments,
    };
    transcript.save(&spec.output)?;
    job.report(Stage::Finishing, 1.0);
    Ok(transcript)
}

fn model_label(spec: &JobSpec) -> String {
    if spec.device == Device::Npu {
        return spec.npu.model.clone();
    }
    match models::find(&spec.model) {
        Some(m) => m.name.to_string(),
        None => Path::new(&spec.model)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| spec.model.clone()),
    }
}

/// Maps word times to the media timeline, removes fillers if requested and drops
/// Whisper's typical hallucinated credits.
fn postprocess(words: Vec<RawWord>, packed: &Packed, offset: f64, verbatim: bool) -> Vec<RawWord> {
    let mut out: Vec<RawWord> = words
        .into_iter()
        .filter(|w| verbatim || !is_filler(&w.text))
        .map(|mut w| {
            let start = packed.to_original(w.start);
            let end = packed.to_original(w.end).max(start);
            w.start = start + offset;
            w.end = end + offset;
            w
        })
        .collect();
    // Keep times monotonic; Whisper's token timestamps occasionally step backwards.
    for i in 1..out.len() {
        if out[i].start < out[i - 1].start {
            out[i].start = out[i - 1].start;
        }
        if out[i].end < out[i].start {
            out[i].end = out[i].start;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The GUI sends jobs in exactly this shape (see ui/src/lib/types.ts).
    #[test]
    fn deserializes_frontend_job() {
        let json = r#"{
            "media": "/x/a.m4a", "output": "/x/a.scribe.json",
            "range": { "start": null, "end": 90.5 },
            "language": null, "model": "large-v3-turbo-german",
            "device": { "kind": "gpu", "index": 0 }, "quality": "precise",
            "verbatim": true, "vocabulary": ["Bourdieu"],
            "speakers": { "mode": "fixed", "count": 2 }, "vad": true,
            "npu": { "base_url": "http://localhost:8000/api/v1", "model": "whisper" }
        }"#;
        let spec: JobSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.device, Device::Gpu { index: 0 });
        assert_eq!(spec.speakers, Speakers::Fixed { count: 2 });
        assert_eq!(spec.range.end, Some(90.5));
        let auto: Speakers = serde_json::from_str(r#"{"mode":"auto","threshold":0.8}"#).unwrap();
        assert_eq!(auto, Speakers::default());
        let npu: Device = serde_json::from_str(r#"{"kind":"npu"}"#).unwrap();
        assert_eq!(npu, Device::Npu);
    }
}
