//! Media decoding via `ffmpeg`, which handles practically every audio/video container.
//! Release builds ship it next to the executable; otherwise the one on `PATH` is used.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context};

/// Sample rate expected by Whisper and the diarization models.
pub const SAMPLE_RATE: u32 = 16_000;

/// Mono 16 kHz PCM, normalised to `-1..=1`.
pub struct Audio {
    pub samples: Vec<f32>,
    /// Offset (seconds) of `samples[0]` within the source media.
    pub offset: f64,
}

impl Audio {
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / SAMPLE_RATE as f64
    }
}

/// Optional sub-range of the media to transcribe, in seconds.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct TimeRange {
    pub start: Option<f64>,
    pub end: Option<f64>,
}

/// The bundled copy of an ffmpeg tool if there is one, else its name for a `PATH` lookup.
fn tool(name: &str) -> PathBuf {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(&file)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(file))
}

pub fn check_ffmpeg() -> anyhow::Result<()> {
    let ok = Command::new(tool("ffmpeg"))
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        bail!("ffmpeg was not found – please install it with your package manager");
    }
    Ok(())
}

/// Duration of the media file in seconds, via `ffprobe`.
pub fn probe_duration(path: &Path) -> anyhow::Result<f64> {
    let out = Command::new(tool("ffprobe"))
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .context("running ffprobe")?;
    if !out.status.success() {
        bail!(
            "ffprobe failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .context("parsing media duration")
}

/// Decodes (a range of) any media file to 16 kHz mono f32.
pub fn decode(path: &Path, range: TimeRange) -> anyhow::Result<Audio> {
    check_ffmpeg()?;
    let mut cmd = Command::new(tool("ffmpeg"));
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error"]);
    let offset = range.start.unwrap_or(0.0).max(0.0);
    if offset > 0.0 {
        cmd.args(["-ss", &format!("{offset:.3}")]);
    }
    cmd.arg("-i").arg(path);
    if let Some(end) = range.end {
        let dur = end - offset;
        if dur <= 0.0 {
            bail!("the stop time must be after the start time");
        }
        cmd.args(["-t", &format!("{dur:.3}")]);
    }
    cmd.args(["-vn", "-ac", "1", "-ar", &SAMPLE_RATE.to_string(), "-f", "f32le", "-"]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().context("starting ffmpeg")?;
    let mut raw = Vec::new();
    child
        .stdout
        .take()
        .expect("stdout is piped")
        .read_to_end(&mut raw)
        .context("reading decoded audio from ffmpeg")?;
    let mut err = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut err);
    }
    let status = child.wait()?;
    if !status.success() {
        bail!("ffmpeg could not decode {}: {}", path.display(), err.trim());
    }

    let samples: Vec<f32> = raw
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    if samples.is_empty() {
        bail!("{} contains no audio", path.display());
    }
    Ok(Audio { samples, offset })
}

/// Transcodes media into an Ogg/Opus file the editor's web view can always play
/// (WebKitGTK may lack codecs for e.g. AAC or WMA).
pub fn make_playback_copy(src: &Path, dst: &Path) -> anyhow::Result<PathBuf> {
    check_ffmpeg()?;
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let out = Command::new(tool("ffmpeg"))
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(src)
        .args(["-vn", "-ac", "1", "-c:a", "libopus", "-b:a", "48k"])
        .arg(dst)
        .output()
        .context("running ffmpeg")?;
    if !out.status.success() {
        bail!("ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(dst.to_path_buf())
}

/// Encodes 16 kHz mono samples as a 16-bit PCM WAV file in memory.
pub fn encode_wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Finds a quiet place to cut near `target` (sample index), searching ±`window` samples.
/// Used to split long audio into chunks without slicing through words.
pub fn quietest_cut(samples: &[f32], target: usize, window: usize) -> usize {
    const FRAME: usize = SAMPLE_RATE as usize / 50; // 20 ms
    let lo = target.saturating_sub(window);
    let hi = (target + window).min(samples.len().saturating_sub(FRAME));
    if lo >= hi {
        return target.min(samples.len());
    }
    (lo..hi)
        .step_by(FRAME)
        .min_by(|&a, &b| {
            let ea: f32 = samples[a..a + FRAME].iter().map(|s| s * s).sum();
            let eb: f32 = samples[b..b + FRAME].iter().map(|s| s * s).sum();
            ea.total_cmp(&eb)
        })
        .unwrap_or(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_consistent() {
        let wav = encode_wav(&[0.0; 100]);
        assert_eq!(wav.len(), 44 + 200);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 200);
    }

    #[test]
    fn cut_lands_in_silence() {
        let mut s = vec![0.5f32; 16_000 * 4];
        for v in &mut s[30_000..31_000] {
            *v = 0.0;
        }
        let cut = quietest_cut(&s, 32_000, 8_000);
        assert!((30_000..31_000).contains(&cut), "cut at {cut}");
    }
}
