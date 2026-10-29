// Pure transcript editing operations. Word timings are preserved wherever the text did not
// change, so click-to-seek and playback highlighting keep working after corrections.

import type { Segment, Transcript, Word } from "./types";

/** Splits paragraph text into word tokens (punctuation stays attached, like Whisper output). */
export function tokenize(text: string): string[] {
  return text.split(/\s+/).filter(Boolean);
}

type Op = { kind: "eq"; a: number; b: number } | { kind: "del"; a: number } | { kind: "ins"; b: number };

/** Longest-common-subsequence diff of two token lists. */
function diff(a: string[], b: string[]): Op[] {
  const n = a.length;
  const m = b.length;
  const dp: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const ops: Op[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) ops.push({ kind: "eq", a: i++, b: j++ });
    else if (dp[i + 1][j] >= dp[i][j + 1]) ops.push({ kind: "del", a: i++ });
    else ops.push({ kind: "ins", b: j++ });
  }
  while (i < n) ops.push({ kind: "del", a: i++ });
  while (j < m) ops.push({ kind: "ins", b: j++ });
  return ops;
}

/**
 * Applies edited text to a paragraph. Unchanged words keep their timing; changed or new words
 * share the time span of the words they replace (or the gap they were inserted into) and are
 * marked as verified (`conf = 1`).
 */
export function retext(old: Word[], text: string, segStart: number, segEnd: number): Word[] {
  const tokens = tokenize(text);
  const ops = diff(
    old.map((w) => w.text),
    tokens,
  );
  const out: Word[] = [];
  let pendingIns: string[] = [];
  let pendingDel: Word[] = [];

  const flush = (nextStart: number) => {
    if (pendingIns.length) {
      const prevEnd = out.length ? out[out.length - 1].end : segStart;
      const start = pendingDel.length ? pendingDel[0].start : prevEnd;
      const end = pendingDel.length ? pendingDel[pendingDel.length - 1].end : Math.max(prevEnd, nextStart);
      const step = (end - start) / pendingIns.length;
      pendingIns.forEach((t, k) => {
        out.push({ text: t, start: start + k * step, end: start + (k + 1) * step, conf: 1 });
      });
    }
    pendingIns = [];
    pendingDel = [];
  };

  for (const op of ops) {
    if (op.kind === "eq") {
      flush(old[op.a].start);
      out.push(old[op.a]);
    } else if (op.kind === "del") {
      pendingDel.push(old[op.a]);
    } else {
      pendingIns.push(tokens[op.b]);
    }
  }
  flush(segEnd);
  return out;
}

export function segmentText(seg: Segment): string {
  return seg.words.map((w) => w.text).join(" ");
}

function bounds(seg: Segment): Segment {
  if (seg.words.length) {
    seg.start = seg.words[0].start;
    seg.end = seg.words[seg.words.length - 1].end;
  }
  return seg;
}

export function setSegmentText(seg: Segment, text: string): Segment {
  return bounds({ ...seg, words: retext(seg.words, text, seg.start, seg.end) });
}

/** Splits a paragraph before word `at`; the second half keeps the speaker. */
export function splitSegment(segments: Segment[], index: number, at: number): Segment[] {
  const seg = segments[index];
  if (at <= 0 || at >= seg.words.length) return segments;
  const first = bounds({ ...seg, words: seg.words.slice(0, at) });
  const second = bounds({ ...seg, words: seg.words.slice(at) });
  return [...segments.slice(0, index), first, second, ...segments.slice(index + 1)];
}

/** Merges a paragraph into the previous one (keeping the previous speaker). */
export function mergeWithPrevious(segments: Segment[], index: number): Segment[] {
  if (index <= 0) return segments;
  const prev = segments[index - 1];
  const merged = bounds({ ...prev, words: [...prev.words, ...segments[index].words] });
  return [...segments.slice(0, index - 1), merged, ...segments.slice(index + 1)];
}

