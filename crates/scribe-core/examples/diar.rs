//! Diarization-only experiment: `cargo run --release --example diar -- <media> <seg.onnx> <emb.onnx> <auto:THRESH|N>`
use scribe_core::{audio, diarize};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let audio = audio::decode(a[1].as_ref(), Default::default())?;
    let speakers = match a[4].strip_prefix("auto:") {
        Some(t) => diarize::Speakers::Auto { threshold: t.parse()? },
        None => diarize::Speakers::Fixed { count: a[4].parse()? },
    };
    let t0 = std::time::Instant::now();
    let turns = diarize::diarize(&audio.samples, a[2].as_ref(), a[3].as_ref(), speakers, 8)?;
    eprintln!("{} turns in {:.1}s", turns.len(), t0.elapsed().as_secs_f64());
    for t in turns {
        println!("{:7.2} {:7.2} {}", t.start, t.end, t.speaker);
    }
    Ok(())
}
