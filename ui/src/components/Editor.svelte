<script lang="ts">
  import { tick } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { app } from "../lib/app.svelte";
  import { api, errorText } from "../lib/api";
  import { clock, duration as fmtDuration, fileName, speakerColor } from "../lib/format";
  import {
    History,
    addSpeaker,
    mergeSpeakers,
    mergeWithPrevious,
    replaceAll,
    search,
    setSegmentText,
    speakingTime,
    splitSegment,
    wordAt,
    type Match,
  } from "../lib/editing";
  import type { Transcript } from "../lib/types";
  import Paragraph from "./Paragraph.svelte";
  import ExportDialog from "./ExportDialog.svelte";

  // ---- document state -------------------------------------------------------------------
  let path = $state<string | null>(null);
  let t = $state.raw<Transcript | null>(null);
  let dirty = $state(false);
  let saving = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let history = new History();
  let canUndo = $state(false);
  let canRedo = $state(false);

  function syncHistory() {
    canUndo = history.canUndo;
    canRedo = history.canRedo;
  }
  let editing = $state<{ seg: number; word: number } | null>(null);
  let focus = $state<Match | null>(null);
  let showExport = $state(false);
  let scroller = $state<HTMLElement | null>(null);

  // ---- playback state -------------------------------------------------------------------
  let audio = $state<HTMLAudioElement | null>(null);
  let audioSrc = $state<string | null>(null);
  let audioError = $state<string | null>(null);
  let triedTranscode = false;
  let currentTime = $state(0);
  let mediaDuration = $state(0);
  let paused = $state(true);
  let rate = $state(1);
  let rewindPending = false;

  // ---- search ---------------------------------------------------------------------------
  let searchOpen = $state(false);
  let query = $state("");
  let replacement = $state("");
  let matchIndex = $state(0);
  let searchInput = $state<HTMLInputElement | null>(null);

  $effect(() => {
    const wanted = app.editorPath;
    if (wanted && wanted !== path) load(wanted);
  });

  async function load(p: string) {
    if (dirty) await saveNow();
    try {
      const loaded = await api.loadTranscript(p);
      path = p;
      t = loaded;
      history = new History();
      syncHistory();
      editing = null;
      focus = null;
      dirty = false;
      currentTime = 0;
      await setupAudio(loaded.media.path, false);
    } catch (e) {
      app.notify(errorText(e), true);
      app.editorPath = path;
    }
  }

  async function setupAudio(media: string, force: boolean) {
    audioError = null;
    triedTranscode = force;
    if (audioSrc) URL.revokeObjectURL(audioSrc);
    audioSrc = null;
    try {
      audioSrc = await api.playbackUrl(media, force);
    } catch (e) {
      audioSrc = null;
      audioError = errorText(e);
    }
  }

  async function onAudioError() {
    if (!t) return;
    if (!triedTranscode) {
      await setupAudio(t.media.path, true);
    } else {
      audioError = "The recording cannot be played. Was it moved or deleted?";
    }
  }

  async function locateMedia() {
    const picked = await open({ multiple: false });
    if (!picked || !t || Array.isArray(picked)) return;
    commit({ ...t, media: { ...t.media, path: picked } });
    await setupAudio(picked, false);
  }

  async function openFile() {
    const picked = await open({
      multiple: false,
      filters: [{ name: "liveScribe transcript", extensions: ["json"] }],
    });
    if (picked && !Array.isArray(picked)) app.editorPath = picked;
  }

  // ---- editing --------------------------------------------------------------------------
  function commit(next: Transcript) {
    if (!t) return;
    history.record(t);
    syncHistory();
    t = next;
    dirty = true;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(saveNow, 1200);
  }

  async function saveNow() {
    clearTimeout(saveTimer);
    if (!t || !path || saving) return;
    saving = true;
    const snapshot = t;
    try {
      await api.saveTranscript(path, snapshot);
      if (t === snapshot) dirty = false;
    } catch (e) {
      app.notify(`Saving failed: ${errorText(e)}`, true);
    } finally {
      saving = false;
    }
  }

  function undo() {
    if (!t) return;
    const prev = history.undo(t);
    if (prev) {
      t = prev;
      syncHistory();
      dirty = true;
      saveTimer = setTimeout(saveNow, 1200);
    }
  }

  function redo() {
    if (!t) return;
    const next = history.redo(t);
    if (next) {
      t = next;
      syncHistory();
      dirty = true;
      saveTimer = setTimeout(saveNow, 1200);
    }
  }

  function updateSegments(fn: (segs: Transcript["segments"]) => Transcript["segments"]) {
    if (t) commit({ ...t, segments: fn(t.segments) });
  }

  function onCommitText(i: number, text: string, splitAt: number | null) {
    editing = null;
    if (!t) return;
    const old = t.segments[i];
    const changed = text.trim().split(/\s+/).join(" ") !== old.words.map((w) => w.text).join(" ");
    if (!changed && splitAt === null) return;
    updateSegments((segs) => {
      const seg = changed ? setSegmentText(old, text) : old;
      let next = segs.map((s, k) => (k === i ? seg : s));
      if (seg.words.length === 0) return next.filter((_, k) => k !== i);
      if (splitAt !== null) next = splitSegment(next, i, splitAt);
      return next;
    });
  }

  function setSpeaker(i: number, speaker: string | null) {
    updateSegments((segs) => segs.map((s, k) => (k === i ? { ...s, speaker } : s)));
  }

  function renameSpeaker(id: string, name: string) {
    if (!t || !name.trim()) return;
    commit({ ...t, speakers: t.speakers.map((s) => (s.id === id ? { ...s, name: name.trim() } : s)) });
  }

  // ---- playback -------------------------------------------------------------------------
  function seek(time: number, play = true) {
    if (!audio) return;
    audio.currentTime = Math.max(0, time);
    currentTime = audio.currentTime;
    rewindPending = false;
    if (play && audio.paused) audio.play();
  }

  function togglePlay() {
    if (!audio) return;
    if (audio.paused) {
      if (rewindPending) audio.currentTime = Math.max(0, audio.currentTime - app.settings.editor.rewindOnResume);
      rewindPending = false;
      audio.play();
    } else {
      audio.pause();
      rewindPending = true;
    }
  }

  function skip(delta: number) {
    if (audio) seek(audio.currentTime + delta, !audio.paused);
  }

  function changeRate(delta: number) {
    rate = Math.min(2, Math.max(0.5, Math.round((rate + delta) * 10) / 10));
  }

  // Smooth highlighting: poll the position every frame while playing.
  $effect(() => {
    if (paused || !audio) return;
    let id = 0;
    const loop = () => {
      currentTime = audio!.currentTime;
      id = requestAnimationFrame(loop);
    };
    loop();
    return () => cancelAnimationFrame(id);
  });

  const activeSeg = $derived.by(() => {
    if (!t) return -1;
    const segs = t.segments;
    let lo = 0;
    let hi = segs.length - 1;
    let found = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (segs[mid].start <= currentTime + 0.05) {
        found = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    return found >= 0 && currentTime <= segs[found].end + 1 ? found : -1;
  });

  const activeWord = $derived(activeSeg >= 0 && t ? wordAt(t.segments[activeSeg], currentTime + 0.05) : -1);

  function scrollToSeg(i: number, onlyIfHidden: boolean) {
    const el = scroller?.querySelector<HTMLElement>(`[data-index="${i}"]`);
    if (!el || !scroller) return;
    const r = el.getBoundingClientRect();
    const c = scroller.getBoundingClientRect();
    const hidden = r.top < c.top + 40 || r.bottom > c.bottom - 40;
    if (!onlyIfHidden || hidden) el.scrollIntoView({ block: "center", behavior: "smooth" });
  }

  $effect(() => {
    if (activeSeg >= 0 && !paused && !editing && app.settings.editor.followPlayback) scrollToSeg(activeSeg, true);
  });

  // ---- search & uncertain words -----------------------------------------------------------
  const matches = $derived(searchOpen && t && query.trim().length >= 2 ? search(t, query) : []);
  const matchesBySeg = $derived.by(() => {
    const m = new Map<number, Set<number>>();
    for (const x of matches) {
      if (!m.has(x.seg)) m.set(x.seg, new Set());
      m.get(x.seg)!.add(x.word);
    }
    return m;
  });

  function gotoMatch(m: Match | undefined) {
    if (!m || !t) return;
    focus = m;
    seek(t.segments[m.seg].words[m.word].start, false);
    tick().then(() => scrollToSeg(m.seg, false));
  }

  function stepMatch(dir: 1 | -1) {
    if (!matches.length) return;
    matchIndex = (matchIndex + dir + matches.length) % matches.length;
    gotoMatch(matches[matchIndex]);
  }

  function doReplaceAll() {
    if (!t) return;
    const [next, n] = replaceAll(t, query, replacement);
    if (n > 0) commit(next);
    app.notify(n === 1 ? "1 replacement" : `${n} replacements`);
  }

  async function openSearch() {
    searchOpen = true;
    await tick();
    searchInput?.focus();
    searchInput?.select();
  }

  const uncertain = $derived.by(() => {
    const out: Match[] = [];
    const th = app.settings.editor.lowConfidence;
    t?.segments.forEach((s, si) => s.words.forEach((w, wi) => w.conf < th && out.push({ seg: si, word: wi })));
    return out;
  });

  function stepUncertain(dir: 1 | -1) {
    if (!uncertain.length) return;
    const from = focus ?? (activeSeg >= 0 ? { seg: activeSeg, word: activeWord } : { seg: 0, word: -1 });
    const cmp = (m: Match) => (m.seg - from.seg) * 1e6 + (m.word - from.word);
    const next =
      dir === 1
        ? (uncertain.find((m) => cmp(m) > 0) ?? uncertain[0])
        : ([...uncertain].reverse().find((m) => cmp(m) < 0) ?? uncertain[uncertain.length - 1]);
    gotoMatch(next);
  }

  // ---- keyboard ---------------------------------------------------------------------------
  function onKeydown(e: KeyboardEvent) {
    if (!t) return;
    const inField = e.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName);
    const ctrl = e.ctrlKey || e.metaKey;
    const handled = () => e.preventDefault();

    if (e.key === "F4" || (ctrl && e.code === "Space")) return handled(), togglePlay();
    if (e.key === "F3") return handled(), skip(-5);
    if (e.key === "F5") return handled(), skip(5);
    if (e.key === "F7") return handled(), changeRate(-0.1);
    if (e.key === "F8") return handled(), changeRate(0.1);
    if (e.key === "F9") return handled(), stepUncertain(e.shiftKey ? -1 : 1);
    if (ctrl && e.key === "s") return handled(), saveNow();
    if (ctrl && e.key === "f") return handled(), openSearch();
    if (e.key === "Escape" && searchOpen && !editing) return handled(), (searchOpen = false);
    if (inField) return;
    if (ctrl && e.key === "ArrowLeft") return handled(), skip(-5);
    if (ctrl && e.key === "ArrowRight") return handled(), skip(5);
    if (ctrl && e.key.toLowerCase() === "z") return handled(), e.shiftKey ? redo() : undo();
    if (ctrl && e.key === "y") return handled(), redo();
  }

  const times = $derived(t ? speakingTime(t) : new Map());
  const totalTime = $derived([...times.values()].reduce((a, b) => a + b, 0) || 1);
  const unknownCount = $derived(t ? t.segments.filter((s) => !s.speaker).length : 0);
  const wordCount = $derived(t ? t.segments.reduce((n, s) => n + s.words.length, 0) : 0);
