//! Speaker diarization ("who spoke when") with pyannote segmentation, speaker embeddings and
//! clustering, run through sherpa-onnx.

use std::path::Path;

use anyhow::bail;
use serde::{Deserialize, Serialize};
use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Speakers {
    /// No speaker detection.
    Off,
    /// Estimate the number of speakers. Lower `threshold` → more speakers. The default suits
    /// the CAM++ embedding model; it was tuned on a two-person German interview.
    Auto { threshold: f32 },
    /// The number of speakers is known (most reliable).
    Fixed { count: u32 },
}

impl Default for Speakers {
    fn default() -> Self {
        Speakers::Auto { threshold: 0.8 }
    }
}

/// A stretch of audio attributed to one speaker. Times in seconds, relative to the input.
#[derive(Debug, Clone, Copy)]
pub struct Turn {
    pub start: f64,
    pub end: f64,
    pub speaker: i32,
}

pub fn diarize(
    samples: &[f32],
    segmentation_model: &Path,
    embedding_model: &Path,
    speakers: Speakers,
    threads: i32,
) -> anyhow::Result<Vec<Turn>> {
    let clustering = match speakers {
        Speakers::Off => return Ok(Vec::new()),
        Speakers::Auto { threshold } => FastClusteringConfig {
            num_clusters: -1,
            threshold,
            ..Default::default()
        },
        Speakers::Fixed { count } => FastClusteringConfig {
            num_clusters: count.max(1) as i32,
            ..Default::default()
        },
    };
    let config = OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(segmentation_model.to_string_lossy().into_owned()),
                ..Default::default()
            },
            num_threads: threads.max(1),
            ..Default::default()
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(embedding_model.to_string_lossy().into_owned()),
            num_threads: threads.max(1),
            ..Default::default()
        },
        clustering,
        // Interviews have short back-channel turns ("mhm", "ja") worth keeping.
        min_duration_on: 0.2,
        min_duration_off: 0.5,
    };
    let Some(sd) = OfflineSpeakerDiarization::create(&config) else {
        bail!("could not initialise speaker diarization (check the model files)");
    };
    let Some(result) = sd.process(samples) else {
        bail!("speaker diarization failed");
    };
    Ok(result
        .sort_by_start_time()
        .into_iter()
        .map(|s| Turn {
            start: s.start as f64,
            end: s.end as f64,
            speaker: s.speaker,
        })
        .collect())
}
