//! Merges recognised words with diarization turns into speaker-attributed segments.

use crate::asr::RawWord;
use crate::diarize::Turn;
use crate::transcript::{Segment, Speaker, Word};

/// Start a new paragraph after a pause this long, even if the speaker stays the same.
const PARAGRAPH_PAUSE: f64 = 8.0;
/// How far (seconds) a word may lie outside any turn and still be attributed to the nearest one.
const SNAP_DISTANCE: f64 = 2.0;

fn overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// Share of a word another speaker must cover to count as overlapping speech. Turn edges from
/// diarization overlap by a few hundred milliseconds, which must not be marked.
const OVERLAP_SHARE: f64 = 0.5;

/// Picks the speaker for a word from the turns it overlaps (most overlap wins) and reports
/// whether another speaker covers a substantial part of it.
///
/// Words falling into gaps between turns are common because Whisper's timestamps are fuzzy.
/// For those, `sentence_start` decides: a new sentence goes to the speaker of the next turn
/// (speaker changes mostly happen at sentence boundaries), anything else stays with the
/// current speaker (`None`).
fn speaker_for(word: &RawWord, turns: &[Turn], sentence_start: bool) -> (Option<i32>, bool) {
    // Turns are sorted by start; only nearby turns can matter.
    let first = turns.partition_point(|t| t.end < word.start - SNAP_DISTANCE);
    let nearby: Vec<&Turn> = turns[first..]
        .iter()
        .take_while(|t| t.start <= word.end + SNAP_DISTANCE)
        .collect();

    let mut per_speaker: Vec<(i32, f64)> = Vec::new();
    for t in &nearby {
        let ov = overlap(word.start, word.end, t.start, t.end);
        if ov > 0.0 {
            match per_speaker.iter_mut().find(|(s, _)| *s == t.speaker) {
                Some(e) => e.1 += ov,
                None => per_speaker.push((t.speaker, ov)),
            }
        }
    }
    if let Some(&(best, _)) = per_speaker.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
        let dur = (word.end - word.start).max(0.01);
        let overlapping = per_speaker
            .iter()
            .any(|&(s, ov)| s != best && ov / dur >= OVERLAP_SHARE);
        return (Some(best), overlapping);
    }

    if sentence_start {
        if let Some(next) = nearby.iter().find(|t| t.start >= word.start) {
            return (Some(next.speaker), false);
        }
    }
    (None, false)
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

/// Builds speaker turns from words. Speakers are numbered in order of first appearance.
pub fn build_segments(words: &[RawWord], turns: &[Turn]) -> (Vec<Speaker>, Vec<Segment>) {
    let mut speaker_ids: Vec<i32> = Vec::new();
    let mut segments: Vec<Segment> = Vec::new();
    let mut last_speaker: Option<String> = None;
    let mut last_raw: Option<i32> = None;

    for (i, w) in words.iter().enumerate() {
        let sentence_start = i == 0 || ends_sentence(&words[i - 1].text);
        let (mut raw, overlap) = if turns.is_empty() {
            (None, false)
        } else {
            speaker_for(w, turns, sentence_start)
        };
        if raw.is_none() && last_raw.is_none() && !turns.is_empty() {
            raw = nearest_speaker(w, turns);
        }
        if raw.is_some() {
            last_raw = raw;
        }
        // Words in gaps between turns stay with the current speaker.
        let speaker = match raw {
            Some(r) => {
                let idx = match speaker_ids.iter().position(|&s| s == r) {
                    Some(i) => i,
                    None => {
                        speaker_ids.push(r);
                        speaker_ids.len() - 1
                    }
                };
                Some(format!("S{}", idx + 1))
            }
            None => last_speaker.clone(),
        };
        let word = Word {
            text: w.text.clone(),
            start: w.start,
            end: w.end,
            conf: w.conf,
            overlap,
        };
        let continue_segment = segments.last().is_some_and(|s| {
            s.speaker == speaker && w.start - s.end < PARAGRAPH_PAUSE
        });
        if continue_segment {
            let seg = segments.last_mut().unwrap();
            seg.end = seg.end.max(w.end);
            seg.words.push(word);
        } else {
            segments.push(Segment {
                speaker: speaker.clone(),
                start: w.start,
                end: w.end,
                words: vec![word],
            });
        }
        last_speaker = speaker;
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
    fn long_pause_splits_paragraph() {
        let words = [w("Eins.", 0.0, 0.5), w("Zwei.", 20.0, 20.5)];
        let (speakers, segs) = build_segments(&words, &[]);
        assert!(speakers.is_empty());
        assert_eq!(segs.len(), 2);
    }
}
