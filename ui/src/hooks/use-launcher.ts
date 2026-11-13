import { useCallback, useEffect, useRef, useState } from "react"

import {
  invoke,
  listen,
  type GameLine,
  type Info,
  type ProgressEvent,
  type ServerStatus,
} from "@/lib/api"

export type ServerState =
  | { state: "loading" }
  | { state: "offline" }
  | ({ state: "online" } & ServerStatus)

const MAX_LOG = 2000

// Строка журнала: шаг установки, обычный текст или строка лога игры.
// instance — из какой сборки (сразу могут играть несколько).
export type LogEntry = (
  | { kind: "stage"; text: string }
  | { kind: "text"; text: string }
  | ({ kind: "game" } & GameLine)
) & { instance?: string }

/** Что сейчас с запуском сборки: шаг и прогресс. */
export type Run = { stage: string; progress: number | null; running: boolean }

// Состояние главного окна: аккаунт, сервер, запуски сборок и журнал.
export function useLauncher() {
  const [info, setInfo] = useState<Info | null>(null)
  const [server, setServer] = useState<ServerState>({ state: "loading" })
  // По сборкам: несколько игр могут идти одновременно.
  const [runs, setRuns] = useState<Record<string, Run>>({})
  const [log, setLog] = useState<LogEntry[]>([])
  const logRef = useRef<LogEntry[]>([])
  const activeRef = useRef<string | null>(null)
  activeRef.current = info?.active_instance ?? null

  const pushEntry = useCallback((entry: LogEntry) => {
    const next = [...logRef.current, entry]
    logRef.current = next.length > MAX_LOG ? next.slice(-1500) : next
    setLog(logRef.current)
  }, [])
  const pushLog = useCallback(
    (text: string, instance?: string) => pushEntry({ kind: "text", text, instance }),
    [pushEntry]
  )

  const patchRun = useCallback((id: string | null | undefined, patch: Partial<Run>) => {
    if (!id) return
    const idle: Run = { stage: "", progress: null, running: false }
    setRuns((r) => ({ ...r, [id]: { ...idle, ...r[id], ...patch } }))
  }, [])

  const refresh = useCallback(async () => {
    setInfo(await invoke<Info>("info"))
  }, [])

  const ping = useCallback(async () => {
    try {
      const st = await invoke<ServerStatus>("server_status")
      setServer({ state: "online", ...st })
    } catch {
      setServer({ state: "offline" })
    }
  }, [])

  useEffect(() => {
    refresh().then(ping)
    const timer = setInterval(ping, 60_000)
    const offProgress = listen<ProgressEvent & { instance?: string }>("progress", ({ payload: e }) => {
      // Без сборки шлёт только перенос папки игры: показываем у выбранной.
      const id = e.instance ?? activeRef.current
      if (e.kind === "stage") {
        patchRun(id, { stage: e.name, progress: null })
        pushEntry({ kind: "stage", text: e.name, instance: e.instance })
      } else if (e.kind === "progress") {
        patchRun(id, { progress: e.total > 0 ? Math.min(100, (e.done / e.total) * 100) : null })
      } else if (e.kind === "log") {
        pushLog(e.line, e.instance)
      } else if (e.kind === "game") {
        pushEntry(e)
      }
    })
    const offGame = listen<{ instance: string; state: string }>("game", ({ payload }) => {
      if (payload.state === "started") patchRun(payload.instance, { progress: 100 })
    })
    return () => {
      clearInterval(timer)
      offProgress.then((f) => f())
      offGame.then((f) => f())
    }
  }, [refresh, ping, pushLog, pushEntry, patchRun])

  // Запускает выбранную сборку; другие можно запускать, пока она играет.
  const play = useCallback(async () => {
    const id = activeRef.current
    if (!id) return
    patchRun(id, { running: true, stage: "Запускаем", progress: null })
    let stage = "Игра закрыта"
    try {
      await invoke("play")
    } catch (e) {
      stage = String(e)
      pushLog(`Ошибка: ${e}`, id)
    } finally {
      patchRun(id, { running: false, progress: null, stage })
      await refresh()
    }
  }, [pushLog, refresh, patchRun])

  const active = info?.active_instance ?? ""
  const current = runs[active]
  const runningIds = Object.keys(runs).filter((id) => runs[id].running)
  return {
    info,
    server,
    /** Выбранная сборка запускается или играет. */
    playing: !!current?.running,
    /** Идёт хоть одна игра. */
    anyPlaying: runningIds.length > 0,
    runningIds,
    stage: current?.stage ?? "",
    progress: current?.progress ?? null,
    log,
    pushLog,
    refresh,
    play,
  }
}
