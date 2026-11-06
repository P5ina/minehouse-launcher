import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  ArrowDown,
  ArrowUp,
  Check,
  Download,
  ExternalLink,
  FolderOpen,
  Loader2,
  Plus,
  RefreshCw,
  Search as SearchIcon,
  Trash2,
} from "lucide-react"

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  downloadsLabel,
  invoke,
  openUrl,
  type ContentPack,
  type PackHit,
  type PackKind,
  type PacksAdded,
  type PackSearch,
  type PacksListing,
} from "@/lib/api"

import { ModIcon, Row } from "./mods-dialog"

type Mode = "mine" | "find"
type Msg = { text: string; error: boolean } | null

const TEXT: Record<
  PackKind,
  { one: string; many: string; empty: string; search: string; hint: string; modrinth: string }
> = {
  resource: {
    one: "ресурспак",
    many: "Ресурспаки",
    empty: "Ресурспаков пока нет. Найдите их на Modrinth или добавьте .zip.",
    search: "Поиск ресурспаков",
    hint:
      "Можно включить несколько: верхний перекрывает нижние, порядок меняется стрелками. Ресурспак сервера MineHouse игра подключает сама поверх всех.",
    modrinth: "resourcepack",
  },
  shader: {
    one: "шейдер",
    many: "Шейдеры",
    empty: "Шейдеров пока нет. Найдите их на Modrinth или добавьте .zip.",
    search: "Поиск шейдеров для Iris",
    hint:
      "Включён может быть только один шейдер. В игре их можно переключать в настройках графики. Шейдеры сильно нагружают видеокарту.",
    modrinth: "shader",
  },
}

