//! Speech recognition backends.
//!
//! Every backend turns 16 kHz mono samples into timed words. Timestamps are relative to the
//! samples passed in; the pipeline maps them back onto the media timeline.

pub mod openai_api;
pub mod whisper;

use serde::{Deserialize, Serialize};

use crate::job::JobCtx;

/// Where speech recognition runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Device {
    Cpu,
    /// Vulkan device index as reported by [`whisper::list_gpus`].
    Gpu { index: i32 },
    /// Neural processing unit, reached through a local OpenAI-compatible
    /// transcription server (e.g. AMD Lemonade/FastFlowLM on Ryzen AI).
    Npu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    /// Greedy decoding – roughly twice as fast.
    Fast,
    /// Beam search – fewer errors, recommended for research transcripts.
    #[default]
    Precise,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrOptions {
    /// ISO-639-1 code, or `None` for automatic detection.
    pub language: Option<String>,
    pub quality: Quality,
    /// Keep filler words and disfluencies ("äh", "ähm", repetitions).
    pub verbatim: bool,
    /// Names and technical terms that should be spelled correctly.
    pub vocabulary: Vec<String>,
    pub threads: i32,
}

#[derive(Debug, Clone)]
pub struct RawWord {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub conf: f32,
}

pub struct AsrOutput {
    pub words: Vec<RawWord>,
    /// Detected or forced language (ISO-639-1).
    pub language: String,
}

pub trait AsrBackend: Send {
    /// Human readable name, stored in the transcript.
    fn describe(&self) -> String;

    fn transcribe(
        &mut self,
        samples: &[f32],
        opts: &AsrOptions,
        job: &JobCtx,
    ) -> anyhow::Result<AsrOutput>;
}

/// Builds the initial prompt that steers Whisper's style.
///
/// Whisper tends to "clean up" speech. Priming it with a disfluent example makes it keep
/// fillers, which is what verbatim research transcription needs. Vocabulary is appended so
/// that rare names are spelled as the user expects.
pub fn initial_prompt(opts: &AsrOptions) -> String {
    let lang = opts.language.as_deref().unwrap_or("");
    let mut prompt = String::new();
    if opts.verbatim {
        prompt.push_str(match lang {
            "de" => "Ähm, also, ich, äh, ich weiß nicht so genau, hm. Ja, mhm, genau.",
            "fr" => "Euh, alors, je, euh, je sais pas trop, hein. Bah, voilà.",
            "es" => "Eh, bueno, yo, eh, no sé muy bien, mmm. Pues, sí.",
            "it" => "Ehm, allora, io, eh, non so bene, mmh. Cioè, sì.",
            "nl" => "Eh, nou, ik, eh, ik weet het niet zo goed, hm. Ja, precies.",
            _ => "Umm, so, I, uh, I don't really know, hmm. Like, yeah.",
        });
    }
    if !opts.vocabulary.is_empty() {
        if !prompt.is_empty() {
            prompt.push(' ');
        }
        prompt.push_str(&opts.vocabulary.join(", "));
        prompt.push('.');
    }
    prompt
}

/// Filler words dropped when verbatim mode is off.
pub fn is_filler(word: &str) -> bool {
    let w: String = word
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    matches!(
        w.as_str(),
        "äh" | "ähm" | "öh" | "öhm" | "hm" | "hmm" | "mhm" | "ehm" | "eh" | "uh" | "um" | "umm"
            | "uhm" | "erm" | "euh" | "mmh"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fillers() {
        assert!(is_filler("Ähm,"));
        assert!(is_filler("mhm."));
        assert!(!is_filler("Ähnlich"));
    }

    #[test]
    fn prompt_combines_parts() {
        let opts = AsrOptions {
            language: Some("de".into()),
            quality: Quality::Fast,
            verbatim: true,
            vocabulary: vec!["Bourdieu".into(), "Habitus".into()],
            threads: 4,
        };
        let p = initial_prompt(&opts);
        assert!(p.starts_with("Ähm"));
        assert!(p.ends_with("Bourdieu, Habitus."));
    }
}
