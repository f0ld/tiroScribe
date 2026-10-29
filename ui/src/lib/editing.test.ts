import { test } from "node:test";
import assert from "node:assert/strict";
import {
  History,
  mergeSpeakers,
  mergeWithPrevious,
  replaceAll,
  retext,
  search,
  splitSegment,
  wordAt,
} from "./editing.ts";
import type { Segment, Transcript, Word } from "./types";

const w = (text: string, start: number, end: number, conf = 0.5): Word => ({ text, start, end, conf });

const seg = (speaker: string, words: Word[]): Segment => ({
  speaker,
  start: words[0].start,
  end: words[words.length - 1].end,
  words,
});

const transcript = (segments: Segment[]): Transcript => ({
  version: 1,
  media: { path: "/x.ogg", duration: 100, range_start: 0 },
  language: "de",
  engine: { backend: "test", model: "m", created: 0 },
  speakers: [
    { id: "S1", name: "Interviewer" },
    { id: "S2", name: "Befragte" },
    { id: "S3", name: "S3" },
  ],
  segments,
});

const words = [w("Ich", 0, 1), w("weiss", 1, 2), w("es", 2, 3), w("nicht.", 3, 4)];

test("retext keeps timing of unchanged words", () => {
  const out = retext(words, "Ich weiß es nicht.", 0, 4);
  assert.deepEqual(
    out.map((x) => x.text),
    ["Ich", "weiß", "es", "nicht."],
  );
  assert.equal(out[0], words[0]); // untouched object
  assert.deepEqual([out[1].start, out[1].end, out[1].conf], [1, 2, 1]); // replacement takes span
  assert.equal(out[3], words[3]);
});

test("retext spreads inserted words over the replaced span", () => {
  const out = retext(words, "Ich weiss das eben nicht.", 0, 4);
  assert.deepEqual(
    out.map((x) => [x.text, x.start, x.end]),
    [
      ["Ich", 0, 1],
      ["weiss", 1, 2],
      ["das", 2, 2.5],
      ["eben", 2.5, 3],
      ["nicht.", 3, 4],
    ],
  );
});

test("retext handles pure insertions and deletions", () => {
  const ins = retext(words, "Also Ich weiss es nicht.", 0, 4);
  assert.equal(ins[0].text, "Also");
  assert.equal(ins[0].start, 0);
  const del = retext(words, "Ich nicht.", 0, 4);
  assert.deepEqual(
    del.map((x) => x.text),
    ["Ich", "nicht."],
  );
  assert.equal(retext(words, "   ", 0, 4).length, 0);
});

test("split and merge paragraphs", () => {
  const segs = [seg("S1", words)];
  const split = splitSegment(segs, 0, 2);
  assert.equal(split.length, 2);
  assert.deepEqual([split[1].start, split[1].end, split[1].speaker], [2, 4, "S1"]);
  const merged = mergeWithPrevious(split, 1);
  assert.equal(merged.length, 1);
  assert.equal(merged[0].words.length, 4);
});

test("mergeSpeakers joins only paragraphs affected by the merge", () => {
  const t = transcript([
    seg("S1", [w("a", 0, 1)]),
    seg("S1", [w("b", 1, 2)]), // separate on purpose; must stay separate
    seg("S3", [w("c", 2, 3)]),
    seg("S2", [w("d", 3, 4)]),
  ]);
  const out = mergeSpeakers(t, "S3", "S1");
  assert.deepEqual(
    out.segments.map((s) => s.words.map((x) => x.text).join(" ")),
    ["a", "b c", "d"],
  );
  assert.ok(!out.speakers.some((s) => s.id === "S3"));
});

test("search and replace", () => {
  const t = transcript([seg("S1", [w("Weber-Wulf,", 0, 1), w("wir", 1, 2)]), seg("S2", [w("Frau", 2, 3), w("Weber-Wulf", 3, 4)])]);
  assert.equal(search(t, "weber").length, 2);
  assert.deepEqual(search(t, "Frau Weber"), [{ seg: 1, word: 0 }]);
  const [out, n] = replaceAll(t, "Weber-Wulf", "Weber-Wulff");
  assert.equal(n, 2);
  assert.equal(out.segments[0].words[0].text, "Weber-Wulff,");
  assert.equal(out.segments[0].words[1], t.segments[0].words[1]);
});

test("wordAt finds the word being played", () => {
  const s = seg("S1", words);
  assert.equal(wordAt(s, -1), -1);
  assert.equal(wordAt(s, 2.5), 2);
  assert.equal(wordAt(s, 99), 3);
});

test("history undo and redo", () => {
  const h = new History();
  const a = transcript([seg("S1", words)]);
  const b = { ...a, language: "en" };
  h.record(a);
  const undone = h.undo(b)!;
  assert.equal(undone.language, "de");
  assert.equal(h.redo(undone)!.language, "en");
  assert.equal(h.canRedo, false);
});
