//! Renders transcripts to the formats researchers and journalists hand on: HTML (opens in
//! Word/LibreOffice), plain text, and WebVTT / SRT subtitles (e.g. for EXMARaLDA, video editors).

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::transcript::{join_words, Segment, Transcript};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Html,
    Txt,
    Vtt,
    Srt,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Html => "html",
            Format::Txt => "txt",
            Format::Vtt => "vtt",
            Format::Srt => "srt",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TimestampStyle {
    None,
    /// `[00:01:38]` at the start of each paragraph (noScribe style).
    #[default]
    Bracket,
    /// `#00:01:38-0#` at the end of each paragraph (f4/Dresing & Pehl, readable by f4analyse/MAXQDA).
    F4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    pub timestamps: TimestampStyle,
    /// Mark pauses at least this long (seconds). `None` disables pause marks.
    pub pause_min: Option<f64>,
    /// Mark overlapping speech with `//…//`.
    pub mark_overlap: bool,
    /// Include a header with file name, duration and engine.
    pub header: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            timestamps: TimestampStyle::Bracket,
            pause_min: Some(1.0),
            mark_overlap: true,
            header: true,
        }
    }
}

pub fn render(t: &Transcript, format: Format, opts: &ExportOptions) -> String {
    match format {
        Format::Html => html(t, opts),
        Format::Txt => txt(t, opts),
        Format::Vtt => subtitles(t, true),
        Format::Srt => subtitles(t, false),
    }
}

