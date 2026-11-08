import { useEffect, useRef, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import {
  api,
  isPostPhase,
  type CookieBrowser,
  type Format,
  type Job,
  type Settings,
  type ToolsStatus
} from './api'

const RESOLUTIONS: Array<{ value: number | null; label: string }> = [
  { value: null, label: 'Beste' },
  { value: 2160, label: '2160p (4K)' },
  { value: 1440, label: '1440p' },
  { value: 1080, label: '1080p' },
  { value: 720, label: '720p' },
  { value: 480, label: '480p' }
]

/** Safari liest yt-dlp nur auf dem Mac */
const IS_MAC = navigator.userAgent.includes('Mac')

const COOKIE_BROWSERS: Array<{ value: CookieBrowser; label: string }> = [
  { value: 'firefox', label: 'Firefox' },
  { value: 'chrome', label: 'Chrome' },
  { value: 'edge', label: 'Edge' },
  { value: 'brave', label: 'Brave' },
  { value: 'opera', label: 'Opera' },
  { value: 'vivaldi', label: 'Vivaldi' },
  { value: 'chromium', label: 'Chromium' },
  ...(IS_MAC ? [{ value: 'safari' as const, label: 'Safari' }] : [])
]

const COOKIE_HINT =
  'Für altersbeschränkte oder nur angemeldet sichtbare Videos: yt-dlp nimmt die Anmeldung ' +
  'des Browsers (dort bei der Seite angemeldet sein). Die Cookies liest yt-dlp bei jedem ' +
  'Abruf selbst – der MottulVideoLoader speichert sie nicht. Unter Windows sperren Chrome ' +
  'und Edge ihre Cookies für andere Programme: dort Firefox wählen.'

const STATUS_LABEL: Record<Job['status'], string> = {
  queued: 'wartet',
  running: 'lädt',
  done: 'fertig',
  error: 'Fehler',
  canceled: 'abgebrochen'
}

/** Abgeschnitten, nicht gerundet: Der Kern hält bis zum Ende bei 99,9 %. */
const percent = (job: Job): number => (job.status === 'done' ? 100 : Math.floor(job.progress * 100))

export function App(): JSX.Element {
  const [status, setStatus] = useState<ToolsStatus | null>(null)
  const [settings, setSettings] = useState<Settings | null>(null)
  const [jobs, setJobs] = useState<Job[]>([])
  const [url, setUrl] = useState('')
  const [message, setMessage] = useState<string | null>(null)
  const [binPath, setBinPath] = useState('')
  const saveTimer = useRef<number | undefined>(undefined)

  useEffect(() => {
    void api.toolsStatus().then(setStatus)
    void api.loadSettings().then(setSettings)
    void api.listJobs().then(setJobs)
    void api.binDir().then(setBinPath)
    const offJob = api.onJob((job) =>
      setJobs((prev) => {
        const next = prev.filter((j) => j.id !== job.id)
        next.push(job)
        return next.sort((a, b) => b.createdAt - a.createdAt)
      })
    )
    const offStatus = api.onStatus(setStatus)
    return () => {
      void offJob.then((fn) => fn())
      void offStatus.then((fn) => fn())
    }
  }, [])

  /** Einstellungen gebündelt sichern, nicht bei jedem Tastendruck. */
  function update(patch: Partial<Settings>): void {
    setSettings((prev) => {
      if (!prev) return prev
      const next = { ...prev, ...patch }
      window.clearTimeout(saveTimer.current)
      saveTimer.current = window.setTimeout(() => void api.saveSettings(next), 250)
      return next
    })
  }

  async function pickFolder(): Promise<void> {
    const dir = await open({ directory: true, multiple: false, title: 'Zielordner wählen' })
    if (typeof dir === 'string') update({ outputDir: dir })
  }

  async function start(): Promise<void> {
    if (!settings) return
    const trimmed = url.trim()
    if (!trimmed) return
    try {
      await api.enqueue({
        url: trimmed,
        format: settings.format,
        maxHeight: settings.format === 'video' ? settings.maxHeight : null,
        outputDir: settings.outputDir,
        cookies: settings.cookies ?? null
      })
      setUrl('')
      setMessage(null)
    } catch (err) {
      setMessage(String(err))
    }
  }

  const busy = status?.busy ?? false
  const toolsReady = !!status?.ytdlpVersion && !!status?.ffmpegReady
  const ready = toolsReady && !!settings?.outputDir
  const problem = message ?? status?.lastError ?? null

  return (
    <div className="app">
      <header>
        <h1>MottulVideoLoader</h1>
        <span className="sub">Video-Downloader auf Basis von yt-dlp</span>
      </header>

      <section className="card tools">
        <div className="tool-line">
          {status === null ? (
            <>
              <span className="dot spin" aria-hidden="true" />
              <span>Prüfe Werkzeuge…</span>
            </>
          ) : busy ? (
            <>
              <span className="dot spin" aria-hidden="true" />
              <span>
                {status?.ytdlpVersion ? 'Prüfe auf neue Version…' : 'Richte Werkzeuge ein…'}
                {status?.pendingMb ? ` (ca. ${status.pendingMb} MB)` : ''}
              </span>
            </>
          ) : toolsReady ? (
            <>
              <span className={`dot ${status?.ytdlpUpToDate === false ? 'warn' : 'ok'}`} aria-hidden="true" />
              <span>
                yt-dlp <code>{status?.ytdlpVersion}</code>
                {status?.ytdlpUpToDate === true && <span className="tag ok">aktuell</span>}
                {status?.ytdlpUpToDate === false && (
                  <span className="tag warn">neuer verfügbar: {status.ytdlpLatest}</span>
                )}
                {' · ffmpeg '}
                <code>{status?.ffmpegVersion ?? '—'}</code>
                {' · Deno '}
                <code>{status?.denoVersion ?? 'fehlt'}</code>
              </span>
            </>
          ) : (
            <>
              <span className="dot warn" aria-hidden="true" />
              <span>
                Werkzeuge fehlen — beim ersten Start werden yt-dlp, ffmpeg und Deno geladen
                {status?.pendingMb ? ` (ca. ${status.pendingMb} MB)` : ''}.
              </span>
            </>
          )}
          <button className="ghost" disabled={busy || status === null} onClick={() => void api.ensureTools()}>
            {toolsReady || status === null ? 'Prüfen' : 'Jetzt einrichten'}
          </button>
        </div>
        {problem && <p className="note bad">{problem}</p>}
      </section>

      <section className="card">
        <div className="row">
          <input
            className="grow"
            value={url}
            placeholder="Video-Adresse einfügen…"
            spellCheck={false}
            onChange={(e) => setUrl(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && ready) void start()
            }}
          />
          <button className="primary" disabled={!ready || !url.trim()} onClick={() => void start()}>
            Laden
          </button>
        </div>

        <div className="row wrap">
          <label>
            <span>Format</span>
            <select
              value={settings?.format ?? 'video'}
              onChange={(e) => update({ format: e.target.value as Format })}
            >
              <option value="video">Video (MP4)</option>
              <option value="audio-mp3">Nur Ton (MP3)</option>
              <option value="audio-m4a">Nur Ton (M4A)</option>
            </select>
          </label>

          {settings?.format === 'video' && (
            <label>
              <span>Max. Auflösung</span>
              <select
                value={settings?.maxHeight ?? 'best'}
                onChange={(e) =>
                  update({ maxHeight: e.target.value === 'best' ? null : Number(e.target.value) })
                }
              >
                {RESOLUTIONS.map((r) => (
                  <option key={r.label} value={r.value ?? 'best'}>
                    {r.label}
                  </option>
                ))}
              </select>
            </label>
          )}

          <label title={COOKIE_HINT}>
            <span>Anmeldung</span>
            <select
              value={settings?.cookies ?? 'none'}
              onChange={(e) =>
                update({
                  cookies: e.target.value === 'none' ? null : (e.target.value as CookieBrowser)
                })
              }
            >
              <option value="none">Keine</option>
              {COOKIE_BROWSERS.map((b) => (
                <option key={b.value} value={b.value}>
                  Cookies aus {b.label}
                </option>
              ))}
            </select>
          </label>

          <label className="grow">
            <span>Zielordner</span>
            <div className="row">
              <input className="grow" readOnly value={settings?.outputDir ?? ''} placeholder="noch nicht gewählt" />
              <button className="ghost" onClick={() => void pickFolder()}>
                Wählen
              </button>
            </div>
          </label>
        </div>
      </section>

      <section className="card jobs">
        <div className="jobs-head">
          <h2>Warteschlange</h2>
          {jobs.length > 0 && (
            <button className="ghost small" onClick={() => void api.clearFinished().then(() => api.listJobs().then(setJobs))}>
              Erledigte entfernen
            </button>
          )}
        </div>

        {jobs.length === 0 && <p className="empty">Noch nichts in der Warteschlange.</p>}

        {jobs.map((job) => (
          <article key={job.id} className={`job ${job.status}`}>
            <div className="job-head">
              <span className="job-title" title={job.url}>
                {job.title ?? job.url}
              </span>
              <span className={`tag ${job.status}`}>{STATUS_LABEL[job.status]}</span>
            </div>
            <div
              className={`bar${isPostPhase(job.phase) ? ' busy' : ''}`}
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={percent(job)}
              aria-valuetext={job.phaseLabel ?? undefined}
            >
              <span style={{ width: `${percent(job)}%` }} />
            </div>
            <div className="job-foot">
              <span>
                {isPostPhase(job.phase) ? (
                  job.phaseLabel
                ) : (
                  <>
                    {percent(job)} %
                    {job.phaseLabel && job.phase !== 'download' ? ` · ${job.phaseLabel}` : ''}
                    {job.speed ? ` · ${job.speed}` : ''}
                    {job.eta ? ` · Rest ${job.eta}` : ''}
                  </>
                )}
              </span>
              <span className="actions">
                {job.status === 'done' && job.outputFile && (
                  <button className="ghost small" onClick={() => void revealItemInDir(job.outputFile!)}>
                    Im Ordner zeigen
                  </button>
                )}
                {(job.status === 'queued' || job.status === 'running') && (
                  <button className="ghost small" onClick={() => void api.cancelJob(job.id)}>
                    Abbrechen
                  </button>
                )}
              </span>
            </div>
            {job.error && <p className="note bad">{job.error}</p>}
            {job.note && <p className="note">{job.note}</p>}
          </article>
        ))}
      </section>

      <footer>
        <label className="check">
          <input
            type="checkbox"
            checked={settings?.autoUpdate ?? true}
            onChange={(e) => update({ autoUpdate: e.target.checked })}
          />
          Beim Programmstart auf eine neue yt-dlp-Version prüfen
        </label>
        <span className="path" title={binPath}>
          Werkzeuge: {binPath}
        </span>
      </footer>
    </div>
  )
}
