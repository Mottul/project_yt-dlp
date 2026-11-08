// Beispieldaten für die Oberfläche im gewöhnlichen Browser (`npm run dev`, nicht
// im Tauri-Fenster). Zeigt jeden Zustand, den der Kern melden kann, und spielt
// einen neuen Auftrag durch alle Phasen – zum Ansehen, nicht zum Laden.
// Die Texte der Phasen stammen sonst aus dem Kern (`Phase::label`); hier stehen
// sie gleichlautend, damit die Ansicht stimmt.

import type { UnlistenFn } from '@tauri-apps/api/event'
import type { Api, EnqueueRequest, Job, Settings, ToolsStatus } from './api'

const DIR = 'C:\\Users\\Demo\\Downloads'

const status: ToolsStatus = {
  ytdlpVersion: '2026.10.26',
  ytdlpLatest: '2026.10.26',
  ytdlpUpToDate: true,
  ffmpegVersion: '6.1.1',
  ffmpegReady: true,
  denoVersion: '2.9.8',
  busy: false,
  lastError: null,
  pendingMb: 0,
  // Hinweis auf eine neue App-Version, damit er in der Ansicht vorkommt
  appUpdate: {
    version: '0.1.4',
    current: '0.1.3',
    url: 'https://github.com/Mottul/project_yt-dlp/releases/tag/v0.1.4'
  }
}

let settings: Settings = {
  outputDir: DIR,
  format: 'video',
  maxHeight: 1080,
  autoUpdate: true,
  cookies: null
}

const base = {
  format: 'video' as const,
  progress: 0,
  title: null,
  speed: null,
  eta: null,
  outputDir: DIR,
  outputFile: null,
  error: null,
  note: null,
  phase: null,
  phaseLabel: null
}

const now = Date.now()
let jobs: Job[] = [
  {
    ...base,
    id: 'demo-video',
    url: 'https://www.youtube.com/watch?v=demo1',
    status: 'running',
    progress: 0.23,
    title: 'Naturdoku – Der Wald im Herbst',
    speed: '4,8 MB/s',
    eta: '1:12',
    phase: 'video',
    phaseLabel: 'Video 1/2',
    createdAt: now - 1000
  },
  {
    ...base,
    id: 'demo-prepare',
    url: 'https://www.youtube.com/watch?v=demo8',
    status: 'running',
    phase: 'prepare',
    phaseLabel: 'Abfragen…',
    createdAt: now - 500
  },
  {
    ...base,
    id: 'demo-merge',
    url: 'https://www.arte.tv/de/videos/demo2',
    status: 'running',
    progress: 0.999,
    title: 'Konzertmitschnitt (Live)',
    phase: 'merge',
    phaseLabel: 'Zusammenfügen…',
    createdAt: now - 2000
  },
  { ...base, id: 'demo-queued', url: 'https://www.youtube.com/watch?v=demo3', status: 'queued', createdAt: now - 3000 },
  {
    ...base,
    id: 'demo-done',
    url: 'https://www.youtube.com/watch?v=demo4',
    status: 'done',
    progress: 1,
    title: 'Kurzer Clip',
    outputFile: `${DIR}\\Kurzer Clip.mp4`,
    createdAt: now - 4000
  },
  {
    ...base,
    id: 'demo-existing',
    url: 'https://www.youtube.com/watch?v=demo5',
    status: 'done',
    progress: 1,
    title: 'Trailer',
    outputFile: `${DIR}\\Trailer.mp4`,
    note:
      'War schon im Zielordner – nicht erneut geladen. Gleicher Titel, aber anderes Video? ' +
      'Vorhandene Datei umbenennen und erneut laden.',
    createdAt: now - 5000
  },
  {
    ...base,
    id: 'demo-error',
    url: 'https://www.youtube.com/watch?v=demo6',
    status: 'error',
    progress: 0,
    error:
      'Altersbeschränkt: YouTube verlangt eine Anmeldung. Unter „Anmeldung“ den Browser wählen, ' +
      'in dem du bei YouTube angemeldet bist.',
    createdAt: now - 6000
  },
  {
    ...base,
    id: 'demo-canceled',
    url: 'https://www.youtube.com/watch?v=demo7',
    status: 'canceled',
    progress: 0.41,
    title: 'Abgebrochener Download',
    createdAt: now - 7000
  }
]

const jobListeners = new Set<(job: Job) => void>()
const statusListeners = new Set<(s: ToolsStatus) => void>()
const timers = new Map<string, number>()

function emit(job: Job): void {
  jobs = jobs.map((j) => (j.id === job.id ? job : j))
  jobListeners.forEach((cb) => cb({ ...job }))
}

const wait = <T>(ms: number, value: T): Promise<T> =>
  new Promise((resolve) => window.setTimeout(() => resolve(value), ms))

