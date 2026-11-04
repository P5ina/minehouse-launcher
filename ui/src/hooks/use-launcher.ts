import { useCallback, useEffect, useRef, useState } from "react"

import {
  invoke,
  listen,
  type Info,
  type ProgressEvent,
  type ServerStatus,
} from "@/lib/api"

export type ServerState =
  | { state: "loading" }
  | { state: "offline" }
  | ({ state: "online" } & ServerStatus)

const MAX_LOG = 2000

// Состояние главного окна: аккаунт, сервер, установка и журнал.
export function useLauncher() {
  const [info, setInfo] = useState<Info | null>(null)
  const [server, setServer] = useState<ServerState>({ state: "loading" })
  const [playing, setPlaying] = useState(false)
  const [stage, setStage] = useState("")
  const [progress, setProgress] = useState<number | null>(null)
  const [log, setLog] = useState<string[]>([])
  const logRef = useRef<string[]>([])

  const pushLog = useCallback((line: string) => {
    const next = [...logRef.current, line]
    logRef.current = next.length > MAX_LOG ? next.slice(-1500) : next
    setLog(logRef.current)
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
    const offProgress = listen<ProgressEvent>("progress", ({ payload: e }) => {
      if (e.kind === "stage") {
        setStage(e.name)
        setProgress(null)
        pushLog(`== ${e.name}`)
      } else if (e.kind === "progress") {
        setProgress(e.total > 0 ? Math.min(100, (e.done / e.total) * 100) : null)
      } else if (e.kind === "log") {
        pushLog(e.line)
      }
    })
    const offGame = listen<string>("game", ({ payload }) => {
      if (payload === "started") setProgress(100)
    })
    return () => {
      clearInterval(timer)
      offProgress.then((f) => f())
      offGame.then((f) => f())
    }
  }, [refresh, ping, pushLog])

  const play = useCallback(async () => {
    setPlaying(true)
    try {
      await invoke("play")
      setStage("Игра закрыта")
    } catch (e) {
      setStage(String(e))
      pushLog(`Ошибка: ${e}`)
    } finally {
      setPlaying(false)
      setProgress(null)
      await refresh()
    }
  }, [pushLog, refresh])

  return { info, server, playing, stage, progress, log, pushLog, refresh, play }
}
