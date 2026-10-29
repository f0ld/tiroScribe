<script lang="ts">
  import { tick } from "svelte";
  import { clock, speakerColor } from "../lib/format";
  import { segmentText, tokenize } from "../lib/editing";
  import type { Segment, Speaker } from "../lib/types";

  interface Props {
    seg: Segment;
    index: number;
    speakers: Speaker[];
    /** Index of the word being played, or -1. */
    activeWord: number;
    lowConfidence: number;
    /** Word indices matching the current search. */
    matches: Set<number> | undefined;
    /** Word index of the focused search match / uncertain word, or -1. */
    focusWord: number;
    editing: boolean;
    caretWord: number;
    onSeek: (time: number) => void;
    onStartEdit: (word: number) => void;
    onCommit: (text: string, splitAtWord: number | null) => void;
    onCancel: () => void;
    onSpeaker: (id: string | null) => void;
    onMergePrevious: () => void;
    onSplit: (word: number) => void;
  }

  let {
    seg,
    index,
    speakers,
    activeWord,
    lowConfidence,
    matches,
    focusWord,
    editing,
    caretWord,
    onSeek,
    onStartEdit,
    onCommit,
    onCancel,
    onSpeaker,
    onMergePrevious,
    onSplit,
  }: Props = $props();

  const speakerIndex = $derived(speakers.findIndex((s) => s.id === seg.speaker));
  const color = $derived(speakerIndex >= 0 ? speakerColor(speakerIndex) : "var(--muted)");

  let textarea = $state<HTMLTextAreaElement | null>(null);
  let draft = $state("");
  // Escape unmounts the textarea, which fires blur; make sure each edit ends exactly once.
  let finished = false;

  function commit(splitAtWord: number | null) {
    if (finished) return;
    finished = true;
    onCommit(draft, splitAtWord);
  }

  function cancel() {
    if (finished) return;
    finished = true;
    onCancel();
  }

  $effect(() => {
    if (editing) {
      finished = false;
      draft = segmentText(seg);
      tick().then(() => {
        if (!textarea) return;
        textarea.focus();
        autosize();
        const offset = seg.words.slice(0, Math.max(0, caretWord)).reduce((n, w) => n + w.text.length + 1, 0);
        textarea.setSelectionRange(offset, offset);
      });
    }
  });

  function autosize() {
    if (!textarea) return;
    textarea.style.height = "auto";
    textarea.style.height = `${textarea.scrollHeight}px`;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    } else if (e.key === "Enter" && e.altKey) {
      // Split the paragraph at the caret.
      e.preventDefault();
      const caret = textarea?.selectionStart ?? draft.length;
      commit(tokenize(draft.slice(0, caret)).length);
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      commit(null);
    }
  }

  function wordClick(e: MouseEvent, i: number) {
    if (e.altKey) onSplit(i);
    else onSeek(seg.words[i].start);
  }
</script>

<div class="para" class:playing={activeWord >= 0} data-index={index} style:--speaker={color}>
  <div class="meta">
    <select
      class="speaker"
      value={seg.speaker ?? ""}
      onchange={(e) => onSpeaker(e.currentTarget.value || null)}
      aria-label="Speaker"
    >
      {#each speakers as s (s.id)}
        <option value={s.id}>{s.name}</option>
      {/each}
      <option value="">Unknown</option>
    </select>
    <button class="time ghost" onclick={() => onSeek(seg.start)} title="Play from here">{clock(seg.start, true)}</button>
    <div class="tools">
      {#if index > 0}
        <button class="ghost" title="Merge with previous paragraph" aria-label="Merge with previous paragraph" onclick={onMergePrevious}>⤒</button>
      {/if}
      <button class="ghost" title="Edit (double-click a word)" aria-label="Edit paragraph" onclick={() => onStartEdit(0)}>✎</button>
    </div>
  </div>

  {#if editing}
    <textarea
      bind:this={textarea}
      bind:value={draft}
      oninput={autosize}
      onkeydown={onKey}
      onblur={() => commit(null)}
      spellcheck="true"
      lang="de"
    ></textarea>
    <p class="hint small muted"><kbd>Enter</kbd> save · <kbd>Alt</kbd>+<kbd>Enter</kbd> split here · <kbd>Esc</kbd> cancel</p>
  {:else}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <p class="text" ondblclick={() => onStartEdit(activeWord >= 0 ? activeWord : 0)}>
      {#each seg.words as w, i (i)}<span
          class="word"
          class:active={i === activeWord}
          class:uncertain={w.conf < lowConfidence}
          class:overlap={w.overlap}
          class:match={matches?.has(i)}
          class:focus={i === focusWord}
          role="button"
          tabindex="-1"
          title={w.conf < lowConfidence ? `Uncertain (${Math.round(w.conf * 100)} %)` : undefined}
          onclick={(e) => wordClick(e, i)}
          ondblclick={(e) => { e.stopPropagation(); onStartEdit(i); }}
          onkeydown={() => {}}>{w.text}</span>{" "}{/each}
    </p>
  {/if}
</div>

<style>
  .para {
    position: relative;
    padding: 10px 14px 12px 16px;
    border-left: 3px solid var(--speaker);
    border-radius: 0 var(--radius) var(--radius) 0;
    transition: background 0.15s;
  }

  .para.playing {
    background: var(--surface);
  }

  .para:hover .tools {
    opacity: 1;
  }

  .meta {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }

  .speaker {
    border: none;
    background-color: transparent;
    padding: 2px 22px 2px 6px;
    background-position: right 6px center;
    border-radius: 6px;
    font-weight: 700;
    font-size: 13px;
    color: var(--speaker);
    cursor: pointer;
  }

  .speaker:hover {
    background-color: var(--surface-2);
  }

  .time {
    padding: 2px 6px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }

  .tools {
    margin-left: auto;
    display: flex;
    gap: 2px;
    opacity: 0;
    transition: opacity 0.1s;
  }

  .tools button {
    padding: 2px 8px;
    color: var(--muted);
  }

  .text {
    margin: 0;
    font-family: var(--font-text);
    font-size: 17px;
    line-height: 1.65;
    cursor: text;
  }

  .word {
    border-radius: 3px;
    cursor: pointer;
  }

  .word:hover {
    background: var(--surface-2);
  }

  .word.uncertain {
    text-decoration: underline wavy var(--warn);
    text-decoration-thickness: 1px;
    text-underline-offset: 4px;
  }

  .word.overlap {
    color: var(--muted);
    font-style: italic;
  }

  .word.match {
    background: var(--highlight-soft);
  }

  .word.focus {
    outline: 2px solid var(--highlight);
  }

  .word.active {
    background: var(--accent-soft);
    color: var(--accent-strong);
    box-shadow: 0 0 0 2px var(--accent-soft);
  }

  textarea {
    width: 100%;
    resize: none;
    overflow: hidden;
    font-family: var(--font-text);
    font-size: 17px;
    line-height: 1.65;
    padding: 4px 6px;
    margin: -5px -7px 0;
    border-color: var(--accent);
  }

  .hint {
    margin: 4px 0 0;
  }
</style>
