<script lang="ts">
  import { app } from "../lib/app.svelte";
  import { api, errorText } from "../lib/api";

  let probing = $state(false);
  let probeResult = $state<{ ok: boolean; text: string } | null>(null);

  async function testNpu() {
    probing = true;
    probeResult = null;
    try {
      const models = await api.probeNpu($state.snapshot(app.settings.npu));
      probeResult = { ok: true, text: models.length ? `Connected. Models: ${models.join(", ")}` : "Connected, but the server lists no models." };
    } catch (e) {
      probeResult = { ok: false, text: errorText(e) };
    } finally {
      probing = false;
    }
  }
</script>

<div class="page">
  <h1>Settings</h1>

  <section class="card">
    <h2>Editor</h2>
    <div class="grid">
      <label>
        Highlight words with confidence below
        <select bind:value={app.settings.editor.lowConfidence} onchange={() => app.saveSettings()}>
          <option value={0}>never</option>
          <option value={0.4}>40 % (only very unsure)</option>
          <option value={0.6}>60 %</option>
          <option value={0.8}>80 % (more to check)</option>
        </select>
      </label>
      <label>
        Jump back when resuming playback
        <select bind:value={app.settings.editor.rewindOnResume} onchange={() => app.saveSettings()}>
          <option value={0}>off</option>
          <option value={1}>1 second</option>
          <option value={2}>2 seconds</option>
          <option value={3}>3 seconds</option>
        </select>
      </label>
    </div>
  </section>

  <section class="card">
    <h2>NPU (experimental)</h2>
    <p class="small muted">
      liveScribe reaches the NPU through a local transcription server with an OpenAI-compatible API. On AMD Ryzen AI
      (XDNA 2) under Linux this is <strong>Lemonade Server</strong> with the FastFlowLM backend (needs the
      <code>amdxdna</code> driver, kernel 7.0+ and NPU firmware ≥ 1.1). Start the server, load a Whisper model, then
      test the connection here.
    </p>
    <p class="small">
      NPU device:
      {#if app.info?.npu_present}<span class="badge ok">detected (/dev/accel/accel0)</span>{:else}<span class="badge warn">not found</span>{/if}
    </p>
    <div class="grid">
      <label>
        Server URL
        <input type="url" bind:value={app.settings.npu.base_url} onchange={() => app.saveSettings()} />
      </label>
      <label>
        Whisper model name on the server
        <input type="text" bind:value={app.settings.npu.model} onchange={() => app.saveSettings()} />
      </label>
    </div>
    <div class="probe">
      <button onclick={testNpu} disabled={probing}>{probing ? "Testing…" : "Test connection"}</button>
      {#if probeResult}
        <span class="small" class:ok={probeResult.ok} class:bad={!probeResult.ok}>{probeResult.text}</span>
      {/if}
    </div>
  </section>

  <section class="card">
    <h2>System</h2>
    <dl>
      <dt>Version</dt>
      <dd>{app.info?.version}</dd>
      <dt>GPUs (Vulkan)</dt>
      <dd>
        {#each app.info?.gpus ?? [] as g (g.index)}
          <div>{g.name}</div>
        {:else}
          <span class="muted">none</span>
        {/each}
      </dd>
      <dt>ffmpeg</dt>
      <dd>{app.info?.ffmpeg ? "found" : "missing"}</dd>
      <dt>Models folder</dt>
      <dd><code>{app.info?.models_dir}</code></dd>
    </dl>
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

  section {
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  section p {
    margin: 0;
    line-height: 1.5;
  }

  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px 16px;
  }

  .probe {
    display: flex;
    gap: 12px;
    align-items: center;
  }

  .ok {
    color: var(--accent-strong);
  }

  .bad {
    color: var(--danger);
  }

  dl {
    display: grid;
    grid-template-columns: 140px 1fr;
    gap: 6px 12px;
    margin: 0;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
  }

  code {
    font-size: 12px;
  }
</style>
