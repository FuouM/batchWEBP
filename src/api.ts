import { invoke } from "@tauri-apps/api/core";

export type Mode = "lossless" | "nearLossless" | "lossy";

/** Mirrors batchwebp_core::Options. */
export interface Options {
  input: string;
  /** null: in place, each WebP replaces its original. */
  output: string | null;
  encoding: { mode: Mode; effort: number; nearLossless: number; quality: number };
  recursive: boolean;
  keepSmallerOriginal: boolean;
  overwrite: boolean;
  threads: number;
}

export interface Snapshot {
  scanning: boolean;
  found: number;
  done: number;
  converted: number;
  kept: number;
  skipped: number;
  failed: number;
  bytesIn: number;
  bytesOut: number;
}

export interface FileError {
  path: string;
  error: string;
}

export interface Status {
  running: boolean;
  cancelled: boolean;
  elapsedMs: number;
  progress: Snapshot;
  errors: FileError[];
  error: string | null;
}

export const api = {
  start: (options: Options) => invoke<void>("start", { options }),
  cancel: () => invoke<void>("cancel"),
  status: (errorsFrom: number) => invoke<Status | null>("status", { errorsFrom }),
  defaultOutput: (input: string) => invoke<string>("default_output", { input }),
  folderOf: (path: string) => invoke<string>("folder_of", { path }),
};
