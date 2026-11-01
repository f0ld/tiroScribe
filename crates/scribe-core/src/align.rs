//! Merges recognised words with diarization turns into speaker-attributed segments.

use crate::asr::RawWord;
use crate::diarize::Turn;
use crate::transcript::{Segment, Speaker, Word};

/// Start a new paragraph after a pause this long, even if the speaker stays the same.
const PARAGRAPH_PAUSE: f64 = 8.0;
/// A same-speaker paragraph may end at a sentence followed by this pause, if it has at least
/// `PARAGRAPH_MIN_WORDS`. Thought units in long answers are usually separated like this.
const SENTENCE_PAUSE: f64 = 1.2;
const PARAGRAPH_MIN_WORDS: usize = 30;
/// Longest paragraph before it is broken at the next sentence end, whatever the pause.
const PARAGRAPH_MAX_WORDS: usize = 150;
/// How far (seconds) a word may lie outside any turn and still be attributed to the nearest one.
const SNAP_DISTANCE: f64 = 2.0;

/// Share of a word another speaker must cover to count as overlapping speech. Turn edges from
/// diarization overlap by a few hundred milliseconds, which must not be marked.
const OVERLAP_SHARE: f64 = 0.5;
/// Share of a word the current speaker must still cover to keep it while someone else talks
/// over them. Diarization finds short back-channels ("mhm", "ja") inside long answers that
/// Whisper usually does not transcribe; the words spoken meanwhile belong to the main speaker.
const KEEP_SHARE: f64 = 0.3;
/// A speaker change inside a sentence needs at least this much speech on both sides;
/// shorter runs are timing noise (Whisper's word times are often ~0.5 s off).
const MIN_RUN_SECS: f64 = 1.0;
const MIN_RUN_WORDS: usize = 3;
/// Fingerprints of shorter stretches are unreliable.
const VOICE_MIN_SECS: f64 = 1.5;
/// A speaker needs this many sentences for a usable voice profile.
const VOICE_MIN_UNITS: usize = 3;
/// How much closer a sentence's voice must be to another speaker's profile to be moved there.
/// Sentences of a clearly separated test interview favoured their own speaker by at least 0.54;
/// this stays well clear of that so that similar voices are left alone rather than swapped.
const VOICE_MARGIN: f32 = 0.2;

fn overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// Seconds of the word covered by each speaker's turns. Very short (or zero-length) word
/// timings are widened a little so they still hit the turn they lie in.
///
/// `reach[i]` is the latest end of `turns[..=i]`. Turns are sorted by start only – a long
/// answer can outlast many short turns that start after it – so searching by end needs it.
fn coverage(word: &RawWord, turns: &[Turn], reach: &[f64]) -> (Vec<(i32, f64)>, f64) {
    let mid = (word.start + word.end) / 2.0;
    let (w0, w1) = if word.end - word.start < 0.1 {
        (mid - 0.05, mid + 0.05)
    } else {
        (word.start, word.end)
    };
    let first = reach.partition_point(|&end| end <= w0);
    let mut per_speaker: Vec<(i32, f64)> = Vec::new();
    for t in turns[first..].iter().take_while(|t| t.start <= w1) {
        let ov = overlap(w0, w1, t.start, t.end);
        if ov > 0.0 {
            match per_speaker.iter_mut().find(|(s, _)| *s == t.speaker) {
                Some(e) => e.1 += ov,
                None => per_speaker.push((t.speaker, ov)),
            }
        }
    }
    (per_speaker, w1 - w0)
}

/// Picks the speaker for a word from the turns it overlaps.
///
/// Mid-sentence, the current speaker keeps the word as long as they still cover a fair part
/// of it; otherwise the speaker with the most overlap wins. Words falling into gaps between
/// turns are common because Whisper's timestamps are fuzzy. For those, `sentence_start`
/// decides: a new sentence goes to the speaker of the next turn (speaker changes mostly
/// happen at sentence boundaries), anything else stays with the current speaker (`None`).
fn speaker_for(
    word: &RawWord,
    turns: &[Turn],
    cover: &[(i32, f64)],
    dur: f64,
    sentence_start: bool,
    current: Option<i32>,
) -> Option<i32> {
    if let Some(&(best, _)) = cover.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
        let keep = !sentence_start
            && current.is_some_and(|c| cover.iter().any(|&(s, ov)| s == c && ov / dur >= KEEP_SHARE));
        return if keep { current } else { Some(best) };
    }
    if sentence_start {
        let first = turns.partition_point(|t| t.start < word.start);
        if let Some(next) = turns[first..].iter().find(|t| t.start <= word.end + SNAP_DISTANCE) {
            return Some(next.speaker);
        }
    }
    None
}

