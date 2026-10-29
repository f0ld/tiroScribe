// Global reactive app state (Svelte 5 runes).

import { api, errorText, onDownloadFinished, onDownloadProgress, onJobUpdated } from "./api";
import type { AppInfo, ExportOptions, JobSpec, JobView, ModelStatus, NpuConfig } from "./types";

export type View = "transcribe" | "editor" | "models" | "settings";

/** Options for new jobs; remembered between sessions. */
export interface JobDefaults {
  language: string; // "auto" or ISO code
  model: string;
  device: JobSpec["device"];
  quality: JobSpec["quality"];
  speakers: JobSpec["speakers"];
  verbatim: boolean;
  vad: boolean;
  vocabulary: string;
}

export interface Settings {
  job: JobDefaults;
  npu: NpuConfig;
  editor: {
    /** Words below this confidence are underlined. */
    lowConfidence: number;
    /** Seconds to jump back when resuming playback (f4-style). */
    rewindOnResume: number;
    followPlayback: boolean;
  };
  export: ExportOptions;
}

const DEFAULT_SETTINGS: Settings = {
  job: {
    language: "de",
    model: "large-v3-turbo-german",
    device: { kind: "gpu", index: 0 },
    quality: "precise",
    speakers: { mode: "fixed", count: 2 },
    verbatim: false,
    vad: true,
    vocabulary: "",
  },
  npu: { base_url: "http://localhost:8000/api/v1", model: "whisper-large-v3-turbo" },
  editor: { lowConfidence: 0.6, rewindOnResume: 1, followPlayback: true },
  export: { timestamps: "bracket", pause_min: 1, mark_overlap: true, header: true },
};

/** Saved settings over defaults, section by section, so new options get their default. */
function withDefaults(saved: unknown): Settings {
  const s = (saved ?? {}) as Partial<Settings>;
  const d = structuredClone(DEFAULT_SETTINGS);
  return {
    job: { ...d.job, ...s.job },
    npu: { ...d.npu, ...s.npu },
    editor: { ...d.editor, ...s.editor },
    export: { ...d.export, ...s.export },
  };
}

class AppState {
  view = $state<View>("transcribe");
  info = $state<AppInfo | null>(null);
  models = $state<ModelStatus[]>([]);
  downloads = $state<Record<string, { done: number; total: number }>>({});
  jobs = $state<JobView[]>([]);
  settings = $state<Settings>(structuredClone(DEFAULT_SETTINGS));
  /** Transcript path the editor should open. */
  editorPath = $state<string | null>(null);
  toast = $state<{ text: string; error: boolean } | null>(null);
  private toastTimer: ReturnType<typeof setTimeout> | undefined;

  async init() {
    const [info, saved, jobs] = await Promise.all([api.appInfo(), api.getSettings(), api.listJobs()]);
    this.info = info;
    this.settings = withDefaults(saved);
    this.jobs = jobs;
    // Fall back to CPU if the remembered GPU is gone.
    const dev = this.settings.job.device;
    if (dev.kind === "gpu" && !info.gpus.some((g) => g.index === dev.index)) {
      this.settings.job.device = { kind: "cpu" };
    }
    await this.refreshModels();
    const initial = await api.initialFile();
    if (initial) this.openInEditor(initial);

    await onJobUpdated((job) => {
      const i = this.jobs.findIndex((j) => j.id === job.id);
      if (i >= 0) this.jobs[i] = job;
      else this.jobs.push(job);
      if (job.status === "done") this.notify(`Finished: ${job.spec.media.split("/").pop()}`);
      if (job.status === "failed") this.notify(job.error ?? "Transcription failed", true);
    });
    await onDownloadProgress((p) => {
      this.downloads[p.id] = { done: p.done, total: p.total };
    });
    await onDownloadFinished(async (p) => {
      for (const id of Object.keys(this.downloads)) delete this.downloads[id];
      if (p.error && p.error !== "cancelled") this.notify(`Download failed: ${p.error}`, true);
      await this.refreshModels();
    });
  }

  async refreshModels() {
    this.models = await api.listModels();
  }

  get installedWhisper() {
    return this.models.filter((m) => m.kind === "whisper" && m.installed);
  }

  /** Helper models (VAD, diarization) that still need downloading. */
  get missingEssentials() {
    return this.models.filter((m) => m.essential && !m.installed);
  }

  async saveSettings() {
    try {
      await api.setSettings($state.snapshot(this.settings));
    } catch (e) {
      this.notify(errorText(e), true);
    }
  }

  notify(text: string, error = false) {
    this.toast = { text, error };
    clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => (this.toast = null), error ? 8000 : 3500);
  }

  openInEditor(path: string) {
    this.editorPath = path;
    this.view = "editor";
  }
}

export const app = new AppState();
