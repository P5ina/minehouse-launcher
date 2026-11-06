import { useCallback, useEffect, useRef, useState } from "react"
import {
  Check,
  ChevronDown,
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
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  downloadsLabel,
  invoke,
  openUrl,
  type ModListing,
  type Search,
  type SearchHit,
  type UserMod,
} from "@/lib/api"
import { cn } from "@/lib/utils"

export type ModsTab = "mine" | "find"
type Msg = { text: string; error: boolean } | null

export function ModIcon({ url, title }: { url?: string | null; title: string }) {
  const [broken, setBroken] = useState(false)
  if (url && !broken)
    return (
      <img
        src={url}
        alt=""
        loading="lazy"
        onError={() => setBroken(true)}
        className="size-10 shrink-0 rounded-lg border object-cover"
        draggable={false}
      />
    )
  return (
    <div className="flex size-10 shrink-0 items-center justify-center rounded-lg border bg-muted text-sm font-semibold text-muted-foreground">
      {(title || "?").slice(0, 1).toUpperCase()}
    </div>
  )
}

export function Row({
  dim,
  children,
}: {
  dim?: boolean
  children: React.ReactNode
}) {
  return (
    <li
      className={cn(
        "flex items-center gap-3 rounded-lg border bg-card/50 px-3 py-2.5 transition-opacity",
        dim && "opacity-60"
      )}
    >
      {children}
    </li>
  )
}

