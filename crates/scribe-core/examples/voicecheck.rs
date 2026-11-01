//! Voice-check experiment: `voicecheck <media> <embedding.onnx> <transcript.scribe.json>`.
//! Prints how far each sentence's voice is from its own speaker versus the other one.
use scribe_core::{align, asr::RawWord, audio, diarize::VoiceEmbedder, Transcript};
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let audio = audio::decode(a[1].as_ref(), Default::default())?;
    let emb = VoiceEmbedder::new(a[2].as_ref(), 8)?;
    let t = Transcript::load(a[3].as_ref())?;
    let (mut words, mut labels) = (Vec::new(), Vec::new());
    for s in &t.segments {
        let label: i32 = s.speaker.as_deref().unwrap_or("S0")[1..].parse()?;
        for w in &s.words {
            words.push(RawWord { text: w.text.clone(), start: w.start, end: w.end, conf: w.conf });
            labels.push(label);
        }
    }
    let t0 = std::time::Instant::now();
    let scores = align::voice_scores(&words, &labels, |s, e| {
        emb.embed(&audio.samples[(s * 16000.0) as usize..((e * 16000.0) as usize).min(audio.samples.len())])
    });
    eprintln!("{} units in {:.1}s", scores.len(), t0.elapsed().as_secs_f64());
    for u in &scores {
        let text: Vec<&str> = words[u.first..=u.last].iter().map(|w| w.text.as_str()).collect();
        println!("{:+.3} own={:.3} other={:.3} S{} {:6.1}s {}", u.other_sim - u.own, u.own, u.other_sim, u.speaker, words[u.first].start, text.join(" "));
    }
    Ok(())
}
