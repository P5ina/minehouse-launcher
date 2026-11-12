import { useState } from "react"
import { FolderInput, RotateCcw } from "lucide-react"

import { Button } from "@/components/ui/button"
import { invoke, listen, type ProgressEvent } from "@/lib/api"

/** Открывает выбор папки; вернёт, куда на самом деле лягут файлы, или null. */
export function pickDataDir() {
  return invoke<string | null>("data_dir_pick")
}

// Перенос Java, Minecraft и сборок в другую папку; шаги приходят событиями progress.
export function useMoveDataDir() {
  const [moving, setMoving] = useState(false)
  const [stage, setStage] = useState("")
  const [error, setError] = useState("")

  const move = async (path: string) => {
    setMoving(true)
    setError("")
    setStage("Готовим папку")
    const off = await listen<ProgressEvent>("progress", ({ payload }) => {
      if (payload.kind === "stage") setStage(payload.name)
    })
    try {
      await invoke<string>("data_dir_set", { path })
      return true
    } catch (e) {
      setError(String(e))
      return false
    } finally {
      off()
      setMoving(false)
      setStage("")
    }
  }

  return { moving, stage, error, move }
}

// Путь папки игры с кнопками «Изменить» и «Как было».
export function DataDirField({
  path,
  defaultPath,
  disabled,
  onPick,
}: {
  path: string
  defaultPath: string
  disabled?: boolean
  onPick: (path: string) => void
}) {
  const pick = async () => {
    const p = await pickDataDir()
    if (p && p !== path) onPick(p)
  }
  return (
    <div className="grid gap-2">
      <div
        className="truncate rounded-md border bg-muted/40 px-3 py-2 font-mono text-xs select-text"
        title={path}
        dir="rtl"
      >
        <bdi>{path}</bdi>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" disabled={disabled} onClick={pick}>
          <FolderInput />
          Выбрать папку…
        </Button>
        {path !== defaultPath && (
          <Button variant="ghost" size="sm" disabled={disabled} onClick={() => onPick(defaultPath)}>
            <RotateCcw />
            Папка по умолчанию
          </Button>
        )}
      </div>
    </div>
  )
}
