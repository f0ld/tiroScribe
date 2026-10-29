//! Voice activity detection: removes long silences before recognition.
//!
//! Whisper hallucinates text in silence and noise ("Vielen Dank fürs Zuschauen"). We detect
//! speech with Silero VAD, transcribe only the speech regions packed back to back, and map all
//! timestamps back to the original timeline. whisper.cpp's built-in VAD cannot be used for this
//! because it only remaps segment times, not word times.

use std::path::Path;

use anyhow::anyhow;
use whisper_rs::{WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

use crate::audio::SAMPLE_RATE;

/// Silence inserted between packed speech regions so words don't run together.
const GAP_SECS: f64 = 0.3;
/// Padding kept around each detected speech region.
const PAD_SECS: f64 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Piece {
    /// Start in the packed audio (seconds).
    packed: f64,
    /// Start in the original audio (seconds).
    orig: f64,
    len: f64,
}

/// Speech-only audio plus the mapping back to original time.
pub struct Packed {
    pub samples: Vec<f32>,
    pieces: Vec<Piece>,
}

impl Packed {
    /// Unmodified audio (VAD disabled).
    pub fn identity(samples: &[f32]) -> Self {
        let len = samples.len() as f64 / SAMPLE_RATE as f64;
        Self {
            samples: samples.to_vec(),
            pieces: vec![Piece {
                packed: 0.0,
                orig: 0.0,
                len,
            }],
        }
    }

    /// Maps a time in the packed audio to the original timeline.
    pub fn to_original(&self, t: f64) -> f64 {
        let idx = self.pieces.partition_point(|p| p.packed <= t);
        let Some(p) = self.pieces.get(idx.saturating_sub(1)) else {
            return t;
        };
        // Inside the inserted gap after a piece, clamp to the end of that piece.
        p.orig + (t - p.packed).clamp(0.0, p.len)
    }

    /// Speech regions in original time, for diagnostics and UI.
    pub fn speech_regions(&self) -> Vec<(f64, f64)> {
        self.pieces.iter().map(|p| (p.orig, p.orig + p.len)).collect()
    }
}

/// Detects speech and packs it. `regions` are `(start, end)` seconds, possibly overlapping.
fn pack(samples: &[f32], regions: &[(f64, f64)]) -> Packed {
    let sr = SAMPLE_RATE as f64;
    let total = samples.len() as f64 / sr;
    // Pad, clamp and merge regions that touch.
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for &(s, e) in regions {
        let (s, e) = ((s - PAD_SECS).max(0.0), (e + PAD_SECS).min(total));
        if e <= s {
            continue;
        }
        match merged.last_mut() {
            Some(last) if s <= last.1 + GAP_SECS => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }

    let gap = vec![0.0f32; (GAP_SECS * sr) as usize];
    let mut out = Vec::new();
    let mut pieces = Vec::new();
    for (i, &(s, e)) in merged.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(&gap);
        }
        let (a, b) = ((s * sr) as usize, ((e * sr) as usize).min(samples.len()));
        pieces.push(Piece {
            packed: out.len() as f64 / sr,
            orig: a as f64 / sr,
            len: (b - a) as f64 / sr,
        });
        out.extend_from_slice(&samples[a..b]);
    }
    Packed {
        samples: out,
        pieces,
    }
}

pub fn detect_and_pack(samples: &[f32], model: &Path, threads: i32) -> anyhow::Result<Packed> {
    let mut ctx_params = WhisperVadContextParams::new();
    ctx_params.set_n_threads(threads.max(1));
    ctx_params.set_use_gpu(false);
    let mut ctx = WhisperVadContext::new(&model.to_string_lossy(), ctx_params)
        .map_err(|e| anyhow!("loading VAD model {}: {e}", model.display()))?;

    let mut params = WhisperVadParams::new();
    params.set_threshold(0.35); // permissive: quiet interviewees must not be cut
    params.set_min_speech_duration(200);
    params.set_min_silence_duration(1000);
    params.set_speech_pad(0); // we pad ourselves
    let segments = ctx
        .segments_from_samples(params, samples)
        .map_err(|e| anyhow!("voice activity detection failed: {e}"))?;
    let regions: Vec<(f64, f64)> = segments
        .map(|s| (s.start as f64 / 100.0, s.end as f64 / 100.0))
        .collect();
    Ok(pack(samples, &regions))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_times_back() {
        let samples = vec![0.0f32; 16_000 * 60];
        let p = pack(&samples, &[(10.0, 20.0), (40.0, 45.0)]);
        // first piece: orig 9.75..20.25 → packed 0..10.5
        assert!((p.to_original(0.0) - 9.75).abs() < 1e-3);
        assert!((p.to_original(5.0) - 14.75).abs() < 1e-3);
        // second piece starts after 10.5 s + gap
        let second = 10.5 + GAP_SECS;
        assert!((p.to_original(second + 1.0) - 40.75).abs() < 1e-3);
        // inside the gap: clamped to end of first piece
        assert!((p.to_original(10.6) - 20.25).abs() < 1e-3);
        assert_eq!(p.samples.len(), ((10.5 + GAP_SECS + 5.5) * 16_000.0) as usize);
    }

    #[test]
    fn merges_close_regions() {
        let samples = vec![0.0f32; 16_000 * 10];
        let p = pack(&samples, &[(1.0, 2.0), (2.5, 3.0)]);
        assert_eq!(p.speech_regions().len(), 1);
    }
}