/// Nearest turn's speaker, used only when there is no current speaker yet.
fn nearest_speaker(word: &RawWord, turns: &[Turn]) -> Option<i32> {
    turns
        .iter()
        .map(|t| {
            let dist = (t.start - word.end).max(word.start - t.end).max(0.0);
            (t.speaker, dist)
        })
        .filter(|&(_, d)| d <= SNAP_DISTANCE)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, _)| s)
}

fn ends_sentence(word: &str) -> bool {
    word.ends_with(['.', '?', '!', '…'])
}

/// Removes speaker changes inside a sentence that are too short to be real: each short run
/// of labels is absorbed by its longer neighbour, shortest first.
fn smooth_sentence(words: &[RawWord], labels: &mut [Option<i32>]) {
    loop {
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for i in 0..labels.len() {
            match runs.last_mut() {
                Some(r) if labels[r.0] == labels[i] => r.1 = i + 1,
                _ => runs.push((i, i + 1)),
            }
        }
        if runs.len() < 2 {
            return;
        }
        let secs = |r: (usize, usize)| words[r.1 - 1].end - words[r.0].start;
        let weak = runs
            .iter()
            .enumerate()
            .filter(|&(_, &r)| r.1 - r.0 < MIN_RUN_WORDS || secs(r) < MIN_RUN_SECS)
            .min_by(|a, b| secs(*a.1).total_cmp(&secs(*b.1)));
        let Some((k, &run)) = weak else {
            return;
        };
        let prev = k.checked_sub(1).map(|j| runs[j]);
        let next = runs.get(k + 1).copied();
        let target = match (prev, next) {
            (Some(p), Some(n)) => {
                if secs(p) >= secs(n) {
                    p
                } else {
                    n
                }
            }
            (Some(p), None) => p,
            (None, Some(n)) => n,
            (None, None) => return,
        };
        let label = labels[target.0];
        labels[run.0..run.1].fill(label);
    }
}

