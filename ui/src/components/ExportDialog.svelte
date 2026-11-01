<script lang="ts">
  import { save } from "@tauri-apps/plugin-dialog";
  import { app } from "../lib/app.svelte";
  import { api, errorText } from "../lib/api";
  import { fileName } from "../lib/format";
  import type { ExportFormat, Transcript } from "../lib/types";

  interface Props {
    transcript: Transcript;
    path: string;
    onClose: () => void;
  }

  let { transcript, path, onClose }: Props = $props();

  let format = $state<ExportFormat>("html");
  const opts = app.settings.export;

  const FORMATS: { id: ExportFormat; label: string; hint: string }[] = [
    { id: "html", label: "Document (HTML)", hint: "Opens in Word and LibreOffice" },
    { id: "txt", label: "Plain text", hint: "For MAXQDA, ATLAS.ti, f4analyse" },
    { id: "vtt", label: "WebVTT subtitles", hint: "Video players, EXMARaLDA" },
    { id: "srt", label: "SRT subtitles", hint: "Video editors" },
  ];

  let dialog: HTMLDialogElement;
  $effect(() => dialog.showModal());

  async function doExport() {
    const base = path.replace(/\.scribe\.json$/, "");
    const target = await save({
      defaultPath: `${base}.${format}`,
      filters: [{ name: format.toUpperCase(), extensions: [format] }],
    });
    if (!target) return;
    try {
      await api.exportTranscript(transcript, format, $state.snapshot(opts), target);
      app.saveSettings();
      app.notify(`Exported to ${fileName(target)}`);
      onClose();
    } catch (e) {
      app.notify(errorText(e), true);
    }
  }
</script>

<dialog bind:this={dialog} onclose={onClose} class="card">
  <h2>Export transcript</h2>

  <fieldset>
    {#each FORMATS as f (f.id)}
      <label class="inline option" class:selected={format === f.id}>
        <input type="radio" name="format" value={f.id} bind:group={format} />
        <span><strong>{f.label}</strong><br /><span class="small muted">{f.hint}</span></span>
      </label>
    {/each}
  </fieldset>

  {#if format === "html" || format === "txt"}
    <div class="grid">
      <label>
        Timestamps
        <select bind:value={opts.timestamps}>
          <option value="bracket">[00:01:38] at paragraph start</option>
          <option value="f4">#00:01:38-0# at paragraph end (f4 / MAXQDA)</option>
          <option value="none">none</option>
        </select>
      </label>
      <label>
        Mark pauses
        <select bind:value={opts.pause_min}>
          <option value={null}>no</option>
          <option value={1}>from 1 s – (.) (..) (...)</option>
          <option value={2}>from 2 s</option>
          <option value={3}>from 3 s</option>
        </select>
      </label>
      <label class="inline">
        <input type="checkbox" bind:checked={opts.mark_overlap} />
        Mark overlapping speech //like this//
      </label>
      <label class="inline">
        <input type="checkbox" bind:checked={opts.header} />
        Header with file name and duration
      </label>
    </div>
  {/if}

  <div class="actions">
    <button onclick={() => dialog.close()}>Cancel</button>
    <button class="primary" onclick={doExport}>Export…</button>
  </div>
</dialog>

<style>
  dialog {
    width: min(560px, 90vw);
    padding: 22px;
    color: var(--text);
  }

  dialog::backdrop {
    background: rgb(0 0 0 / 0.35);
  }

  fieldset {
    border: none;
    padding: 0;
    margin: 16px 0;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .option {
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    align-items: flex-start;
    cursor: pointer;
  }

  .option.selected {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px 16px;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }
</style>
