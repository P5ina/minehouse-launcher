import { useEffect, useRef, useState } from "react"
import { getCurrentWebview } from "@tauri-apps/api/webview"
import { FileUp } from "lucide-react"

// .jar или .zip можно перетащить в любое место окна: откроются «Моды» или
// «Паки» (ресурспаки и шейдеры), и файлы добавятся.
export function DropOverlay({ onDrop }: { onDrop: (jars: string[], zips: string[]) => void }) {
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
        const zips = payload.paths.filter((p) => p.toLowerCase().endsWith(".zip"))
        if (jars.length || zips.length) dropRef.current(jars, zips)
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
      <span className="text-lg font-semibold">Отпустите, чтобы добавить моды, ресурспаки или шейдеры</span>
    </div>
  )
}
