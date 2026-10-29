<script lang="ts">
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { app } from "../lib/app.svelte";
  import { api, errorText } from "../lib/api";
  import { LANGUAGES, duration, fileName, parseClock } from "../lib/format";
  import type { JobSpec, JobView, Stage } from "../lib/types";

  const MEDIA_EXTENSIONS = ["mp3", "m4a", "wav", "ogg", "oga", "opus", "flac", "aac", "wma", "mp4", "mov", "mkv", "webm", "avi", "amr", "3gp"];

  const job = $derived(app.settings.job);
  let files = $state<{ media: string; output: string }[]>([]);
  let rangeStart = $state("");
  let rangeEnd = $state("");
  let showAdvanced = $state(false);
  let adding = $state(false);

  const speakerChoice = $derived(
    job.speakers.mode === "off" ? "off" : job.speakers.mode === "auto" ? "auto" : String(job.speakers.count),
  );

  function setSpeakers(value: string) {
    job.speakers =
      value === "off" ? { mode: "off" } : value === "auto" ? { mode: "auto", threshold: 0.8 } : { mode: "fixed", count: Number(value) };
    app.saveSettings();
  }

  const deviceChoice = $derived(job.device.kind === "gpu" ? `gpu:${job.device.index}` : job.device.kind);

  function setDevice(value: string) {
    job.device = value.startsWith("gpu:") ? { kind: "gpu", index: Number(value.slice(4)) } : value === "npu" ? { kind: "npu" } : { kind: "cpu" };
    app.saveSettings();
  }

  async function pickFiles() {
    const picked = await open({
      multiple: true,
      filters: [{ name: "Audio & video", extensions: MEDIA_EXTENSIONS }],
    });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    const added = await Promise.all(paths.map(async (media) => ({ media, output: await api.defaultOutputPath(media) })));
    files = [...files, ...added.filter((a) => !files.some((f) => f.media === a.media))];
  }

  async function changeOutput(i: number) {
    const path = await save({
      defaultPath: files[i].output,
      filters: [{ name: "liveScribe transcript", extensions: ["json"] }],
    });
    if (path) files[i].output = path.endsWith(".scribe.json") ? path : path.replace(/(\.json)?$/, ".scribe.json");
  }

  const modelReady = $derived(job.device.kind === "npu" || app.installedWhisper.some((m) => m.id === job.model));
  const canStart = $derived(files.length > 0 && modelReady && app.missingEssentials.length === 0 && app.info?.ffmpeg);

  async function enqueue() {
    adding = true;
    try {
      const start = parseClock(rangeStart);
      const end = parseClock(rangeEnd);
      for (const f of files) {
        const spec: JobSpec = {
          media: f.media,
          output: f.output,
          range: { start, end },
          language: job.language === "auto" ? null : job.language,
          model: job.model,
          device: job.device,
          quality: job.quality,
          verbatim: job.verbatim,
          vocabulary: job.vocabulary
            .split(/[,\n]/)
            .map((v) => v.trim())
            .filter(Boolean),
          speakers: job.speakers,
          vad: job.vad,
          npu: app.settings.npu,
        };
        await api.enqueueJob($state.snapshot(spec) as JobSpec);
      }
      files = [];
      rangeStart = rangeEnd = "";
      app.jobs = await api.listJobs();
    } catch (e) {
      app.notify(errorText(e), true);
    } finally {
      adding = false;
    }
  }

  const STAGE_LABEL: Record<Stage, string> = {
    loading: "Loading models",
    decoding: "Reading audio",
    detecting_speech: "Detecting speech",
    transcribing: "Transcribing",
    diarizing: "Identifying speakers",
    finishing: "Finishing",
  };

  function eta(j: JobView): string {
    const p = j.progress?.overall ?? 0;
    if (!j.elapsed || p < 0.1) return "";
    return `about ${duration((j.elapsed / p) * (1 - p))} left`;
  }

  async function removeFinished() {
    for (const j of app.jobs.filter((j) => j.status !== "running" && j.status !== "queued")) {
      await api.removeJob(j.id);
    }
    app.jobs = await api.listJobs();
  }
</script>

