import { useEffect, useRef } from "react"

// Журнал установки и игры.
export function LogPanel({ lines }: { lines: string[] }) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = ref.current
    if (el) el.scrollTop = el.scrollHeight
  }, [lines])
  return (
    <div className="min-h-0 flex-1 px-4 pb-4">
      <div
        ref={ref}
        className="h-full overflow-auto rounded-xl border bg-card/80 backdrop-blur select-text"
      >
        <pre className="p-3 font-mono text-xs leading-relaxed break-all whitespace-pre-wrap text-muted-foreground">
          {lines.length ? lines.join("\n") : "Журнал пуст. Здесь появятся шаги установки и вывод игры."}
        </pre>
      </div>
    </div>
  )
}
