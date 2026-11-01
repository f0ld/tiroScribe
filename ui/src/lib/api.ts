import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppInfo,
  ExportFormat,
  ExportOptions,
  JobSpec,
  JobView,
  ModelStatus,
  NpuConfig,
  Transcript,
} from "./types";

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  initialFile: () => invoke<string | null>("initial_file"),
  listModels: () => invoke<ModelStatus[]>("list_models"),
  downloadModel: (id: string) => invoke<void>("download_model", { id }),
  cancelDownload: (id: string) => invoke<void>("cancel_download", { id }),
  probeNpu: (config: NpuConfig) => invoke<string[]>("probe_npu", { config }),
  enqueueJob: (spec: JobSpec) => invoke<number>("enqueue_job", { spec }),
  listJobs: () => invoke<JobView[]>("list_jobs"),
  cancelJob: (id: number) => invoke<void>("cancel_job", { id }),
  removeJob: (id: number) => invoke<void>("remove_job", { id }),
  defaultOutputPath: (media: string) => invoke<string>("default_output_path", { media }),
  loadTranscript: (path: string) => invoke<Transcript>("load_transcript", { path }),
  saveTranscript: (path: string, transcript: Transcript) =>
    invoke<void>("save_transcript", { path, transcript }),
  exportTranscript: (
    transcript: Transcript,
    format: ExportFormat,
    options: ExportOptions,
    path: string,
  ) => invoke<void>("export_transcript", { transcript, format, options, path }),
  /** Audio for the editor as a blob URL (revoke it when done). */
  async playbackUrl(media: string, force: boolean): Promise<string> {
    const buf = new Uint8Array(await invoke<ArrayBuffer>("prepare_playback", { media, force }));
    const nl = buf.indexOf(10);
    const mime = new TextDecoder().decode(buf.subarray(0, nl));
    return URL.createObjectURL(new Blob([buf.subarray(nl + 1)], { type: mime }));
  },
  playbackCacheSize: () => invoke<number>("playback_cache_size"),
  clearPlaybackCache: () => invoke<void>("clear_playback_cache"),
  getSettings: () => invoke<unknown>("get_settings"),
  setSettings: (settings: unknown) => invoke<void>("set_settings", { settings }),
};

export function onJobUpdated(cb: (job: JobView) => void): Promise<UnlistenFn> {
  return listen<JobView | null>("job-updated", (e) => e.payload && cb(e.payload));
}

export function onDownloadProgress(
  cb: (p: { id: string; done: number; total: number }) => void,
): Promise<UnlistenFn> {
  return listen<{ id: string; done: number; total: number }>("download-progress", (e) =>
    cb(e.payload),
  );
}

export function onDownloadFinished(
  cb: (p: { id: string; error: string | null }) => void,
): Promise<UnlistenFn> {
  return listen<{ id: string; error: string | null }>("download-finished", (e) => cb(e.payload));
}

/** Errors from Tauri commands arrive as plain strings. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
