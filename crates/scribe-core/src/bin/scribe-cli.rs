//! Headless front end: download models, transcribe and export without the GUI.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use scribe_core::asr::openai_api::ServerConfig;
use scribe_core::asr::{whisper, Device, Quality};
use scribe_core::audio::TimeRange;
use scribe_core::diarize::Speakers;
use scribe_core::export::{self, ExportOptions, Format};
use scribe_core::transcript::default_transcript_path;
use scribe_core::{models, Engines, JobCtx, JobSpec, Transcript};

#[derive(Parser)]
#[command(name = "scribe-cli", about = "tiroScribe – local interview transcription")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List available models and devices.
    Info,
    /// Download a model (and the required helper models).
    Download { id: String },
    /// Transcribe a media file to a .scribe.json transcript.
    Transcribe(TranscribeArgs),
    /// Export a transcript to another format.
    Export {
        transcript: PathBuf,
        #[arg(short, long, value_enum, default_value = "html")]
        format: ExportFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(clap::Args)]
struct TranscribeArgs {
    media: PathBuf,
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Language code (de, en, …) or "auto".
    #[arg(short, long, default_value = "de")]
    language: String,
    /// Model id from `info`, or a path to a ggml file.
    #[arg(short, long, default_value = "large-v3-turbo")]
    model: String,
    /// cpu, gpu, gpu:<index> or npu.
    #[arg(short, long, default_value = "gpu")]
    device: String,
    /// Number of speakers; 0 = detect automatically, -1 = no speaker detection.
    #[arg(short, long, default_value_t = 0, allow_negative_numbers = true)]
    speakers: i32,
    #[arg(long)]
    fast: bool,
    /// Keep fillers like "äh", "ähm".
    #[arg(long)]
    verbatim: bool,
    /// Comma separated names/terms to spell correctly.
    #[arg(long, value_delimiter = ',')]
    vocabulary: Vec<String>,
    #[arg(long)]
    start: Option<f64>,
    #[arg(long)]
    end: Option<f64>,
    #[arg(long)]
    no_vad: bool,
    #[arg(long, default_value = "http://localhost:8000/api/v1")]
    npu_url: String,
    #[arg(long, default_value = "whisper-large-v3-turbo")]
    npu_model: String,
}

#[derive(Clone, Copy, ValueEnum)]
enum ExportFormat {
    Html,
    Txt,
    Vtt,
    Srt,
}

impl From<ExportFormat> for Format {
    fn from(f: ExportFormat) -> Self {
        match f {
            ExportFormat::Html => Format::Html,
            ExportFormat::Txt => Format::Txt,
            ExportFormat::Vtt => Format::Vtt,
            ExportFormat::Srt => Format::Srt,
        }
    }
}

fn parse_device(s: &str) -> anyhow::Result<Device> {
    Ok(match s {
        "cpu" => Device::Cpu,
        "npu" => Device::Npu,
        "gpu" => Device::Gpu { index: 0 },
        _ => match s.strip_prefix("gpu:") {
            Some(i) => Device::Gpu {
                index: i.parse().context("invalid GPU index")?,
            },
            None => anyhow::bail!("unknown device {s}"),
        },
    })
}

fn download(id: &str) -> anyhow::Result<()> {
    let wanted = models::find(id).with_context(|| format!("unknown model {id}"))?;
    let essentials = models::CATALOG.iter().filter(|m| m.essential);
    for m in std::iter::once(wanted).chain(essentials) {
        if models::is_installed(m) {
            continue;
        }
        eprintln!("Downloading {} …", m.name);
        models::download(m, &JobCtx::detached(), |done, total| {
            eprint!("\r  {:5.1} %", done as f64 / total as f64 * 100.0);
            let _ = std::io::stderr().flush();
        })?;
        eprintln!();
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    whisper::install_logging();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info => {
            println!("Models ({}):", models::models_dir().display());
            for m in models::CATALOG {
                let mark = if models::is_installed(m) { "✔" } else { " " };
                println!("  [{mark}] {:28} {}", m.id, m.description);
            }
            println!("GPUs:");
            for g in whisper::list_gpus() {
                println!("  gpu:{}  {} ({} MB)", g.index, g.name, g.memory_mb);
            }
        }
        Cmd::Download { id } => download(&id)?,
        Cmd::Transcribe(a) => {
            let output = a
                .output
                .unwrap_or_else(|| default_transcript_path(&a.media));
            let spec = JobSpec {
                media: a.media,
                output: output.clone(),
                range: TimeRange {
                    start: a.start,
                    end: a.end,
                },
                language: Some(a.language),
                model: a.model,
                device: parse_device(&a.device)?,
                quality: if a.fast { Quality::Fast } else { Quality::Precise },
                verbatim: a.verbatim,
                vocabulary: a.vocabulary,
                speakers: match a.speakers {
                    n if n < 0 => Speakers::Off,
                    0 => Speakers::default(),
                    n => Speakers::Fixed { count: n as u32 },
                },
                vad: !a.no_vad,
                npu: ServerConfig {
                    base_url: a.npu_url,
                    model: a.npu_model,
                },
            };
            let started = std::time::Instant::now();
            let ctx = JobCtx::new(
                Default::default(),
                Arc::new(|p| {
                    eprint!("\r{:>16?} {:5.1} %   ", p.stage, p.overall * 100.0);
                    let _ = std::io::stderr().flush();
                }),
            );
            let t = scribe_core::pipeline::run(&spec, &mut Engines::default(), &ctx)?;
            eprintln!(
                "\nDone in {:.0} s: {} segments, {} speakers, language {} → {}",
                started.elapsed().as_secs_f64(),
                t.segments.len(),
                t.speakers.len(),
                t.language,
                output.display()
            );
        }
        Cmd::Export {
            transcript,
            format,
            output,
        } => {
            let t = Transcript::load(&transcript)?;
            let format: Format = format.into();
            let text = export::render(&t, format, &ExportOptions::default());
            match output {
                Some(p) => std::fs::write(p, text)?,
                None => print!("{text}"),
            }
        }
    }
    Ok(())
}