<div class="page">
  <header>
    <h1>New transcription</h1>
    <p class="muted">Audio stays on this computer. Results are saved next to the recording as <code>.scribe.json</code>.</p>
  </header>

  {#if app.info && !app.info.ffmpeg}
    <div class="notice warn">ffmpeg was not found. Install it (<code>sudo dnf install ffmpeg</code>) to read audio files.</div>
  {/if}
  {#if app.missingEssentials.length > 0 || app.installedWhisper.length === 0}
    <div class="notice">
      Models need to be downloaded once before the first transcription.
      <button onclick={() => (app.view = "models")}>Open models</button>
    </div>
  {/if}

  <section class="card form">
    <div class="files">
      {#if files.length === 0}
        <button class="drop" onclick={pickFiles}>
          <strong>Choose recordings…</strong>
          <span class="muted small">MP3, M4A, WAV, video files and more · several files are queued one after another</span>
        </button>
      {:else}
        <ul>
          {#each files as f, i (f.media)}
            <li>
              <div class="file">
                <strong>{fileName(f.media)}</strong>
                <button class="ghost small muted" onclick={() => changeOutput(i)} title="Change where the transcript is saved">→ {fileName(f.output)}</button>
              </div>
              <button class="ghost" aria-label="Remove" onclick={() => (files = files.filter((_, k) => k !== i))}>✕</button>
            </li>
          {/each}
        </ul>
        <button onclick={pickFiles}>Add more…</button>
      {/if}
    </div>

    <div class="grid">
      <label>
        Language
        <select bind:value={job.language} onchange={() => app.saveSettings()}>
          <option value="auto">Detect automatically</option>
          {#each LANGUAGES as [code, name] (code)}
            <option value={code}>{name}</option>
          {/each}
        </select>
      </label>

      <label>
        Speakers
        <select value={speakerChoice} onchange={(e) => setSpeakers(e.currentTarget.value)}>
          <option value="auto">Detect automatically</option>
          {#each [1, 2, 3, 4, 5, 6, 7, 8] as n (n)}
            <option value={String(n)}>{n} {n === 1 ? "speaker" : "speakers"}{n === 2 ? " (interview)" : ""}</option>
          {/each}
          <option value="off">Don't separate speakers</option>
        </select>
      </label>

      <label>
        Run on
        <select value={deviceChoice} onchange={(e) => setDevice(e.currentTarget.value)}>
          {#each app.info?.gpus ?? [] as g (g.index)}
            <option value={`gpu:${g.index}`}>GPU – {g.name}</option>
          {/each}
          <option value="cpu">CPU (slow)</option>
          <option value="npu">NPU – via local server{app.info?.npu_present ? "" : " (no NPU found)"}</option>
        </select>
      </label>

      <label>
        Model
        {#if job.device.kind === "npu"}
          <input type="text" value={app.settings.npu.model} disabled title="Configured in Settings" />
        {:else}
          <select bind:value={job.model} onchange={() => app.saveSettings()}>
            {#each app.models.filter((m) => m.kind === "whisper") as m (m.id)}
              <option value={m.id} disabled={!m.installed}>{m.name}{m.installed ? "" : " – not downloaded"}</option>
            {/each}
          </select>
        {/if}
      </label>

      <label>
        Quality
        <select bind:value={job.quality} onchange={() => app.saveSettings()}>
          <option value="precise">Precise (recommended)</option>
          <option value="fast">Fast</option>
        </select>
      </label>

      <label class="inline check">
        <input type="checkbox" bind:checked={job.verbatim} onchange={() => app.saveSettings()} />
        <span>Verbatim<br /><span class="muted small">keep “äh”, “ähm”, repetitions</span></span>
      </label>
    </div>

    <button class="ghost toggle" onclick={() => (showAdvanced = !showAdvanced)} aria-expanded={showAdvanced}>
      {showAdvanced ? "▾" : "▸"} More options
    </button>

    {#if showAdvanced}
      <div class="grid advanced">
        <label class="wide">
          Vocabulary – names and terms that should be spelled correctly (comma separated)
          <textarea rows="2" bind:value={job.vocabulary} onchange={() => app.saveSettings()} placeholder="Bourdieu, Habitus, Kreuzberg, Dr. Yılmaz"></textarea>
        </label>
        <label>
          Start at
          <input type="text" bind:value={rangeStart} placeholder="0:00" />
        </label>
        <label>
          Stop at
          <input type="text" bind:value={rangeEnd} placeholder="end" />
        </label>
        <label class="inline check">
          <input type="checkbox" bind:checked={job.vad} onchange={() => app.saveSettings()} />
          <span>Skip silence<br /><span class="muted small">prevents invented text in pauses</span></span>
        </label>
      </div>
    {/if}

    <div class="actions">
      {#if files.length > 0 && !modelReady}
        <span class="muted small">Download the selected model first.</span>
      {/if}
      <button class="primary" disabled={!canStart || adding} onclick={enqueue}>
        {files.length > 1 ? `Transcribe ${files.length} files` : "Transcribe"}
      </button>
    </div>
  </section>

  {#if app.jobs.length > 0}
    <section class="queue">
      <div class="queue-head">
        <h2>Queue</h2>
        <button class="ghost small" onclick={removeFinished}>Clear finished</button>
      </div>
      {#each [...app.jobs].reverse() as j (j.id)}
        <div class="card job" class:failed={j.status === "failed"}>
          <div class="job-main">
            <strong>{fileName(j.spec.media)}</strong>
            <span class="small muted">
              {#if j.status === "running" && j.progress}
                {STAGE_LABEL[j.progress.stage]} · {Math.round(j.progress.overall * 100)} % {eta(j) ? `· ${eta(j)}` : ""}
              {:else if j.status === "queued"}
                Waiting
              {:else if j.status === "done"}
                Done{j.elapsed ? ` in ${duration(j.elapsed)}` : ""}
              {:else if j.status === "cancelled"}
                Cancelled
              {:else}
                {j.error}
              {/if}
            </span>
            {#if j.status === "running"}
              <progress value={j.progress?.overall ?? 0} max="1"></progress>
            {/if}
          </div>
          <div class="job-actions">
            {#if j.status === "done"}
              <button class="primary" onclick={() => app.openInEditor(j.spec.output)}>Open</button>
            {/if}
            {#if j.status === "running" || j.status === "queued"}
              <button onclick={() => api.cancelJob(j.id)}>Cancel</button>
            {:else}
              <button class="ghost" aria-label="Remove from list" onclick={async () => { await api.removeJob(j.id); app.jobs = await api.listJobs(); }}>✕</button>
            {/if}
          </div>
        </div>
      {/each}
    </section>
  {/if}
</div>

<style>
  .page {
    height: 100%;
    overflow-y: auto;
    padding: 28px 36px 48px;
    max-width: 920px;
  }

  header {
    margin-bottom: 18px;
  }

  header p {
    margin: 6px 0 0;
  }

  code {
    font-size: 12px;
  }

  .notice {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 14px;
    margin-bottom: 14px;
    border-radius: var(--radius);
    background: var(--accent-soft);
    color: var(--accent-strong);
  }

  .notice.warn {
    background: var(--warn-soft);
    color: var(--warn);
  }

  .form {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .drop {
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 28px;
    border: 2px dashed var(--border);
    background: var(--bg);
  }

  .files ul {
    list-style: none;
    margin: 0 0 10px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .files li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 10px;
    background: var(--bg);
    border-radius: var(--radius);
  }

  .file {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    min-width: 0;
  }

  .file button {
    padding: 2px 0;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 14px 16px;
    align-items: end;
  }

  .wide {
    grid-column: 1 / -1;
  }

  .check {
    align-self: center;
  }

  .toggle {
    align-self: flex-start;
    padding: 4px 0;
    color: var(--muted);
  }

  .advanced {
    padding-top: 4px;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 12px;
  }

  .queue {
    margin-top: 28px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .queue-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .job {
    display: flex;
    gap: 16px;
    align-items: center;
    padding: 12px 16px;
  }

  .job.failed .small {
    color: var(--danger);
  }

  .job-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .job-actions {
    display: flex;
    gap: 6px;
  }
</style>