/// `hh:mm:ss`
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// `hh:mm:ss.mmm` (VTT) or `hh:mm:ss,mmm` (SRT)
fn subtitle_clock(secs: f64, sep: char) -> String {
    let ms = (secs.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}{sep}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Dresing & Pehl pause notation: `(.)` ≈1 s, `(..)` ≈2 s, `(...)` ≈3 s, `(n)` beyond.
pub fn pause_mark(secs: f64) -> String {
    match secs.round() as u64 {
        0 | 1 => "(.)".into(),
        2 => "(..)".into(),
        3 => "(...)".into(),
        n => format!("({n})"),
    }
}

/// The paragraph's text with pause and overlap markers applied.
fn segment_body(seg: &Segment, opts: &ExportOptions) -> String {
    let mut tokens: Vec<String> = Vec::with_capacity(seg.words.len());
    let mut in_overlap = false;
    for (i, w) in seg.words.iter().enumerate() {
        if let (Some(min), Some(prev)) = (opts.pause_min, i.checked_sub(1).map(|j| &seg.words[j])) {
            let gap = w.start - prev.end;
            if gap >= min {
                tokens.push(pause_mark(gap));
            }
        }
        let mut text = w.text.clone();
        if opts.mark_overlap && w.overlap != in_overlap {
            // Opening marker attaches to this word; closing marker to the previous one.
            if w.overlap {
                text = format!("//{text}");
            } else if let Some(last) = tokens.last_mut() {
                last.push_str("//");
            }
            in_overlap = w.overlap;
        }
        tokens.push(text);
    }
    if in_overlap {
        if let Some(last) = tokens.last_mut() {
            last.push_str("//");
        }
    }
    join_words(tokens.iter().map(String::as_str))
}

fn f4_stamp(secs: f64) -> String {
    format!("#{}-{}#", clock(secs), ((secs.fract()) * 10.0) as u32)
}

fn paragraphs(t: &Transcript, opts: &ExportOptions) -> Vec<(Option<String>, String)> {
    t.segments
        .iter()
        .filter(|s| !s.words.is_empty())
        .map(|seg| {
            let mut body = segment_body(seg, opts);
            match opts.timestamps {
                TimestampStyle::None => {}
                TimestampStyle::Bracket => body = format!("[{}] {body}", clock(seg.start)),
                TimestampStyle::F4 => body = format!("{body} {}", f4_stamp(seg.end)),
            }
            let speaker = t.speaker_name(seg.speaker.as_deref()).map(str::to_string);
            (speaker, body)
        })
        .collect()
}

fn header_lines(t: &Transcript) -> Vec<String> {
    let file = std::path::Path::new(&t.media.path)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    vec![
        file,
        format!("Duration: {}", clock(t.media.duration)),
        format!("Transcribed with liveScribe – {}, model {}", t.engine.backend, t.engine.model),
    ]
}

fn txt(t: &Transcript, opts: &ExportOptions) -> String {
    let mut out = String::new();
    if opts.header {
        for line in header_lines(t) {
            out.push_str(&line);
            out.push('\n');
        }
        out.push('\n');
    }
    for (speaker, body) in paragraphs(t, opts) {
        match speaker {
            Some(s) => writeln!(out, "{s}: {body}\n").unwrap(),
            None => writeln!(out, "{body}\n").unwrap(),
        }
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn html(t: &Transcript, opts: &ExportOptions) -> String {
    let mut out = String::from(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>Transcript</title>\n<style>\n\
         body { font-family: Arial, sans-serif; font-size: 11pt; line-height: 1.5; max-width: 50em; margin: 2em auto; }\n\
         .header { color: #555; font-size: 9pt; border-bottom: 1px solid #ccc; margin-bottom: 1.5em; }\n\
         .speaker { font-weight: bold; }\n\
         p { margin: 0 0 0.8em 0; }\n\
         </style>\n</head>\n<body>\n",
    );
    if opts.header {
        out.push_str("<div class=\"header\">\n");
        for line in header_lines(t) {
            writeln!(out, "<p>{}</p>", escape(&line)).unwrap();
        }
        out.push_str("</div>\n");
    }
    for (speaker, body) in paragraphs(t, opts) {
        match speaker {
            Some(s) => writeln!(
                out,
                "<p><span class=\"speaker\">{}:</span> {}</p>",
                escape(&s),
                escape(&body)
            )
            .unwrap(),
            None => writeln!(out, "<p>{}</p>", escape(&body)).unwrap(),
        }
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// Splits segments into cues of at most ~7 s / 84 characters, as subtitle guidelines suggest.
fn subtitles(t: &Transcript, vtt: bool) -> String {
    const MAX_SECS: f64 = 7.0;
    const MAX_CHARS: usize = 84;
    let sep = if vtt { '.' } else { ',' };
    let mut out = String::new();
    if vtt {
        out.push_str("WEBVTT\n\n");
    }
    let mut n = 0;
    for seg in &t.segments {
        let speaker = t.speaker_name(seg.speaker.as_deref());
        let mut i = 0;
        while i < seg.words.len() {
            let start = seg.words[i].start;
            let mut j = i;
            let mut chars = 0;
            while j < seg.words.len() {
                let w = &seg.words[j];
                if j > i && (w.end - start > MAX_SECS || chars + w.text.len() > MAX_CHARS) {
                    break;
                }
                chars += w.text.len() + 1;
                j += 1;
            }
            let text = join_words(seg.words[i..j].iter().map(|w| w.text.as_str()));
            let end = seg.words[j - 1].end;
            n += 1;
            if !vtt {
                writeln!(out, "{n}").unwrap();
            }
            writeln!(
                out,
                "{} --> {}",
                subtitle_clock(start, sep),
                subtitle_clock(end, sep)
            )
            .unwrap();
            match (vtt, speaker) {
                (true, Some(s)) => writeln!(out, "<v {}>{}\n", escape(s), escape(&text)).unwrap(),
                (false, Some(s)) => writeln!(out, "{s}: {text}\n").unwrap(),
                (true, None) => writeln!(out, "{}\n", escape(&text)).unwrap(),
                (false, None) => writeln!(out, "{text}\n").unwrap(),
            }
            i = j;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::*;

    fn sample() -> Transcript {
        let word = |text: &str, start: f64, end: f64, overlap: bool| Word {
            text: text.into(),
            start,
            end,
            conf: 0.9,
            overlap,
        };
        Transcript {
            version: FORMAT_VERSION,
            media: MediaInfo {
                path: "/x/int.m4a".into(),
                duration: 100.0,
                range_start: 0.0,
            },
            language: "de".into(),
            engine: EngineInfo {
                backend: "test".into(),
                model: "m".into(),
                created: 0,
            },
            speakers: vec![Speaker {
                id: "S1".into(),
                name: "Interviewer".into(),
            }],
            segments: vec![Segment {
                speaker: Some("S1".into()),
                start: 98.0,
                end: 104.0,
                words: vec![
                    word("Also", 98.0, 98.4, false),
                    word("ja.", 100.6, 101.0, true),
                    word("Genau.", 101.0, 101.5, false),
                ],
            }],
        }
    }

    #[test]
    fn txt_with_pauses_and_overlap() {
        let opts = ExportOptions {
            header: false,
            ..Default::default()
        };
        let out = render(&sample(), Format::Txt, &opts);
        assert_eq!(out, "Interviewer: [00:01:38] Also (..) //ja.// Genau.\n\n");
    }

    #[test]
    fn f4_timestamp() {
        let opts = ExportOptions {
            header: false,
            timestamps: TimestampStyle::F4,
            pause_min: None,
            mark_overlap: false,
        };
        let out = render(&sample(), Format::Txt, &opts);
        assert_eq!(out, "Interviewer: Also ja. Genau. #00:01:44-0#\n\n");
    }

    #[test]
    fn srt_clock() {
        assert_eq!(subtitle_clock(3723.5, ','), "01:02:03,500");
        let out = render(&sample(), Format::Srt, &ExportOptions::default());
        assert!(out.starts_with("1\n00:01:38,000 --> 00:01:41,500\nInterviewer: Also ja. Genau."));
    }
}