/// A speaker-uniform stretch of a sentence, scored against the voices of all speakers.
#[derive(Debug, Clone)]
pub struct VoiceScore {
    /// Word range.
    pub first: usize,
    pub last: usize,
    pub speaker: i32,
    /// Similarity to the unit's own speaker (excluding the unit itself).
    pub own: f32,
    /// Most similar other speaker.
    pub other: i32,
    pub other_sim: f32,
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn normalize(v: &mut [f32]) {
    let n = dot(v, v).sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

/// Scores every sufficiently long sentence against a voice profile of each speaker, built
/// from that speaker's other sentences. `embed` returns a fingerprint for a time range.
pub fn voice_scores(
    words: &[RawWord],
    labels: &[i32],
    mut embed: impl FnMut(f64, f64) -> Option<Vec<f32>>,
) -> Vec<VoiceScore> {
    let mut units: Vec<(usize, usize, i32, Vec<f32>)> = Vec::new();
    let mut first = 0;
    for i in 0..words.len() {
        let boundary = i + 1 == words.len()
            || ends_sentence(&words[i].text)
            || labels[i + 1] != labels[i];
        if !boundary {
            continue;
        }
        if words[i].end - words[first].start >= VOICE_MIN_SECS {
            if let Some(mut e) = embed(words[first].start, words[i].end) {
                normalize(&mut e);
                units.push((first, i, labels[i], e));
            }
        }
        first = i + 1;
    }
    let mut speakers: Vec<i32> = units.iter().map(|u| u.2).collect();
    speakers.sort_unstable();
    speakers.dedup();
    let enough = |s: i32| units.iter().filter(|u| u.2 == s).count() >= VOICE_MIN_UNITS;
    if speakers.len() < 2 || !speakers.iter().all(|&s| enough(s)) {
        return Vec::new();
    }
    // Sum per speaker; subtracting a unit from its own sum leaves out its self-similarity.
    let dim = units[0].3.len();
    let mut sums: Vec<Vec<f32>> = vec![vec![0.0; dim]; speakers.len()];
    for u in &units {
        let k = speakers.binary_search(&u.2).unwrap();
        sums[k].iter_mut().zip(&u.3).for_each(|(a, b)| *a += b);
    }
    units
        .iter()
        .map(|u| {
            let mut own_profile: Vec<f32> = Vec::new();
            let mut other = (u.2, f32::NEG_INFINITY);
            for (k, &s) in speakers.iter().enumerate() {
                if s == u.2 {
                    own_profile = sums[k].iter().zip(&u.3).map(|(a, b)| a - b).collect();
                } else {
                    let mut profile = sums[k].clone();
                    normalize(&mut profile);
                    let sim = dot(&profile, &u.3);
                    if sim > other.1 {
                        other = (s, sim);
                    }
                }
            }
            normalize(&mut own_profile);
            VoiceScore {
                first: u.0,
                last: u.1,
                speaker: u.2,
                own: dot(&own_profile, &u.3),
                other: other.0,
                other_sim: other.1,
            }
        })
        .collect()
}

/// Builds speaker turns from words. Speakers are numbered in order of first appearance.
pub fn build_segments(words: &[RawWord], turns: &[Turn]) -> (Vec<Speaker>, Vec<Segment>) {
    build_segments_with_voice(words, turns, |_, _| None)
}

/// Like [`build_segments`], and also moves sentences whose voice (from `embed`, which returns
/// a fingerprint for a time range) clearly belongs to another speaker.
pub fn build_segments_with_voice(
    words: &[RawWord],
    turns: &[Turn],
    embed: impl FnMut(f64, f64) -> Option<Vec<f32>>,
) -> (Vec<Speaker>, Vec<Segment>) {
    // 1. A speaker per word from the diarization turns.
    let reach: Vec<f64> = turns
        .iter()
        .scan(f64::NEG_INFINITY, |r, t| {
            *r = r.max(t.end);
            Some(*r)
        })
        .collect();
    let covers: Vec<(Vec<(i32, f64)>, f64)> =
        words.iter().map(|w| coverage(w, turns, &reach)).collect();
    let mut labels: Vec<Option<i32>> = Vec::with_capacity(words.len());
    let mut current: Option<i32> = None;
    for (i, w) in words.iter().enumerate() {
        let sentence_start = i == 0 || ends_sentence(&words[i - 1].text);
        let (cover, dur) = &covers[i];
        let mut label = speaker_for(w, turns, cover, *dur, sentence_start, current);
        if label.is_none() && current.is_none() {
            label = nearest_speaker(w, turns);
        }
        // Words in gaps between turns stay with the current speaker.
        current = label.or(current);
        labels.push(current);
    }
    // Words before the first attributable one go to its speaker.
    if let Some(first) = labels.iter().flatten().next().copied() {
        for l in labels.iter_mut().take_while(|l| l.is_none()) {
            *l = Some(first);
        }
    }

    // 2. Smooth out timing noise within sentences.
    let mut start = 0;
    for i in 0..words.len() {
        if ends_sentence(&words[i].text) || i + 1 == words.len() {
            smooth_sentence(&words[start..=i], &mut labels[start..=i]);
            start = i + 1;
        }
    }

    // 3. Correct whole sentences by their voice.
    if let Some(known) = labels.iter().copied().collect::<Option<Vec<i32>>>() {
        for u in voice_scores(words, &known, embed) {
            if u.other_sim - u.own >= VOICE_MARGIN {
                log::debug!("voice check: {:.2}s moves to speaker {}", words[u.first].start, u.other);
                labels[u.first..=u.last].fill(Some(u.other));
            }
        }
    }

    // 4. Group into paragraphs.
    let mut speaker_ids: Vec<i32> = Vec::new();
    let mut segments: Vec<Segment> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let speaker = labels[i].map(|r| {
            let idx = speaker_ids.iter().position(|&s| s == r).unwrap_or_else(|| {
                speaker_ids.push(r);
                speaker_ids.len() - 1
            });
            format!("S{}", idx + 1)
        });
        let (cover, dur) = &covers[i];
        let overlap = cover
            .iter()
            .any(|&(s, ov)| Some(s) != labels[i] && ov / dur >= OVERLAP_SHARE);
        let word = Word {
            text: w.text.clone(),
            start: w.start,
            end: w.end,
            conf: w.conf,
            overlap,
        };
        let continue_segment = segments.last().is_some_and(|s| {
            let pause = w.start - s.end;
            let after_sentence = s.words.last().is_some_and(|l| ends_sentence(&l.text));
            let break_here = after_sentence
                && (s.words.len() >= PARAGRAPH_MAX_WORDS
                    || (pause >= SENTENCE_PAUSE && s.words.len() >= PARAGRAPH_MIN_WORDS));
            s.speaker == speaker && pause < PARAGRAPH_PAUSE && !break_here
        });
        if continue_segment {
            let seg = segments.last_mut().unwrap();
            seg.end = seg.end.max(w.end);
            seg.words.push(word);
        } else {
            segments.push(Segment {
                speaker,
                start: w.start,
                end: w.end,
                words: vec![word],
            });
        }
    }

    let speakers = (1..=speaker_ids.len())
        .map(|i| Speaker {
            id: format!("S{i}"),
            name: format!("S{i}"),
        })
        .collect();
    (speakers, segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str, start: f64, end: f64) -> RawWord {
        RawWord {
            text: text.into(),
            start,
            end,
            conf: 0.9,
        }
    }

    fn t(start: f64, end: f64, speaker: i32) -> Turn {
        Turn {
            start,
            end,
            speaker,
        }
    }

    #[test]
    fn assigns_by_overlap_and_numbers_by_appearance() {
        let words = [w("Wie", 0.0, 0.3), w("geht's?", 0.3, 0.8), w("Gut.", 1.2, 1.6)];
        let turns = [t(0.0, 0.9, 3), t(1.1, 2.0, 0)];
        let (speakers, segs) = build_segments(&words, &turns);
        assert_eq!(speakers.len(), 2);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].speaker.as_deref(), Some("S1"));
        assert_eq!(segs[0].text(), "Wie geht's?");
        assert_eq!(segs[1].speaker.as_deref(), Some("S2"));
    }

    #[test]
    fn marks_overlap() {
        let words = [w("Ja", 1.0, 1.5)];
        let turns = [t(0.0, 1.4, 0), t(1.1, 3.0, 1)];
        let (_, segs) = build_segments(&words, &turns);
        assert!(segs[0].words[0].overlap);
    }

    #[test]
    fn gap_words_follow_sentence_boundaries() {
        // "Ja," lies in the gap after a finished sentence → next speaker.
        // "genau" lies in a gap mid-sentence → stays with the current speaker.
        let words = [
            w("vorstellen.", 0.0, 1.0),
            w("Ja,", 1.1, 1.2),
            w("also", 1.5, 1.8),
            w("genau", 3.1, 3.2),
        ];
        let turns = [t(0.0, 1.05, 0), t(1.4, 3.0, 1), t(3.5, 5.0, 0)];
        let (_, segs) = build_segments(&words, &turns);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1].text(), "Ja, also genau");
    }

    #[test]
    fn backchannel_does_not_steal_words() {
        // S0 answers at length; S1's short "mhm" (not transcribed) overlaps "Sie denn".
        let words = [
            w("Wie", 41.9, 42.0),
            w("haben", 42.0, 42.3),
            w("Sie", 42.4, 42.6),
            w("denn", 42.6, 42.9),
            w("davon", 42.9, 43.2),
            w("erfahren?", 43.2, 43.8),
        ];
        let turns = [t(37.0, 46.5, 0), t(42.2, 43.1, 1)];
        let (speakers, segs) = build_segments(&words, &turns);
        assert_eq!(speakers.len(), 1);
        assert_eq!(segs.len(), 1);
        assert!(segs[0].words[2].overlap);
    }

    #[test]
    fn finds_long_turn_after_short_ones() {
        // The long turn starts first but outlasts the short one; lookups must still find it.
        let words = [w("Gut.", 379.0, 379.5), w("Danke", 380.9, 381.1), w("dir.", 381.2, 381.8)];
        let turns = [t(377.4, 382.0, 0), t(378.5, 378.9, 1), t(381.1, 381.4, 1)];
        let (_, segs) = build_segments(&words, &turns);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text(), "Gut. Danke dir.");
    }

    #[test]
    fn early_timestamps_at_answer_start_are_smoothed() {
        // Whisper places "Einen ganz" before the answer turn begins.
        let words = [
            w("Physikstudium?", 133.3, 134.3),
            w("Einen", 134.3, 134.6),
            w("ganz", 134.6, 134.9),
            w("grossen", 134.9, 135.3),
            w("Teil", 135.3, 135.5),
            w("meiner", 135.5, 135.9),
            w("Forschung.", 135.9, 136.4),
        ];
        let turns = [t(130.0, 134.5, 0), t(135.0, 140.0, 1)];
        let (_, segs) = build_segments(&words, &turns);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1].text(), "Einen ganz grossen Teil meiner Forschung.");
    }

    #[test]
    fn keeps_long_speaker_change_inside_sentence() {
        // Missing punctuation must not swallow a real change of speaker.
        let words = [
            w("und", 0.0, 0.5),
            w("dann", 0.5, 1.0),
            w("war", 1.0, 1.5),
            w("das", 1.5, 2.0),
            w("ja", 2.2, 2.5),
            w("genau", 2.5, 3.0),
            w("so", 3.0, 3.5),
            w("war", 3.5, 4.0),
            w("das.", 4.0, 4.5),
        ];
        let turns = [t(0.0, 2.1, 0), t(2.1, 5.0, 1)];
        let (_, segs) = build_segments(&words, &turns);
        assert_eq!(segs.len(), 2);
    }

    #[test]
    fn long_answers_break_at_sentences_with_a_pause() {
        let mut words: Vec<RawWord> = (0..40).map(|i| w("wort", i as f64 * 0.3, i as f64 * 0.3 + 0.3)).collect();
        words.push(w("Ende.", 12.0, 12.3));
        words.push(w("Weiter", 14.0, 14.3)); // pause after a sentence, long enough paragraph
        words.push(w("und.", 14.3, 14.6));
        words.push(w("Noch", 16.0, 16.3)); // same pause, but the paragraph is still short
        let (_, segs) = build_segments(&words, &[]);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1].text(), "Weiter und. Noch");
    }

    #[test]
    fn very_long_paragraphs_break_at_the_next_sentence() {
        let mut words: Vec<RawWord> = (0..200)
            .map(|i| w(if i % 40 == 39 { "satz." } else { "wort" }, i as f64 * 0.3, i as f64 * 0.3 + 0.3))
            .collect();
        words.push(w("Ende.", 60.0, 60.3));
        let (_, segs) = build_segments(&words, &[]);
        assert_eq!(segs[0].words.len(), 160);
    }

    /// Six sentences, three per speaker, with fake voices: the one at 10 s sounds like
    /// `voice_of_10`, the others like the speaker their turn says.
    fn voice_case(voice_of_10: [f32; 2]) -> Vec<Segment> {
        let words: Vec<RawWord> = (0..6)
            .flat_map(|i| {
                let s = i as f64 * 5.0;
                [w("eins", s, s + 1.0), w("zwei.", s + 1.0, s + 2.0)]
            })
            .collect();
        let turns = [t(0.0, 15.0, 0), t(15.0, 30.0, 1)];
        build_segments_with_voice(&words, &turns, |s, _| {
            Some(match s as i32 {
                10 => voice_of_10.to_vec(),
                0 | 5 => vec![1.0, 0.0],
                _ => vec![0.0, 1.0],
            })
        })
        .1
    }

    #[test]
    fn voice_check_moves_clearly_wrong_sentences() {
        let segs = voice_case([0.0, 1.0]); // sentence at 10 s sounds like speaker 1
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1].start, 10.0);
    }

    #[test]
    fn voice_check_leaves_undecided_sentences_alone() {
        let segs = voice_case([1.0, 1.0]); // in between
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1].start, 15.0);
    }

    #[test]
    fn long_pause_splits_paragraph() {
        let words = [w("Eins.", 0.0, 0.5), w("Zwei.", 20.0, 20.5)];
        let (speakers, segs) = build_segments(&words, &[]);
        assert!(speakers.is_empty());
        assert_eq!(segs.len(), 2);
    }
}