/** Spielt einen Auftrag durch: Video, Ton, Zusammenfügen bzw. Umwandeln, fertig. */
function simulate(id: string, req: EnqueueRequest): void {
  const audio = req.format !== 'video'
  const prepare = { phase: 'prepare' as const, phaseLabel: 'Abfragen…' }
  const steps: Array<Partial<Job>> = [prepare, prepare, prepare]
  for (let p = 0; p <= 100; p += 10) {
    steps.push(
      audio
        ? { progress: p / 100 * 0.999, phase: 'audio', phaseLabel: 'Ton', speed: '1,2 MB/s', eta: `0:${String(10 - p / 10).padStart(2, '0')}` }
        : { progress: (p / 100) * 0.5, phase: 'video', phaseLabel: 'Video 1/2', speed: '6,1 MB/s', eta: `0:${String(20 - p / 5).padStart(2, '0')}` }
    )
  }
  if (!audio) {
    for (let p = 0; p <= 100; p += 25) {
      steps.push({ progress: 0.5 + (p / 100) * 0.499, phase: 'audio', phaseLabel: 'Ton 2/2', speed: '2,3 MB/s', eta: '0:02' })
    }
  }
  const post = audio ? { phase: 'convert' as const, phaseLabel: 'Umwandeln…' } : { phase: 'merge' as const, phaseLabel: 'Zusammenfügen…' }
  steps.push({ ...post, progress: 0.999, speed: null, eta: null }, { ...post })
  const ext = req.format === 'audio-mp3' ? 'mp3' : req.format === 'audio-m4a' ? 'm4a' : 'mp4'
  steps.push({
    status: 'done',
    progress: 1,
    phase: null,
    phaseLabel: null,
    speed: null,
    eta: null,
    outputFile: `${req.outputDir}\\Neues Video.${ext}`
  })

  let i = 0
  const timer = window.setInterval(() => {
    const job = jobs.find((j) => j.id === id)
    if (!job || job.status === 'canceled' || i >= steps.length) {
      window.clearInterval(timer)
      timers.delete(id)
      return
    }
    const step = steps[i++]
    // Den Titel gibt es erst nach der Abfrage
    const title = step.phase === 'prepare' ? null : 'Neues Video'
    emit({ ...job, status: 'running', title, ...step })
  }, 600)
  timers.set(id, timer)
}

export const demoApi: Api = {
  // Verzögert, damit „Prüfe Werkzeuge…“ zu sehen ist
  toolsStatus: () => wait(1200, { ...status }),
  ensureTools: async () => {
    statusListeners.forEach((cb) => cb({ ...status, busy: true }))
    await wait(1500, null)
    statusListeners.forEach((cb) => cb({ ...status }))
    return { ...status }
  },
  enqueue: async (req: EnqueueRequest) => {
    // Vereinfacht wie `split_addresses` im Kern: ein Wort bleibt, sonst nur Webadressen
    const words = req.url.split(/\s+/).filter(Boolean)
    const urls = words.length === 1 ? words : words.filter((w) => /^(https?:\/\/|www\.)/i.test(w))
    if (urls.length === 0) throw new Error('Keine Adresse gefunden')
    return [...new Set(urls)].map((url, i) => {
      const id = `demo-${Date.now()}-${i}`
      const job: Job = { ...base, id, url, format: req.format, status: 'queued', createdAt: Date.now() + i }
      jobs = [job, ...jobs]
      jobListeners.forEach((cb) => cb({ ...job }))
      simulate(id, { ...req, url })
      return id
    })
  },
  listJobs: async () => jobs.map((j) => ({ ...j })),
  cancelJob: async (id: string) => {
    const job = jobs.find((j) => j.id === id)
    if (job && (job.status === 'queued' || job.status === 'running')) {
      emit({ ...job, status: 'canceled', speed: null, eta: null, phase: null, phaseLabel: null })
    }
  },
  retryJob: async (id: string) => {
    const job = jobs.find((j) => j.id === id)
    if (!job || (job.status !== 'error' && job.status !== 'canceled')) {
      throw new Error('Dieser Auftrag lässt sich gerade nicht neu starten – gleich noch einmal versuchen.')
    }
    emit({ ...job, status: 'queued', progress: 0, error: null, note: null, outputFile: null, phase: null, phaseLabel: null })
    simulate(id, { url: job.url, format: job.format, maxHeight: null, outputDir: job.outputDir, cookies: null })
  },
  clearFinished: async () => {
    jobs = jobs.filter((j) => j.status === 'queued' || j.status === 'running')
  },
  loadSettings: async () => ({ ...settings }),
  saveSettings: async (next: Settings) => {
    settings = { ...next }
  },
  binDir: async () => 'C:\\Users\\Demo\\AppData\\Roaming\\com.mottul.videoloader\\bin',

  onJob: async (cb: (job: Job) => void): Promise<UnlistenFn> => {
    jobListeners.add(cb)
    return () => jobListeners.delete(cb)
  },
  onStatus: async (cb: (s: ToolsStatus) => void): Promise<UnlistenFn> => {
    statusListeners.add(cb)
    return () => statusListeners.delete(cb)
  }
}
