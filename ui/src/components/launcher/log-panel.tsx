import { useEffect, useMemo, useRef, useState } from "react"
import { Check, Copy, Search } from "lucide-react"

import type { LogEntry } from "@/hooks/use-launcher"
import { cn } from "@/lib/utils"

type Filter = "all" | "warn" | "error"

const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "Все" },
  { id: "warn", label: "Предупреждения" },
  { id: "error", label: "Ошибки" },
]

const LEVEL_STYLE: Record<string, string> = {
  FATAL: "bg-red-500/20 text-red-300",
  ERROR: "bg-red-500/15 text-red-400",
  WARN: "bg-amber-500/15 text-amber-400",
  INFO: "bg-sky-500/10 text-sky-400",
  DEBUG: "bg-zinc-500/15 text-zinc-400",
  TRACE: "bg-zinc-500/10 text-zinc-500",
}

const RANK: Record<string, number> = { TRACE: 0, DEBUG: 1, INFO: 2, WARN: 3, ERROR: 4, FATAL: 5 }

function rank(e: LogEntry): number {
  if (e.kind === "game") return RANK[e.level] ?? 2
  if (e.kind === "text" && /^(Ошибка|Exception|Caused by|\tat |\s+at )/.test(e.text)) return 4
  return 2
}

function time(ms: number) {
  return new Date(ms).toLocaleTimeString("ru-RU", { hour12: false })
}

// Короткое имя логгера: net.minecraft.client.Minecraft → Minecraft.
function shortLogger(name: string) {
  const i = name.lastIndexOf(".")
  return i >= 0 ? name.slice(i + 1) : name
}

function asText(e: LogEntry) {
  if (e.kind === "game") return `[${time(e.time)}] [${e.thread}/${e.level}] [${e.logger}]: ${e.message}`
  if (e.kind === "stage") return `== ${e.text}`
  return e.text
}

// Метка сборки у строки, когда в журнале их несколько.
function Tag({ name }: { name?: string }) {
  if (!name) return null
  return (
    <span className="mr-2 inline-block max-w-28 truncate rounded bg-secondary px-1 align-bottom text-[10px] leading-4 text-secondary-foreground">
      {name}
    </span>
  )
}

function Row({ e, tag }: { e: LogEntry; tag?: string }) {
  if (e.kind === "stage") {
    return (
      <div className="mt-2 font-medium text-foreground first:mt-0">
        ▸ <Tag name={tag} />
        {e.text}
      </div>
    )
  }
  if (e.kind === "text") {
    const err = rank(e) >= 4
    return (
      <div className={cn("whitespace-pre-wrap", err ? "text-red-400" : "text-muted-foreground")}>
        <Tag name={tag} />
        {e.text}
      </div>
    )
  }
  const bad = (RANK[e.level] ?? 2) >= 4
  const warn = e.level === "WARN"
  return (
    <div className="flex gap-2 py-px">
      <span className="shrink-0 text-zinc-500 tabular-nums">{time(e.time)}</span>
      {tag && (
        <span className="w-20 shrink-0">
          <Tag name={tag} />
        </span>
      )}
      <span
        className={cn(
          "h-fit w-12 shrink-0 rounded px-1 text-center text-[10px] leading-4 font-semibold",
          LEVEL_STYLE[e.level] ?? LEVEL_STYLE.INFO,
        )}
      >
        {e.level}
      </span>
      <span className="w-32 shrink-0 truncate text-violet-400/90" title={`${e.logger} · ${e.thread}`}>
        {shortLogger(e.logger) || e.thread}
      </span>
      <span
        className={cn(
          "min-w-0 flex-1 break-words whitespace-pre-wrap",
          bad ? "text-red-300" : warn ? "text-amber-200" : "text-foreground/85",
        )}
      >
        {e.message}
      </span>
    </div>
  )
}

// Журнал установки и игры. names: id сборки → название, для меток и фильтра.
export function LogPanel({ lines, names }: { lines: LogEntry[]; names: Record<string, string> }) {
  const ref = useRef<HTMLDivElement>(null)
  const stick = useRef(true)
  const [filter, setFilter] = useState<Filter>("all")
  const [query, setQuery] = useState("")
  const [copied, setCopied] = useState(false)
  const [only, setOnly] = useState<string | null>(null)
  // Сборки, которые есть в журнале: фильтр и метки нужны, только если их больше одной.
  const seen = useMemo(() => [...new Set(lines.map((e) => e.instance).filter((i): i is string => !!i))], [lines])
  const multi = seen.length > 1
  const tagOf = (e: LogEntry) => (multi && !only && e.instance ? (names[e.instance] ?? e.instance) : undefined)

  const shown = useMemo(() => {
    const min = filter === "error" ? 4 : filter === "warn" ? 3 : 0
    const q = query.trim().toLowerCase()
    return lines.filter((e) => {
      if (only && e.instance && e.instance !== only) return false
      if (e.kind === "stage") return min === 0 && !q
      if (rank(e) < min) return false
      return !q || asText(e).toLowerCase().includes(q)
    })
  }, [lines, filter, query, only])

  // Прокручиваем вниз, только если пользователь не отмотал журнал вверх.
  useEffect(() => {
    const el = ref.current
    if (el && stick.current) el.scrollTop = el.scrollHeight
  }, [shown])

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(shown.map(asText).join("\n"))
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      /* буфер обмена недоступен */
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 px-4 pb-4">
      <div className="flex items-center gap-2">
        <div className="flex rounded-lg border bg-card/80 p-0.5 text-xs backdrop-blur">
          {FILTERS.map((f) => (
            <button
              key={f.id}
              onClick={() => setFilter(f.id)}
              className={cn(
                "rounded-md px-2.5 py-1 transition-colors",
                filter === f.id ? "bg-secondary text-foreground" : "text-muted-foreground hover:text-foreground",
              )}
            >
              {f.label}
            </button>
          ))}
        </div>
        {multi && (
          <select
            value={only ?? ""}
            onChange={(ev) => setOnly(ev.target.value || null)}
            className="h-7 max-w-40 rounded-lg border bg-card/80 px-2 text-xs backdrop-blur outline-none"
            aria-label="Сборка"
          >
            <option value="">Все сборки</option>
            {seen.map((id) => (
              <option key={id} value={id}>
                {names[id] ?? id}
              </option>
            ))}
          </select>
        )}
        <label className="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-lg border bg-card/80 px-2 text-xs backdrop-blur">
          <Search className="size-3.5 shrink-0 text-muted-foreground" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Поиск по журналу"
            className="min-w-0 flex-1 bg-transparent outline-none placeholder:text-muted-foreground"
          />
        </label>
        <button
          onClick={copy}
          disabled={!shown.length}
          title="Скопировать журнал"
          className="flex h-7 items-center gap-1.5 rounded-lg border bg-card/80 px-2.5 text-xs text-muted-foreground backdrop-blur transition-colors hover:text-foreground disabled:opacity-50"
        >
          {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
          {copied ? "Скопировано" : "Копировать"}
        </button>
      </div>
      <div
        ref={ref}
        onScroll={(ev) => {
          const el = ev.currentTarget
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40
        }}
        className="min-h-0 flex-1 overflow-auto rounded-xl border bg-card/80 backdrop-blur select-text"
      >
        <div className="p-3 font-mono text-xs leading-relaxed">
          {shown.length ? (
            shown.map((e, i) => <Row key={i} e={e} tag={tagOf(e)} />)
          ) : (
            <div className="text-muted-foreground">
              {lines.length
                ? "Ничего не найдено."
                : "Журнал пуст. Здесь появятся шаги установки и вывод игры."}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