</script>

<svelte:window onkeydown={onKeydown} onbeforeunload={() => dirty && saveNow()} />

{#if !t}
  <div class="empty">
    <h1>Editor</h1>
    <p class="muted">Open a transcript to review and correct it while listening to the recording.</p>
    <button class="primary" onclick={openFile}>Open transcript…</button>
    {#if app.jobs.some((j) => j.status === "done")}
      <div class="recent">
        <h2>Recently transcribed</h2>
        {#each app.jobs.filter((j) => j.status === "done") as j (j.id)}
          <button class="ghost" onclick={() => (app.editorPath = j.spec.output)}>{fileName(j.spec.output)}</button>
        {/each}
      </div>
    {/if}
  </div>
{:else}
  <div class="editor">
    <header class="toolbar">
      <div class="title">
        <strong>{fileName(t.media.path)}</strong>
        <span class="small muted">
          {saving ? "Saving…" : dirty ? "Unsaved changes" : "All changes saved"}
        </span>
      </div>
      <button class="ghost" onclick={undo} disabled={!canUndo} title="Undo (Ctrl+Z)" aria-label="Undo">↶</button>
      <button class="ghost" onclick={redo} disabled={!canRedo} title="Redo (Ctrl+Shift+Z)" aria-label="Redo">↷</button>
      <button onclick={openSearch} title="Search & replace (Ctrl+F)">Search</button>
      <button onclick={openFile}>Open…</button>
      <button class="primary" onclick={() => (showExport = true)}>Export…</button>
    </header>

    <div class="player">
      <button class="play" onclick={togglePlay} disabled={!audioSrc} aria-label={paused ? "Play" : "Pause"} title="Play / pause (F4)">
        {#if paused}
          <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor"><path d="M7 4v16l13-8z" /></svg>
        {:else}
          <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor"><path d="M6 4h4v16H6zM14 4h4v16h-4z" /></svg>
        {/if}
      </button>
      <button class="ghost" onclick={() => skip(-5)} title="Back 5 s (F3)">−5 s</button>
      <button class="ghost" onclick={() => skip(5)} title="Forward 5 s (F5)">+5 s</button>
      <span class="clock">{clock(currentTime, true)} / {clock(mediaDuration || t.media.duration, true)}</span>
      <input
        class="seek"
        type="range"
        min="0"
        max={mediaDuration || t.media.duration}
        step="0.1"
        value={currentTime}
        oninput={(e) => seek(Number(e.currentTarget.value), false)}
        aria-label="Position"
      />
      <label class="inline small" title="Playback speed (F7 / F8)">
        <select bind:value={rate} aria-label="Speed">
          {#each [0.5, 0.6, 0.7, 0.8, 0.9, 1, 1.1, 1.2, 1.3, 1.5, 1.75, 2] as r (r)}
            <option value={r}>{r}×</option>
          {/each}
        </select>
      </label>
      <label class="inline small">
        <input type="checkbox" bind:checked={app.settings.editor.followPlayback} onchange={() => app.saveSettings()} />
        Follow
      </label>
      {#if audioSrc}
        <audio
          bind:this={audio}
          src={audioSrc}
          bind:paused
          bind:playbackRate={rate}
          bind:duration={mediaDuration}
          ontimeupdate={() => paused && audio && (currentTime = audio.currentTime)}
          onerror={onAudioError}
          preload="auto"
        ></audio>
      {/if}
    </div>

    {#if audioError}
      <div class="notice">
        {audioError}
        <button onclick={locateMedia}>Locate recording…</button>
      </div>
    {/if}

    {#if searchOpen}
      <div class="searchbar">
        <input
          bind:this={searchInput}
          type="text"
          placeholder="Search"
          bind:value={query}
          oninput={() => (matchIndex = -1)}
          onkeydown={(e) => e.key === "Enter" && (e.preventDefault(), stepMatch(e.shiftKey ? -1 : 1))}
        />
        <span class="small muted count">{matches.length ? `${matchIndex >= 0 ? matchIndex + 1 : "–"} / ${matches.length}` : query.trim().length >= 2 ? "no matches" : ""}</span>
        <button class="ghost" onclick={() => stepMatch(-1)} disabled={!matches.length} aria-label="Previous match">↑</button>
        <button class="ghost" onclick={() => stepMatch(1)} disabled={!matches.length} aria-label="Next match">↓</button>
        <input type="text" placeholder="Replace with" bind:value={replacement} />
        <button onclick={doReplaceAll} disabled={!matches.length}>Replace all</button>
        <button class="ghost" onclick={() => (searchOpen = false)} aria-label="Close search">✕</button>
      </div>
    {/if}

    <div class="body">
      <div class="scroller" bind:this={scroller}>
        <div class="paper">
          {#each t.segments as seg, i (i)}
            <Paragraph
              {seg}
              index={i}
              speakers={t.speakers}
              activeWord={i === activeSeg ? activeWord : -1}
              lowConfidence={app.settings.editor.lowConfidence}
              matches={matchesBySeg.get(i)}
              focusWord={focus?.seg === i ? focus.word : -1}
              editing={editing?.seg === i}
              caretWord={editing?.seg === i ? editing.word : 0}
              onSeek={(time) => seek(time)}
              onStartEdit={(word) => (editing = { seg: i, word })}
              onCommit={(text, splitAt) => onCommitText(i, text, splitAt)}
              onCancel={() => (editing = null)}
              onSpeaker={(id) => setSpeaker(i, id)}
              onMergePrevious={() => updateSegments((segs) => mergeWithPrevious(segs, i))}
              onSplit={(word) => updateSegments((segs) => splitSegment(segs, i, word))}
            />
          {/each}
        </div>
      </div>

      <aside>
        <section>
          <h2>Speakers</h2>
          {#each t.speakers as s, si (s.id)}
            <div class="speaker">
              <span class="dot" style:background={speakerColor(si)}></span>
              <input
                type="text"
                value={s.name}
                onchange={(e) => renameSpeaker(s.id, e.currentTarget.value)}
                aria-label={`Name of ${s.id}`}
              />
              <span class="small muted share">{Math.round(((times.get(s.id) ?? 0) / totalTime) * 100)} %</span>
              {#if t.speakers.length > 1}
                <select
                  class="merge"
                  value=""
                  onchange={(e) => { const into = e.currentTarget.value; if (into && t) commit(mergeSpeakers(t, s.id, into)); e.currentTarget.value = ""; }}
                  title="Merge this speaker into another"
                  aria-label={`Merge ${s.name} into`}
                >
                  <option value="">⋯</option>
                  {#each t.speakers.filter((o) => o.id !== s.id) as o (o.id)}
                    <option value={o.id}>Merge into {o.name}</option>
                  {/each}
                </select>
              {/if}
            </div>
          {/each}
          {#if unknownCount > 0}
            <p class="small muted">{unknownCount} paragraph{unknownCount === 1 ? "" : "s"} without speaker</p>
          {/if}
          <button class="ghost small add" onclick={() => { if (t) { const [next] = addSpeaker(t, ""); commit(next); } }}>+ Add speaker</button>
        </section>

        <section>
          <h2>Review</h2>
          <p class="small muted">{uncertain.length} uncertain word{uncertain.length === 1 ? "" : "s"}</p>
          <div class="row">
            <button onclick={() => stepUncertain(-1)} disabled={!uncertain.length}>← Previous</button>
            <button onclick={() => stepUncertain(1)} disabled={!uncertain.length}>Next →</button>
          </div>
        </section>

        <section class="shortcuts">
          <h2>Shortcuts</h2>
          <dl class="small">
            <dt><kbd>F4</kbd> / <kbd>Ctrl</kbd>+<kbd>Space</kbd></dt><dd>Play / pause</dd>
            <dt><kbd>F3</kbd> <kbd>F5</kbd></dt><dd>Back / forward 5 s</dd>
            <dt><kbd>F7</kbd> <kbd>F8</kbd></dt><dd>Slower / faster</dd>
            <dt><kbd>F9</kbd></dt><dd>Next uncertain word</dd>
            <dt>Click word</dt><dd>Play from there</dd>
            <dt>Double-click</dt><dd>Edit paragraph</dd>
            <dt><kbd>Alt</kbd>+click</dt><dd>New paragraph here</dd>
            <dt><kbd>Ctrl</kbd>+<kbd>F</kbd></dt><dd>Search & replace</dd>
          </dl>
        </section>

        <section class="small muted meta">
          <div>{wordCount.toLocaleString()} words · {fmtDuration(t.media.duration)}</div>
          <div>{t.engine.model}</div>
          <div>{t.engine.backend}</div>
        </section>
      </aside>
    </div>
  </div>
{/if}

{#if showExport && t && path}
  <ExportDialog transcript={t} {path} onClose={() => (showExport = false)} />
{/if}

<style>
  .empty {
    padding: 28px 36px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 12px;
  }

  .empty p {
    margin: 0;
  }

  .recent {
    margin-top: 20px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
  }

  .editor {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 12px 20px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .title {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .title strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .player {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 20px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .play {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    padding: 0;
    background: var(--accent);
    border-color: var(--accent);
    color: var(--surface);
  }

  .play:hover:not(:disabled) {
    background: var(--accent-strong);
  }

  .clock {
    font-variant-numeric: tabular-nums;
    font-size: 13px;
    white-space: nowrap;
  }

  .seek {
    flex: 1;
    accent-color: var(--accent);
  }

  .notice {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 8px 20px;
    background: var(--warn-soft);
    color: var(--warn);
  }

  .searchbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 20px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-2);
  }

  .count {
    min-width: 70px;
  }

  .body {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr 270px;
    min-height: 0;
  }

  .scroller {
    overflow-y: auto;
    padding: 24px 32px 50vh;
  }

  .paper {
    max-width: 760px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  aside {
    border-left: 1px solid var(--border);
    overflow-y: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 22px;
    background: var(--surface);
  }

  aside section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  aside p {
    margin: 0;
  }

  .speaker {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex: none;
  }

  .speaker input {
    flex: 1;
    min-width: 0;
    padding: 4px 6px;
  }

  .share {
    width: 36px;
    text-align: right;
  }

  .merge {
    width: 30px;
    padding: 4px 0;
    text-align: center;
    background-image: none;
    border-color: transparent;
    background-color: transparent;
    color: var(--muted);
  }

  .add {
    align-self: flex-start;
    padding: 2px 0;
    color: var(--accent);
  }

  .row {
    display: flex;
    gap: 6px;
  }

  .shortcuts dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 10px;
    margin: 0;
    align-items: center;
  }

  .shortcuts dd {
    margin: 0;
    color: var(--muted);
  }

  .meta {
    gap: 2px;
  }
</style>
