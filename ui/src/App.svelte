<script lang="ts">
  import { onMount } from "svelte";
  import { app, type View } from "./lib/app.svelte";
  import { errorText } from "./lib/api";
  import Transcribe from "./components/Transcribe.svelte";
  import Editor from "./components/Editor.svelte";
  import Models from "./components/Models.svelte";
  import Settings from "./components/Settings.svelte";

  let ready = $state(false);
  let initError = $state<string | null>(null);

  onMount(async () => {
    try {
      await app.init();
      ready = true;
    } catch (e) {
      initError = errorText(e);
    }
  });

  const nav: { view: View; label: string; icon: string }[] = [
    { view: "transcribe", label: "Transcribe", icon: "M12 3v12m0 0a4 4 0 0 0 4-4V7a4 4 0 0 0-8 0v4a4 4 0 0 0 4 4Zm-7-4a7 7 0 0 0 14 0M12 18v3" },
    { view: "editor", label: "Editor", icon: "M4 20h4L19 9l-4-4L4 16v4Zm9-13 4 4" },
    { view: "models", label: "Models", icon: "M12 3 3 8l9 5 9-5-9-5Zm-9 9 9 5 9-5M3 16l9 5 9-5" },
    { view: "settings", label: "Settings", icon: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Zm7.4-3a7.4 7.4 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7.5 7.5 0 0 0-2-1.2L14.5 3h-4l-.4 2.6a7.5 7.5 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6a7.4 7.4 0 0 0 0 2.4l-2 1.6 2 3.4 2.4-1a7.5 7.5 0 0 0 2 1.2l.4 2.6h4l.4-2.6a7.5 7.5 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6c.1-.4.1-.8.1-1.2Z" },
  ];

  const running = $derived(app.jobs.filter((j) => j.status === "running" || j.status === "queued").length);
</script>

<div class="shell">
  <nav>
    <div class="brand">
      <img src="/icon.svg" alt="" width="28" height="28" />
      <span>tiroScribe</span>
    </div>
    {#each nav as item (item.view)}
      <button class="ghost" class:active={app.view === item.view} onclick={() => (app.view = item.view)}>
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={item.icon} /></svg>
        {item.label}
        {#if item.view === "transcribe" && running > 0}<span class="count">{running}</span>{/if}
      </button>
    {/each}
    <div class="grow"></div>
    <p class="small muted footer">100 % local · no cloud</p>
  </nav>

  <main>
    {#if initError}
      <div class="center"><p class="error">Could not start: {initError}</p></div>
    {:else if !ready}
      <div class="center muted">Loading…</div>
    {:else if app.view === "transcribe"}
      <Transcribe />
    {:else if app.view === "editor"}
      <Editor />
    {:else if app.view === "models"}
      <Models />
    {:else}
      <Settings />
    {/if}
  </main>

  {#if app.toast}
    <div class="toast" class:error={app.toast.error} role="status">{app.toast.text}</div>
  {/if}
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 188px 1fr;
    height: 100%;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 14px 10px;
    background: var(--surface);
    border-right: 1px solid var(--border);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 8px 18px;
    font-weight: 700;
    font-size: 16px;
    letter-spacing: -0.01em;
  }

  nav button {
    display: flex;
    align-items: center;
    gap: 10px;
    text-align: left;
    padding: 8px 10px;
    color: var(--muted);
    font-weight: 500;
  }

  nav button.active {
    background: var(--accent-soft);
    color: var(--accent-strong);
  }

  .count {
    margin-left: auto;
    font-size: 11px;
    background: var(--accent);
    color: var(--surface);
    border-radius: 999px;
    padding: 0 7px;
  }

  .grow {
    flex: 1;
  }

  .footer {
    padding: 0 10px;
    margin: 0;
  }

  main {
    overflow: hidden;
    min-width: 0;
  }

  .center {
    display: grid;
    place-items: center;
    height: 100%;
  }

  .error {
    color: var(--danger);
  }

  .toast {
    position: fixed;
    bottom: 20px;
    right: 20px;
    max-width: 420px;
    padding: 10px 16px;
    border-radius: var(--radius);
    background: var(--text);
    color: var(--bg);
    box-shadow: var(--shadow);
    z-index: 100;
  }

  .toast.error {
    background: var(--danger);
    color: #fff;
  }
</style>
