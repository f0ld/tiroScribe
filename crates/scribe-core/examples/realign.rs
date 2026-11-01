//! Alignment-only experiment: `cargo run --example realign -- <transcript.scribe.json> <turns.txt>`
//! where turns.txt is the output of the `diar` example. Prints the re-aligned paragraphs.
use scribe_core::{align, asr::RawWord, diarize::Turn, Transcript};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let t = Transcript::load(a[1].as_ref())?;
    let words: Vec<RawWord> = t
        .segments
        .iter()
        .flat_map(|s| &s.words)
        .map(|w| RawWord { text: w.text.clone(), start: w.start, end: w.end, conf: w.conf })
        .collect();
    let turns: Vec<Turn> = std::fs::read_to_string(&a[2])?
        .lines()
        .filter_map(|l| {
            let f: Vec<f64> = l.split_whitespace().map(|x| x.parse()).collect::<Result<_, _>>().ok()?;
            (f.len() == 3).then(|| Turn { start: f[0], end: f[1], speaker: f[2] as i32 })
        })
        .collect();
    let (_, segs) = align::build_segments(&words, &turns);
    for s in segs {
        println!("{:7.2}-{:7.2} {}: {}", s.start, s.end, s.speaker.as_deref().unwrap_or("-"), s.text());
    }
    Ok(())
}