/** Moves all paragraphs of `from` to `into` and removes `from`. Paragraphs that become
 *  adjacent with the same speaker because of the merge are joined; others stay untouched. */
export function mergeSpeakers(t: Transcript, from: string, into: string): Transcript {
  const segments: Segment[] = [];
  let lastTouched = false;
  for (const s of t.segments) {
    const touched = s.speaker === from;
    const seg = touched ? { ...s, speaker: into } : s;
    const last = segments[segments.length - 1];
    if (last && last.speaker === into && seg.speaker === into && (touched || lastTouched)) {
      segments[segments.length - 1] = bounds({ ...last, words: [...last.words, ...seg.words] });
      lastTouched = true;
    } else {
      segments.push(seg);
      lastTouched = touched;
    }
  }
  return { ...t, segments, speakers: t.speakers.filter((sp) => sp.id !== from) };
}

export function addSpeaker(t: Transcript, name: string): [Transcript, string] {
  let n = t.speakers.length + 1;
  while (t.speakers.some((s) => s.id === `S${n}`)) n++;
  const id = `S${n}`;
  return [{ ...t, speakers: [...t.speakers, { id, name: name || id }] }, id];
}

/** Speaking time per speaker id, in seconds. */
export function speakingTime(t: Transcript): Map<string | null, number> {
  const m = new Map<string | null, number>();
  for (const s of t.segments) m.set(s.speaker, (m.get(s.speaker) ?? 0) + (s.end - s.start));
  return m;
}

export interface Match {
  seg: number;
  word: number;
}

/** Finds words containing `query` (case-insensitive). Multi-word queries match word runs. */
export function search(t: Transcript, query: string): Match[] {
  const q = tokenize(query.toLowerCase());
  if (!q.length) return [];
  const matches: Match[] = [];
  t.segments.forEach((seg, si) => {
    const words = seg.words.map((w) => w.text.toLowerCase());
    for (let i = 0; i + q.length <= words.length; i++) {
      const ok =
        q.length === 1
          ? words[i].includes(q[0])
          : q.every((tok, k) => (k === 0 ? words[i + k].endsWith(tok) : k === q.length - 1 ? words[i + k].startsWith(tok) : words[i + k] === tok));
      if (ok) matches.push({ seg: si, word: i });
    }
  });
  return matches;
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Case-insensitive replace in all paragraphs; returns the new transcript and the count. */
export function replaceAll(t: Transcript, query: string, replacement: string): [Transcript, number] {
  if (!query.trim()) return [t, 0];
  const re = new RegExp(escapeRegExp(query.trim()), "gi");
  let count = 0;
  const segments = t.segments.map((seg) => {
    const text = segmentText(seg);
    const replaced = text.replace(re, () => {
      count++;
      return replacement;
    });
    return replaced === text ? seg : setSegmentText(seg, replaced);
  });
  return [{ ...t, segments }, count];
}

/** First word index at or after `time` in a paragraph (for highlighting during playback). */
export function wordAt(seg: Segment, time: number): number {
  let lo = 0;
  let hi = seg.words.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (seg.words[mid].start <= time) {
      found = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  return found;
}

/** Undo/redo history of whole-transcript snapshots. Transcripts are small enough. */
export class History {
  private past: string[] = [];
  private future: string[] = [];

  record(t: Transcript) {
    this.past.push(JSON.stringify(t));
    if (this.past.length > 200) this.past.shift();
    this.future = [];
  }

  undo(current: Transcript): Transcript | null {
    const prev = this.past.pop();
    if (!prev) return null;
    this.future.push(JSON.stringify(current));
    return JSON.parse(prev);
  }

  redo(current: Transcript): Transcript | null {
    const next = this.future.pop();
    if (!next) return null;
    this.past.push(JSON.stringify(current));
    return JSON.parse(next);
  }

  get canUndo() {
    return this.past.length > 0;
  }
  get canRedo() {
    return this.future.length > 0;
  }
}

