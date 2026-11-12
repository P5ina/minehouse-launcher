import { useEffect, useState } from "react"

import { invoke, openUrl, type DeviceCode } from "@/lib/api"

// Вход через Microsoft по коду устройства: пока active, получает код,
// открывает страницу входа и ждёт, пока игрок введёт код.
export function useDeviceLogin({
  active,
  configured,
  onDone,
}: {
  active: boolean
  configured: boolean
  onDone: () => void | Promise<void>
}) {
  const [code, setCode] = useState<DeviceCode | null>(null)
  const [error, setError] = useState("")
  const [copied, setCopied] = useState(false)
  const [attempt, setAttempt] = useState(0)

  useEffect(() => {
    if (!active) return
    let alive = true
    setCode(null)
    setError("")
    setCopied(false)
    if (!configured) {
      setError("Вход через Microsoft ещё не настроен в этой сборке лаунчера.")
      return
    }
    ;(async () => {
      let c: DeviceCode
      try {
        c = await invoke<DeviceCode>("login_start")
      } catch (e) {
        if (alive) setError(String(e))
        return
      }
      if (!alive) return
      setCode(c)
      openUrl(c.verification_uri).catch(() => {})
      try {
        await invoke("login_finish", { code: c })
        if (alive) await onDone()
      } catch (e) {
        if (alive) setError(String(e))
      }
    })()
    return () => {
      alive = false
    }
    // onDone меняется на каждом рендере у вызывающих; перезапуск входа из-за него не нужен.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, configured, attempt])

  const copy = () => {
    if (!code) return
    navigator.clipboard.writeText(code.user_code)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  return { code, error, copied, copy, retry: () => setAttempt((n) => n + 1) }
}
