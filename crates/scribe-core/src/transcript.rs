//! The transcript document: the single source of truth that the editor reads and writes
//! and that every export format is rendered from.

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Bump when the on-disk layout changes incompatibly.
pub const FORMAT_VERSION: u32 = 1;

/// File suffix used for liveScribe transcripts.
pub const FILE_SUFFIX: &str = ".scribe.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub version: u32,
    pub media: MediaInfo,
    /// ISO-639-1 code of the spoken language, e.g. `de`.
    pub language: String,
    pub engine: EngineInfo,
    pub speakers: Vec<Speaker>,
    /// Speaker turns in chronological order.
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    pub path: String,
    /// Seconds.
    pub duration: f64,
    /// Offset of the transcribed range inside the media file (seconds). Word times are absolute.
    #[serde(default)]
    pub range_start: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineInfo {
    pub backend: String,
    pub model: String,
    /// Unix timestamp (seconds) of when the transcription finished.
    pub created: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Speaker {
    /// Stable id referenced by segments, e.g. `S1`.
    pub id: String,
    /// Display name, freely editable (e.g. "Interviewer").
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// `None` if diarization was disabled or the speaker is unknown.
    pub speaker: Option<String>,
    pub start: f64,
    pub end: f64,
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    /// The word including attached punctuation, without surrounding whitespace.
    pub text: String,
    pub start: f64,
    pub end: f64,
    /// Recognition confidence in `0..=1`. Edited words are set to `1.0`.
    pub conf: f32,
    /// True if another speaker talks at the same time.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub overlap: bool,
}

impl Segment {
    pub fn text(&self) -> String {
        join_words(self.words.iter().map(|w| w.text.as_str()))
    }
}

impl Transcript {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let t: Transcript = serde_json::from_slice(&data)
            .with_context(|| format!("parsing {}", path.display()))?;
        if t.version > FORMAT_VERSION {
            anyhow::bail!(
                "{} was written by a newer liveScribe (format v{}, supported v{})",
                path.display(),
                t.version,
                FORMAT_VERSION
            );
        }
        Ok(t)
    }

    /// Writes atomically (temp file + rename) so a crash never leaves a half-written transcript.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("tmp");
        let data = serde_json::to_vec_pretty(self)?;
        std::fs::write(&tmp, data).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }

    pub fn speaker_name(&self, id: Option<&str>) -> Option<&str> {
        let id = id?;
        self.speakers
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.as_str())
    }
}

/// Default transcript path for a media file: `interview.m4a` → `interview.scribe.json`.
pub fn default_transcript_path(media: &Path) -> std::path::PathBuf {
    let stem = media
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "transcript".into());
    media.with_file_name(format!("{stem}{FILE_SUFFIX}"))
}

/// Joins word tokens with spaces, without a space before closing punctuation.
pub fn join_words<'a>(words: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for w in words {
        if !out.is_empty() && !w.starts_with([',', '.', '!', '?', ':', ';', ')', '…']) {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_handles_punctuation() {
        assert_eq!(join_words(["Hallo", ",", "Welt", "."]), "Hallo, Welt.");
        assert_eq!(join_words(["Ja", "genau!"]), "Ja genau!");
    }

    #[test]
    fn default_path_replaces_extension() {
        let p = default_transcript_path(Path::new("/x/interview 1.m4a"));
        assert_eq!(p, Path::new("/x/interview 1.scribe.json"));
    }
}
