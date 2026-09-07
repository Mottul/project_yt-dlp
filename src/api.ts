// Typisierte Brücke zum Rust-Teil. Die Oberfläche ruft nichts direkt auf.

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type Format = 'video' | 'audio-mp3' | 'audio-m4a'
export type JobStatus = 'queued' | 'running' | 'done' | 'error' | 'canceled'

export interface Job {
  id: string
  url: string
  format: Format
  status: JobStatus
  progress: number
  title: string | null
  speed: string | null
  eta: string | null
  outputDir: string
  outputFile: string | null
  error: string | null
  createdAt: number
}

export interface ToolsStatus {
  ytdlpVersion: string | null
  ytdlpLatest: string | null
  ytdlpUpToDate: boolean | null
  ffmpegVersion: string | null
  ffmpegReady: boolean
  busy: boolean
  lastError: string | null
  pendingMb: number
}

export interface Settings {
  outputDir: string
  format: Format
  maxHeight: number | null
  autoUpdate: boolean
}

export const api = {
  toolsStatus: () => invoke<ToolsStatus>('tools_status'),
  ensureTools: () => invoke<ToolsStatus>('ensure_tools'),
  enqueue: (req: { url: string; format: Format; maxHeight: number | null; outputDir: string }) =>
    invoke<string>('enqueue', { req }),
  listJobs: () => invoke<Job[]>('list_jobs'),
  cancelJob: (id: string) => invoke<void>('cancel_job', { id }),
  clearFinished: () => invoke<void>('clear_finished'),
  loadSettings: () => invoke<Settings>('load_settings'),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  binDir: () => invoke<string>('bin_dir'),

  onJob: (cb: (job: Job) => void): Promise<UnlistenFn> =>
    listen<Job>('job-update', (e) => cb(e.payload)),
  onStatus: (cb: (status: ToolsStatus) => void): Promise<UnlistenFn> =>
    listen<ToolsStatus>('tools-status', (e) => cb(e.payload))
}
