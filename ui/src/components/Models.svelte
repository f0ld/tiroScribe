<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { api, errorText } from "../lib/api";
  import { bytes } from "../lib/format";
  import type { ModelStatus } from "../lib/types";

  const whisper = $derived(app.models.filter((m) => m.kind === "whisper"));
  const helpers = $derived(app.models.filter((m) => m.kind !== "whisper"));
  const busy = $derived(Object.keys(app.downloads).length > 0);

  async function download(m: ModelStatus) {
    try {
      app.downloads[m.id] = { done: 0, total: m.size };
      await api.downloadModel(m.id);
    } catch (e) {
      app.notify(errorText(e), true);
    }
  }

  function progressOf(m: ModelStatus) {
    const d = app.downloads[m.id];
    return d ? d.done / d.total : null;
  }
</script>

{#snippet row(m: ModelStatus, selectable: boolean)}
  <div class="row">
    <div class="info">
      <div class="title">
        <strong>{m.name}</strong>
        <span class="small muted">{bytes(m.size)}</span>
        {#if m.installed}<span class="badge ok">installed</span>{/if}
        {#if selectable && m.installed && app.settings.job.model === m.id}<span class="badge">default</span>{/if}
      </div>
      <p class="small muted">{m.description}</p>
      {#if progressOf(m) !== null}
        <progress value={progressOf(m)} max="1"></progress>
      {/if}
    </div>
    <div class="actions">
      {#if progressOf(m) !== null}
        <button onclick={() => api.cancelDownload(m.id)}>Cancel</button>
      {:else if !m.installed}
        <button class="primary" disabled={busy} onclick={() => download(m)}>Download</button>
      {:else if selectable && app.settings.job.model !== m.id}
        <button onclick={() => { app.settings.job.model = m.id; app.saveSettings(); }}>Use by default</button>
      {/if}
    </div>
  </div>
{/snippet}

<div class="page">
  <header>
    <h1>Models</h1>
    <p class="muted">Downloaded once, then everything runs offline. Stored in <code>{app.info?.models_dir}</code>.</p>
  </header>

  <section class="card">
    <h2>Speech recognition</h2>
    <p class="small muted hint">For German interviews, “large-v3 turbo – German” gives the best results. Downloading a model also fetches the helper models below.</p>
    {#each whisper as m (m.id)}
      {@render row(m, true)}
    {/each}
  </section>

  <section class="card">
    <h2>Helper models</h2>
    <p class="small muted hint">Required for silence detection and telling speakers apart.</p>
    {#each helpers as m (m.id)}
      {@render row(m, false)}
    {/each}
  </section>
</div>

<style>
  .page {
    height: 100%;
    overflow-y: auto;
    padding: 28px 36px 48px;
    max-width: 920px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  header p {
    margin: 6px 0 0;
  }

  code {
    font-size: 12px;
  }

  section {
    padding: 18px 20px 6px;
  }

  .hint {
    margin: 4px 0 8px;
  }

  .row {
    display: flex;
    gap: 16px;
    align-items: center;
    padding: 12px 0;
    border-top: 1px solid var(--border);
  }

  .info {
    flex: 1;
    min-width: 0;
  }

  .title {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }

  .info p {
    margin: 3px 0 0;
  }

  progress {
    margin-top: 8px;
  }
</style>
