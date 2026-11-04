import { useEffect, useRef, useState } from "react"
import { getCurrentWebview } from "@tauri-apps/api/webview"
import { FileUp } from "lucide-react"

// .jar можно перетащить в любое место окна: откроется «Моды» и файлы добавятся.
export function DropOverlay({ onDrop }: { onDrop: (paths: string[]) => void }) {
  const [over, setOver] = useState(false)
  const dropRef = useRef(onDrop)
  dropRef.current = onDrop
  useEffect(() => {
    const off = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "enter" || payload.type === "over") setOver(true)
      else if (payload.type === "leave") setOver(false)
      else if (payload.type === "drop") {
        setOver(false)
        const jars = payload.paths.filter((p) => p.toLowerCase().endsWith(".jar"))
        if (jars.length) dropRef.current(jars)
      }
    })
    return () => {
      off.then((f) => f())
    }
  }, [])
  if (!over) return null
  return (
    <div className="pointer-events-none fixed inset-3 z-[100] flex flex-col items-center justify-center gap-3 rounded-2xl border-2 border-dashed border-foreground/40 bg-background/85 backdrop-blur animate-in fade-in-0">
      <FileUp className="size-8 text-muted-foreground" />
      <span className="text-lg font-semibold">Отпустите, чтобы добавить моды</span>
    </div>
  )
}
