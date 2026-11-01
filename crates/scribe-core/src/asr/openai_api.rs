//! Speech recognition through a local OpenAI-compatible `/audio/transcriptions` endpoint.
//!
//! This is how tiroScribe reaches the NPU: AMD's Lemonade server (FastFlowLM backend) runs
//! Whisper on Ryzen AI XDNA 2 NPUs and exposes this API. Any other compatible local server
//! (e.g. whisper.cpp's `whisper-server`, speaches) works as well.

use std::time::Duration;

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

use super::{initial_prompt, AsrBackend, AsrOptions, AsrOutput, RawWord};
use crate::audio::{encode_wav, quietest_cut, SAMPLE_RATE};
use crate::job::{JobCtx, Stage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Base URL including the API prefix, e.g. `http://localhost:8000/api/v1`.
    pub base_url: String,
    /// Model name as known to the server, e.g. `whisper-large-v3-turbo`.
    pub model: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8000/api/v1".into(),
            model: "whisper-large-v3-turbo".into(),
        }
    }
}

/// Chunk length sent per request. Most NPU Whisper implementations work on 30 s windows.
const CHUNK_SECS: usize = 28;

pub struct OpenAiAsr {
    config: ServerConfig,
    agent: ureq::Agent,
}

impl OpenAiAsr {
    pub fn new(config: ServerConfig) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(300)))
            .build()
            .into();
        Self { config, agent }
    }

    /// Checks that the server answers and returns the model ids it offers.
    pub fn probe(&self) -> anyhow::Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Models {
            data: Vec<Model>,
        }
        #[derive(Deserialize)]
        struct Model {
            id: String,
        }
        let url = format!("{}/models", self.config.base_url.trim_end_matches('/'));
        let models: Models = self
            .agent
            .get(&url)
            .config()
            .timeout_global(Some(Duration::from_secs(3)))
            .build()
            .call()
            .with_context(|| format!("cannot reach {url}"))?
            .body_mut()
            .read_json()?;
        Ok(models.data.into_iter().map(|m| m.id).collect())
    }

    fn request(
        &self,
        samples: &[f32],
        language: Option<&str>,
        prompt: &str,
    ) -> anyhow::Result<VerboseJson> {
        const BOUNDARY: &str = "----tiroScribeBoundary7MA4YWxkTrZu0gW";
        let mut body = Vec::new();
        let mut field = |name: &str, value: &str| {
            body.extend_from_slice(
                format!(
                    "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
                )
                .as_bytes(),
            );
        };
        field("model", &self.config.model);
        field("response_format", "verbose_json");
        field("timestamp_granularities[]", "word");
        field("timestamp_granularities[]", "segment");
        if let Some(lang) = language {
            field("language", lang);
        }
        if !prompt.is_empty() {
            field("prompt", prompt);
        }
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"chunk.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(&encode_wav(samples));
        body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

        let url = format!(
            "{}/audio/transcriptions",
            self.config.base_url.trim_end_matches('/')
        );
        let mut resp = self
            .agent
            .post(&url)
            .header(
                "Content-Type",
                &format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .send(&body[..])
            .with_context(|| format!("transcription request to {url} failed"))?;
        resp.body_mut()
            .read_json()
            .context("unexpected response from transcription server")
    }
}

#[derive(Debug, Deserialize)]
struct VerboseJson {
    #[serde(default)]
    text: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    words: Vec<ApiWord>,
    #[serde(default)]
    segments: Vec<ApiSegment>,
}

