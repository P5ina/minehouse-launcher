import { useEffect, useState } from "react"
import { relaunch } from "@tauri-apps/plugin-process"
import { check, type Update } from "@tauri-apps/plugin-updater"
import { Download } from "lucide-react"

import { Button } from "@/components/ui/button"

// Полоса сверху, если на GitHub вышла новая версия лаунчера.
// enabled=false во Flatpak: там лаунчер обновляет Flathub.
export function UpdateBanner({ enabled }: { enabled: boolean }) {
  const [update, setUpdate] = useState<Update | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (!enabled) return
    check()
      .then(setUpdate)
      .catch((e) => console.warn("update check failed", e))
  }, [enabled])

  if (!update) return null
  const install = async () => {
    setBusy(true)
    await update.downloadAndInstall()
    await relaunch()
  }
  return (
    <div className="relative z-20 flex items-center justify-center gap-3 border-b bg-muted px-4 py-1.5 text-sm">
      <span>{busy ? "Скачиваем обновление…" : `Доступна версия ${update.version}`}</span>
      <Button size="xs" disabled={busy} onClick={install}>
        <Download />
        Обновить
      </Button>
    </div>
  )
}