export function ModsDialog({
  open,
  tab,
  dropped,
  onTabChange,
  onOpenChange,
}: {
  open: boolean
  tab: ModsTab
  dropped: string[]
  onTabChange: (tab: ModsTab) => void
  onOpenChange: (open: boolean) => void
}) {
  const [listing, setListing] = useState<ModListing | null>(null)
  const [msg, setMsg] = useState<Msg>(null)
  const [busy, setBusy] = useState(false)
  const [removing, setRemoving] = useState<UserMod | null>(null)

  const [query, setQuery] = useState("")
  const [hits, setHits] = useState<SearchHit[]>([])
  const [total, setTotal] = useState(0)
  const [searched, setSearched] = useState(false)
  const [installing, setInstalling] = useState<string | null>(null)
  const seq = useRef(0)

  const load = useCallback(async () => {
    try {
      setListing(await invoke<ModListing>("mods_list"))
    } catch (e) {
      setMsg({ text: String(e), error: true })
    }
  }, [])

  const action = useCallback(async (fn: () => Promise<void>) => {
    if (busy) return
    setBusy(true)
    try {
      await fn()
    } catch (e) {
      setMsg({ text: String(e), error: true })
    } finally {
      setBusy(false)
    }
  }, [busy])

  const report = (lines: string[]) => {
    if (lines.length)
      setMsg({ text: lines.join("\n"), error: lines.some((l) => !l.startsWith("Добавлен")) })
  }

  const search = useCallback(async (q: string, offset: number) => {
    const my = ++seq.current
    try {
      const res = await invoke<Search>("mods_search", { query: q, offset })
      if (my !== seq.current) return
      setHits((prev) => (offset ? [...prev, ...res.hits] : res.hits))
      setTotal(res.total)
      setSearched(true)
    } catch (e) {
      if (my === seq.current) setMsg({ text: String(e), error: true })
    }
  }, [])

  // Открытие окна: список модов и файлы, брошенные в окно лаунчера.
  useEffect(() => {
    if (!open) return
    setMsg(null)
    load()
    if (dropped.length)
      action(async () => {
        report(await invoke<string[]>("mods_add_files", { files: dropped }))
        await load()
      })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, dropped])

  useEffect(() => {
    if (open && tab === "find" && !searched) search(query, 0)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, tab])

  useEffect(() => {
    if (!open || tab !== "find") return
    const t = setTimeout(() => search(query, 0), 350)
    return () => clearTimeout(t)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query])

  const install = (h: SearchHit) =>
    action(async () => {
      setInstalling(h.project_id)
      try {
        const titles = await invoke<string[]>("mods_install", {
          project: h.project_id,
          icon: h.icon_url ?? null,
        })
        setMsg({ text: `Установлено: ${titles.join(", ")}`, error: false })
        setHits((prev) => prev.map((x) => (x.project_id === h.project_id ? { ...x, installed: true } : x)))
        await load()
      } finally {
        setInstalling(null)
      }
    })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[min(640px,88vh)] w-[min(820px,94vw)] max-w-none flex-col gap-3 sm:max-w-none">
        <Tabs
          value={tab}
          onValueChange={(v) => onTabChange(v as ModsTab)}
          className="flex min-h-0 flex-1 flex-col gap-3"
        >
          <DialogHeader className="flex-row items-center gap-4 pr-8">
            <DialogTitle className="text-lg">Моды</DialogTitle>
            <TabsList>
              <TabsTrigger value="mine">Мои моды</TabsTrigger>
              <TabsTrigger value="find">Найти на Modrinth</TabsTrigger>
            </TabsList>
          </DialogHeader>

          {msg && (
            <Alert variant={msg.error ? "destructive" : "default"} className="select-text">
              <AlertDescription className="whitespace-pre-line">{msg.text}</AlertDescription>
            </Alert>
          )}

          <TabsContent value="mine" className="flex min-h-0 flex-1 flex-col gap-3">
            <div className="flex flex-wrap gap-2">
              <Button
                variant="outline"
                disabled={busy}
                onClick={() =>
                  action(async () => {
                    report(await invoke<string[]>("mods_pick_files"))
                    await load()
                  })
                }
              >
                <Plus />
                Добавить .jar
              </Button>
              <Button variant="outline" onClick={() => invoke("open_mods_dir")}>
                <FolderOpen />
                Открыть папку модов
              </Button>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() =>
                  action(async () => {
                    const titles = await invoke<string[]>("mods_update")
                    setMsg({
                      text: titles.length ? `Обновлены: ${titles.join(", ")}` : "Все моды уже последних версий",
                      error: false,
                    })
                    await load()
                  })
                }
              >
                {busy ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                Обновить моды
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              Свои моды не стираются при обновлении сборки. Можно просто перетащить .jar в окно лаунчера. Мод,
              который уже есть в сборке или нужен на сервере, лаунчер не поставит, а моды, которые есть на
              сервере, ставит той же версии.
            </p>

            <div className="min-h-0 flex-1 overflow-auto pr-1">
              {listing && listing.mods.length === 0 && (
                <p className="py-8 text-center text-sm text-muted-foreground">
                  Своих модов пока нет. Найдите их на Modrinth или добавьте .jar.
                </p>
              )}
              <ul className="flex flex-col gap-1.5">
                {listing?.mods.map((m) => (
                  <Row key={m.file} dim={!m.enabled}>
                    <ModIcon url={m.icon} title={m.title} />
                    <div className="flex min-w-0 flex-1 flex-col">
                      <span className="truncate text-sm font-medium">{m.title}</span>
                      <span className="truncate text-xs text-muted-foreground">
                        {[m.version, m.project ? "Modrinth" : "свой .jar", m.required_by?.length ? "нужен другим модам" : null]
                          .filter(Boolean)
                          .join(" · ")}
                      </span>
                      {m.note && <span className="text-xs text-destructive">Выключен: {m.note}</span>}
                    </div>
                    <Switch
                      checked={m.enabled}
                      aria-label={m.enabled ? "Выключить" : "Включить"}
                      disabled={busy}
                      onCheckedChange={(enabled) =>
                        action(async () => {
                          try {
                            await invoke("mods_toggle", { file: m.file, enabled })
                            setMsg(null)
                          } finally {
                            await load()
                          }
                        })
                      }
                    />
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label="Удалить"
                      className="text-muted-foreground hover:text-destructive"
                      onClick={() => setRemoving(m)}
                    >
                      <Trash2 />
                    </Button>
                  </Row>
                ))}
              </ul>

              {listing && listing.options.length > 0 && (
                <div className="mt-5 flex flex-col gap-2">
                  <div>
                    <h3 className="text-sm font-medium">Моды сборки MineHouse</h3>
                    <p className="text-xs text-muted-foreground">
                      Их можно выключить, тогда обновление сборки их не вернёт. Вместо выключенного можно
                      поставить свою версию. Обязательные моды (наши, Polymer, голосовой чат, пинги) не выключаются.
                    </p>
                  </div>
                  <ul className="grid gap-1.5 sm:grid-cols-2">
                    {listing.options.map((o) => (
                      <li
                        key={o.id}
                        className={cn(
                          "flex items-center gap-3 rounded-lg border bg-card/50 px-3 py-2 transition-opacity",
                          (!o.enabled || o.missing.length > 0) && "opacity-60"
                        )}
                      >
                        <div className="flex min-w-0 flex-1 flex-col">
                          <span className="truncate text-sm font-medium">{o.name}</span>
                          <span className="truncate text-xs text-muted-foreground" title={o.why ?? undefined}>
                            {o.enabled && o.missing.length > 0 ? `Не работает без: ${o.missing.join(", ")}` : o.why}
                          </span>
                        </div>
                        <Switch
                          checked={o.enabled}
                          aria-label={o.enabled ? "Выключить" : "Включить"}
                          disabled={busy}
                          onCheckedChange={(enabled) =>
                            action(async () => {
                              try {
                                await invoke("mods_pack_option", { id: o.id, enabled })
                                setMsg(null)
                              } finally {
                                await load()
                              }
                            })
                          }
                        />
                      </li>
                    ))}
                  </ul>
                </div>
              )}

              {listing && (
                <details className="group mt-4 text-sm text-muted-foreground">
                  <summary className="flex cursor-pointer list-none items-center gap-1.5 hover:text-foreground">
                    <ChevronDown className="size-4 transition-transform group-open:rotate-180" />
                    Установленные файлы сборки, Minecraft {listing.minecraft} ({listing.pack.length})
                  </summary>
                  <ul className="mt-2 columns-2 gap-6 pl-6 text-xs">
                    {listing.pack.map((f) => (
                      <li key={f} className="truncate py-0.5">
                        {f.replace(/\.jar$/, "")}
                      </li>
                    ))}
                  </ul>
                </details>
              )}
            </div>
          </TabsContent>

          <TabsContent value="find" className="flex min-h-0 flex-1 flex-col gap-3">
            <div className="relative">
              <SearchIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                autoFocus
                className="h-9 pl-8"
                type="search"
                placeholder="Поиск модов для Fabric"
                spellCheck={false}
                autoComplete="off"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
            <div className="min-h-0 flex-1 overflow-auto pr-1">
              {searched && hits.length === 0 && (
                <p className="py-8 text-center text-sm text-muted-foreground">
                  Ничего не нашлось для Fabric этой версии
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
                      {h.verdict.kind === "server" && (
                        <Badge variant="secondary" className="mt-1 w-fit">
                          Есть на сервере, поставится та же версия
                        </Badge>
                      )}
                    </div>
                    <div className="flex shrink-0 items-center gap-1.5">
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => openUrl(`https://modrinth.com/mod/${h.slug}`)}
                      >
                        <ExternalLink />
                        Страница
                      </Button>
                      {h.installed ? (
                        <Button variant="secondary" size="sm" disabled>
                          <Check />
                          Установлен
                        </Button>
                      ) : h.reason ? (
                        <span className="max-w-40 text-right text-xs text-muted-foreground">{h.reason}</span>
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
              {hits.length < total && (
                <div className="flex justify-center py-3">
                  <Button variant="outline" onClick={() => search(query, hits.length)}>
                    Показать ещё
                  </Button>
                </div>
              )}
            </div>
          </TabsContent>
        </Tabs>
      </DialogContent>

      <AlertDialog open={removing !== null} onOpenChange={(o) => !o && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Удалить «{removing?.title}»?</AlertDialogTitle>
            <AlertDialogDescription>Файл мода удалится из папки игры.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Отмена</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                const m = removing
                setRemoving(null)
                if (m)
                  action(async () => {
                    await invoke("mods_remove", { file: m.file })
                    setMsg({ text: `Удалён: ${m.title}`, error: false })
                    await load()
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