export function PacksDialog({
  open,
  kind,
  dropped,
  onKindChange,
  onOpenChange,
}: {
  open: boolean
  kind: PackKind
  dropped: string[]
  onKindChange: (kind: PackKind) => void
  onOpenChange: (open: boolean) => void
}) {
  const t = TEXT[kind]
  const [mode, setMode] = useState<Mode>("mine")
  const [listing, setListing] = useState<PacksListing | null>(null)
  const [msg, setMsg] = useState<Msg>(null)
  const [busy, setBusy] = useState(false)
  const [removing, setRemoving] = useState<ContentPack | null>(null)

  const [query, setQuery] = useState("")
  const [hits, setHits] = useState<PackHit[]>([])
  const [total, setTotal] = useState(0)
  const [searched, setSearched] = useState<PackKind | null>(null)
  const [installing, setInstalling] = useState<string | null>(null)
  const seq = useRef(0)

  const load = useCallback(async (k: PackKind) => {
    try {
      setListing(await invoke<PacksListing>("packs_list", { kind: k }))
    } catch (e) {
      setMsg({ text: String(e), error: true })
    }
  }, [])

  const action = useCallback(
    async (fn: () => Promise<void>) => {
      if (busy) return
      setBusy(true)
      try {
        await fn()
      } catch (e) {
        setMsg({ text: String(e), error: true })
      } finally {
        setBusy(false)
      }
    },
    [busy]
  )

  const added = async (res: PacksAdded) => {
    if (res.lines.length)
      setMsg({ text: res.lines.join("\n"), error: res.lines.some((l) => !l.startsWith("Добавлен")) })
    const k = res.kind ?? kind
    if (k !== kind) onKindChange(k)
    await load(k)
  }

  const search = useCallback(async (k: PackKind, q: string, offset: number) => {
    const my = ++seq.current
    try {
      const res = await invoke<PackSearch>("packs_search", { kind: k, query: q, offset })
      if (my !== seq.current) return
      setHits((prev) => (offset ? [...prev, ...res.hits] : res.hits))
      setTotal(res.total)
      setSearched(k)
    } catch (e) {
      if (my === seq.current) setMsg({ text: String(e), error: true })
    }
  }, [])

  // Открытие окна: список и .zip, брошенные в окно лаунчера.
  useEffect(() => {
    if (!open) return
    setMsg(null)
    if (dropped.length) {
      setMode("mine")
      action(async () => added(await invoke<PacksAdded>("packs_add_files", { files: dropped })))
    } else load(kind)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, dropped])

  useEffect(() => {
    if (!open) return
    load(kind)
    if (mode === "find" && searched !== kind) search(kind, query, 0)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind, mode])

  useEffect(() => {
    if (!open || mode !== "find") return
    const timer = setTimeout(() => search(kind, query, 0), 350)
    return () => clearTimeout(timer)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query])

  // Включённые ресурспаки сверху в порядке важности, как в игре.
  const packs = useMemo(() => {
    const list = [...(listing?.packs ?? [])]
    return list.sort((a, b) => {
      if (a.enabled !== b.enabled) return a.enabled ? -1 : 1
      if (a.enabled && kind === "resource") return (b.order ?? 0) - (a.order ?? 0)
      return a.title.localeCompare(b.title)
    })
  }, [listing, kind])
  const enabledCount = packs.filter((p) => p.enabled).length

  const install = (h: PackHit) =>
    action(async () => {
      setInstalling(h.project_id)
      try {
        const title = await invoke<string>("packs_install", {
          kind,
          project: h.project_id,
          icon: h.icon_url ?? null,
        })
        setMsg({ text: `Установлен и включён: ${title}`, error: false })
        setHits((prev) => prev.map((x) => (x.project_id === h.project_id ? { ...x, installed: true } : x)))
        await load(kind)
      } finally {
        setInstalling(null)
      }
    })

  const toggle = (p: ContentPack, enabled: boolean) =>
    action(async () => {
      try {
        await invoke("packs_toggle", { kind, file: p.file, enabled })
        setMsg(null)
      } finally {
        await load(kind)
      }
    })

  const move = (p: ContentPack, up: boolean) =>
    action(async () => {
      try {
        await invoke("packs_move", { file: p.file, up })
      } finally {
        await load(kind)
      }
    })

  const irisWarning = kind === "shader" && listing && !listing.iris.ready && (
    <Alert>
      <AlertDescription className="flex flex-wrap items-center gap-3">
        <span className="flex-1">
          Шейдеры работают через Iris, а он сейчас выключен в модах сборки
          {listing.iris.missing.length > 1 ? ` (нужно включить: ${listing.iris.missing.join(", ")})` : ""}. Кнопка
          включит его и скачает.
        </span>
        <Button
          size="sm"
          disabled={busy}
          onClick={() =>
            action(async () => {
              await invoke("packs_enable_iris")
              setMsg({ text: "Iris включён", error: false })
              await load(kind)
            })
          }
        >
          {busy ? <Loader2 className="animate-spin" /> : null}
          Включить Iris
        </Button>
      </AlertDescription>
    </Alert>
  )

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[min(640px,88vh)] w-[min(820px,94vw)] max-w-none flex-col gap-3 sm:max-w-none">
        <DialogHeader className="flex-row flex-wrap items-center gap-4 pr-8">
          <DialogTitle className="sr-only">Ресурспаки и шейдеры</DialogTitle>
          <Tabs value={kind} onValueChange={(v) => onKindChange(v as PackKind)}>
            <TabsList>
              <TabsTrigger value="resource">Ресурспаки</TabsTrigger>
              <TabsTrigger value="shader">Шейдеры</TabsTrigger>
            </TabsList>
          </Tabs>
          <Tabs value={mode} onValueChange={(v) => setMode(v as Mode)}>
            <TabsList>
              <TabsTrigger value="mine">Установленные</TabsTrigger>
              <TabsTrigger value="find">Найти на Modrinth</TabsTrigger>
            </TabsList>
          </Tabs>
        </DialogHeader>

        {msg && (
          <Alert variant={msg.error ? "destructive" : "default"} className="select-text">
            <AlertDescription className="whitespace-pre-line">{msg.text}</AlertDescription>
          </Alert>
        )}
        {irisWarning}

        {mode === "mine" ? (
          <div className="flex min-h-0 flex-1 flex-col gap-3">
            <div className="flex flex-wrap gap-2">
              <Button
                variant="outline"
                disabled={busy}
                onClick={() => action(async () => added(await invoke<PacksAdded>("packs_pick_files")))}
              >
                <Plus />
                Добавить .zip
              </Button>
              <Button variant="outline" onClick={() => invoke("open_packs_dir", { kind })}>
                <FolderOpen />
                Открыть папку
              </Button>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() =>
                  action(async () => {
                    const titles = await invoke<string[]>("packs_update", { kind })
                    setMsg({
                      text: titles.length ? `Обновлены: ${titles.join(", ")}` : "Всё уже последних версий",
                      error: false,
                    })
                    await load(kind)
                  })
                }
              >
                {busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                Обновить
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              {t.hint} Можно просто перетащить .zip в окно лаунчера, лаунчер сам поймёт, ресурспак это или
              шейдер.
            </p>

            <div className="min-h-0 flex-1 overflow-auto pr-1">
              {listing && packs.length === 0 && (
                <p className="py-8 text-center text-sm text-muted-foreground">{t.empty}</p>
              )}
              <ul className="flex flex-col gap-1.5">
                {packs.map((p, i) => (
                  <Row key={p.file} dim={!p.enabled}>
                    <ModIcon url={p.icon} title={p.title} />
                    <div className="flex min-w-0 flex-1 flex-col">
                      <span className="truncate text-sm font-medium">{p.title}</span>
                      <span className="truncate text-xs text-muted-foreground">
                        {[p.version, p.project ? "Modrinth" : "свой .zip", kind === "shader" && p.enabled ? "включён" : null]
                          .filter(Boolean)
                          .join(" · ")}
                      </span>
                    </div>
                    {kind === "resource" && p.enabled && enabledCount > 1 && (
                      <div className="flex">
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label="Выше"
                          disabled={busy || i === 0}
                          onClick={() => move(p, true)}
                        >
                          <ArrowUp />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label="Ниже"
                          disabled={busy || i === enabledCount - 1}
                          onClick={() => move(p, false)}
                        >
                          <ArrowDown />
                        </Button>
                      </div>
                    )}
                    <Switch
                      checked={p.enabled}
                      aria-label={p.enabled ? "Выключить" : "Включить"}
                      disabled={busy}
                      onCheckedChange={(enabled) => toggle(p, enabled)}
                    />
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label="Удалить"
                      className="text-muted-foreground hover:text-destructive"
                      onClick={() => setRemoving(p)}
                    >
                      <Trash2 />
                    </Button>
                  </Row>
                ))}
              </ul>
            </div>
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col gap-3">
            <div className="relative">
              <SearchIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                autoFocus
                className="h-9 pl-8"
                type="search"
                placeholder={t.search}
                spellCheck={false}
                autoComplete="off"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
            <div className="min-h-0 flex-1 overflow-auto pr-1">
              {searched === kind && hits.length === 0 && (
                <p className="py-8 text-center text-sm text-muted-foreground">
                  Ничего не нашлось для Minecraft {listing?.minecraft}
                </p>
              )}
              <ul className="flex flex-col gap-1.5">
                {hits.map((h) => (
                  <Row key={h.project_id}>
                    <ModIcon url={h.icon_url} title={h.title} />
                    <div className="flex min-w-0 flex-1 flex-col">
                      <span className="truncate text-sm font-medium">{h.title}</span>
                      <span className="truncate text-xs text-muted-foreground">
                        {h.author} · {downloadsLabel(h.downloads)} загрузок
                      </span>
                      {h.description && (
                        <span className="line-clamp-2 text-xs text-muted-foreground">{h.description}</span>
                      )}
                    </div>
                    <div className="flex shrink-0 items-center gap-1.5">
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => openUrl(`https://modrinth.com/${t.modrinth}/${h.slug}`)}
                      >
                        <ExternalLink />
                        Страница
                      </Button>
                      {h.installed ? (
                        <Button variant="secondary" size="sm" disabled>
                          <Check />
                          Установлен
                        </Button>
                      ) : (
                        <Button size="sm" disabled={busy} onClick={() => install(h)}>
                          {installing === h.project_id ? <Loader2 className="animate-spin" /> : <Download />}
                          {installing === h.project_id ? "Ставим…" : "Установить"}
                        </Button>
                      )}
                    </div>
                  </Row>
                ))}
              </ul>
              {searched === kind && hits.length < total && (
                <div className="flex justify-center py-3">
                  <Button variant="outline" onClick={() => search(kind, query, hits.length)}>
                    Показать ещё
                  </Button>
                </div>
              )}
            </div>
          </div>
        )}
      </DialogContent>

      <AlertDialog open={removing !== null} onOpenChange={(o) => !o && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Удалить «{removing?.title}»?</AlertDialogTitle>
            <AlertDialogDescription>Файл удалится из папки игры.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Отмена</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                const p = removing
                setRemoving(null)
                if (p)
                  action(async () => {
                    await invoke("packs_remove", { kind, file: p.file })
                    setMsg({ text: `Удалён: ${p.title}`, error: false })
                    await load(kind)
                  })
              }}
            >
              Удалить
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Dialog>
  )
}