#[derive(Debug, Deserialize)]
struct ApiWord {
    word: String,
    start: f64,
    end: f64,
    #[serde(default)]
    probability: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct ApiSegment {
    start: f64,
    end: f64,
    text: String,
    #[serde(default)]
    avg_logprob: Option<f32>,
}

impl VerboseJson {
    /// Normalises the response into words, estimating timings when the server lacks them.
    fn into_words(self, offset: f64, chunk_len: f64) -> Vec<RawWord> {
        if !self.words.is_empty() {
            return self
                .words
                .into_iter()
                .filter(|w| !w.word.trim().is_empty())
                .map(|w| RawWord {
                    text: w.word.trim().to_string(),
                    start: offset + w.start,
                    end: offset + w.end,
                    conf: w.probability.unwrap_or(1.0),
                })
                .collect();
        }
        let segments = if self.segments.is_empty() {
            vec![ApiSegment {
                start: 0.0,
                end: chunk_len,
                text: self.text,
                avg_logprob: None,
            }]
        } else {
            self.segments
        };
        let mut out = Vec::new();
        for s in segments {
            let conf = s.avg_logprob.map(|lp| lp.exp()).unwrap_or(1.0);
            out.extend(spread_words(&s.text, offset + s.start, offset + s.end, conf));
        }
        out
    }
}

/// Distributes a segment's duration over its words, proportionally to their length.
fn spread_words(text: &str, start: f64, end: f64, conf: f32) -> Vec<RawWord> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let total: usize = tokens.iter().map(|t| t.chars().count() + 1).sum();
    let mut t = start;
    let per_char = (end - start).max(0.0) / total.max(1) as f64;
    tokens
        .into_iter()
        .map(|tok| {
            let len = (tok.chars().count() + 1) as f64 * per_char;
            let w = RawWord {
                text: tok.to_string(),
                start: t,
                end: t + len,
                conf,
            };
            t += len;
            w
        })
        .collect()
}

impl AsrBackend for OpenAiAsr {
    fn describe(&self) -> String {
        format!("NPU server {} ({})", self.config.base_url, self.config.model)
    }

    fn transcribe(
        &mut self,
        samples: &[f32],
        opts: &AsrOptions,
        job: &JobCtx,
    ) -> anyhow::Result<AsrOutput> {
        let sr = SAMPLE_RATE as usize;
        let base_prompt = initial_prompt(opts);
        let mut words: Vec<RawWord> = Vec::new();
        let mut language = opts.language.clone();
        let mut pos = 0;
        while pos < samples.len() {
            job.check()?;
            job.report(Stage::Transcribing, pos as f32 / samples.len() as f32);
            let target = pos + CHUNK_SECS * sr;
            let end = if target >= samples.len() {
                samples.len()
            } else {
                quietest_cut(samples, target, 2 * sr).max(pos + sr)
            };
            let chunk = &samples[pos..end];
            // Carry the tail of the previous text as context, like Whisper does internally.
            let context: Vec<&str> = words
                .iter()
                .rev()
                .take(40)
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let prompt = if context.is_empty() {
                base_prompt.clone()
            } else {
                context.join(" ")
            };
            let resp = self.request(chunk, language.as_deref(), &prompt)?;
            if language.is_none() {
                language = resp.language.clone().map(|l| normalise_language(&l));
            }
            words.extend(resp.into_words(pos as f64 / sr as f64, chunk.len() as f64 / sr as f64));
            pos = end;
        }
        if words.is_empty() && samples.len() > sr {
            bail!("the transcription server returned no text");
        }
        Ok(AsrOutput {
            words,
            language: language.unwrap_or_else(|| "und".into()),
        })
    }
}

/// OpenAI returns full language names ("german") in verbose_json; map common ones to codes.
fn normalise_language(lang: &str) -> String {
    let l = lang.to_lowercase();
    if l.len() == 2 {
        return l;
    }
    let id = whisper_rs::get_lang_id(&l);
    match id.and_then(whisper_rs::get_lang_str) {
        Some(code) => code.to_string(),
        None => (0..=whisper_rs::get_lang_max_id())
            .find(|&i| whisper_rs::get_lang_str_full(i) == Some(l.as_str()))
            .and_then(whisper_rs::get_lang_str)
            .unwrap_or("und")
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreads_evenly() {
        let w = spread_words("ab cd", 10.0, 12.0, 0.9);
        assert_eq!(w.len(), 2);
        assert!((w[0].start - 10.0).abs() < 1e-9);
        assert!((w[1].end - 12.0).abs() < 1e-9);
    }

    #[test]
    fn parses_verbose_json_with_words() {
        let json = r#"{"text":"Hallo Welt","language":"german",
            "words":[{"word":" Hallo","start":0.1,"end":0.5},{"word":" Welt","start":0.6,"end":1.0}]}"#;
        let v: VerboseJson = serde_json::from_str(json).unwrap();
        let w = v.into_words(5.0, 2.0);
        assert_eq!(w[0].text, "Hallo");
        assert!((w[1].start - 5.6).abs() < 1e-9);
        assert_eq!(normalise_language("german"), "de");
    }
}
