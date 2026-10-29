// Mirrors the serde types in crates/scribe-core.

export interface Word {
  text: string;
  start: number;
  end: number;
  conf: number;
  overlap?: boolean;
}

export interface Segment {
  speaker: string | null;
  start: number;
  end: number;
  words: Word[];
}

export interface Speaker {
  id: string;
  name: string;
}

export interface Transcript {
  version: number;
  media: { path: string; duration: number; range_start: number };
  language: string;
  engine: { backend: string; model: string; created: number };
  speakers: Speaker[];
  segments: Segment[];
}

export type Device = { kind: "cpu" } | { kind: "gpu"; index: number } | { kind: "npu" };

export type Speakers =
  | { mode: "off" }
  | { mode: "auto"; threshold: number }
  | { mode: "fixed"; count: number };

export interface NpuConfig {
  base_url: string;
  model: string;
}

export interface JobSpec {
  media: string;
  output: string;
  range: { start: number | null; end: number | null };
  language: string | null;
  model: string;
  device: Device;
  quality: "fast" | "precise";
  verbatim: boolean;
  vocabulary: string[];
  speakers: Speakers;
  vad: boolean;
  npu: NpuConfig;
}

export type Stage =
  | "loading"
  | "decoding"
  | "detecting_speech"
  | "transcribing"
  | "diarizing"
  | "finishing";

export interface Progress {
  stage: Stage;
  stage_fraction: number;
  overall: number;
}

export interface JobView {
  id: number;
  spec: JobSpec;
  status: "queued" | "running" | "done" | "failed" | "cancelled";
  progress: Progress | null;
  error: string | null;
  elapsed: number | null;
}

export interface ModelStatus {
  id: string;
  kind: "whisper" | "vad" | "segmentation" | "embedding";
  name: string;
  description: string;
  size: number;
  essential: boolean;
  installed: boolean;
}

export interface GpuInfo {
  index: number;
  name: string;
  memory_mb: number;
}

export interface AppInfo {
  version: string;
  gpus: GpuInfo[];
  npu_present: boolean;
  ffmpeg: boolean;
  models_dir: string;
}

export type ExportFormat = "html" | "txt" | "vtt" | "srt";

export interface ExportOptions {
  timestamps: "none" | "bracket" | "f4";
  pause_min: number | null;
  mark_overlap: boolean;
  header: boolean;
}
