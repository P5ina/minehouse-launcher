import { useState } from "react"
import { ChevronsUpDown, Copy, FolderOpen, Layers, Loader2, Pencil, Plus, Trash2 } from "lucide-react"

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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import { activeInstance, invoke, type Info, type InstanceKind } from "@/lib/api"
import { cn } from "@/lib/utils"

type NameMode = "create" | "duplicate" | "rename"

const KINDS: { kind: InstanceKind; title: string; text: string }[] = [
  {
    kind: "fabric",
    title: "MineHouse",
    text: "Обычный Minecraft с модами MineHouse. Свои моды Fabric, ресурспаки и шейдеры.",
  },
  {
    kind: "horizon",
    title: "Horizon (лёгкая)",
    text: "Minecraft, переписанный на Rust: быстрее на слабых компьютерах. Голосовой чат и метки есть, моды Fabric и шейдеры не ставятся.",
  },
]

const TITLES: Record<NameMode, { title: string; text: string; button: string }> = {
  create: {
    title: "Новая сборка",
    text: "Своя папка игры: настройки и миры отдельно от других сборок. Всё нужное скачается при первом запуске.",
    button: "Создать",
  },
  duplicate: {
    title: "Копия сборки",
    text: "Скопируются моды, паки, настройки и миры. Дальше сборки живут отдельно.",
    button: "Скопировать",
  },
  rename: { title: "Переименовать сборку", text: "", button: "Сохранить" },
}

// Переключатель сборок в шапке, как выбор команды в Vercel: у каждой сборки
// своя папка игры, а Java и Minecraft общие.
export function InstanceMenu({
  info,
  running,
  onChanged,
}: {
  info: Info | null
  /** Сборки, которые сейчас запускаются или играют. */
  running: string[]
  onChanged: () => Promise<void>
}) {
  const [mode, setMode] = useState<NameMode | null>(null)
  const [name, setName] = useState("")
  const [kind, setKind] = useState<InstanceKind>("fabric")
  const [error, setError] = useState("")
  const [working, setWorking] = useState(false)
  const [removing, setRemoving] = useState(false)
  const active = activeInstance(info)
  if (!info || !active) return null

  const open = (m: NameMode) => {
    setMode(m)
    setKind("fabric")
    setError("")
    setName(m === "rename" ? active.name : m === "duplicate" ? `${active.name} (копия)` : "")
  }

  const run = async (f: () => Promise<unknown>) => {
    setWorking(true)
    setError("")
    try {
      await f()
      setMode(null)
      setRemoving(false)
      await onChanged()
    } catch (e) {
      setError(String(e))
    } finally {
      setWorking(false)
    }
  }

  const submit = () =>
    run(() =>
      mode === "create"
        ? invoke("instance_create", { name, kind })
        : mode === "duplicate"
          ? invoke("instance_duplicate", { id: active.id, name })
          : invoke("instance_rename", { id: active.id, name })
    )

  const select = async (id: string) => {
    try {
      await invoke("instance_select", { id })
    } finally {
      await onChanged()
    }
  }

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger
          className="flex h-8 max-w-56 min-w-0 items-center gap-2 rounded-md px-2 text-sm outline-none hover:bg-muted/60 focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-60"
        >
          <Layers className="size-4 shrink-0 text-muted-foreground" />
          <span className="truncate font-medium">{active.name}</span>
          {running.includes(active.id) ? (
            <span className="rounded bg-emerald-500/15 px-1.5 text-[10px] leading-4 text-emerald-400">в игре</span>
          ) : (
            running.length > 0 && (
              <span className="rounded bg-secondary px-1.5 text-[10px] leading-4 text-muted-foreground tabular-nums">
                ещё {running.length} в игре
              </span>
            )
          )}
          <ChevronsUpDown className="size-3.5 shrink-0 text-muted-foreground" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-64">
          <DropdownMenuGroup>
            <DropdownMenuLabel>Сборки</DropdownMenuLabel>
            <DropdownMenuRadioGroup value={active.id} onValueChange={select}>
              {info.instances.map((i) => (
                <DropdownMenuRadioItem key={i.id} value={i.id}>
                  <span className="truncate">{i.name}</span>
                  {i.kind === "horizon" && (
                    <span className="rounded bg-secondary px-1.5 text-[10px] leading-4 text-muted-foreground">
                      Horizon
                    </span>
                  )}
                  {running.includes(i.id) && (
                    <span className="flex items-center gap-1 text-xs text-muted-foreground">
                      <span className="size-1.5 rounded-full bg-emerald-500" />
                      играет
                    </span>
                  )}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuGroup>
          <DropdownMenuSeparator />
          <DropdownMenuItem onClick={() => open("create")}>
            <Plus />
            Новая сборка
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => open("duplicate")}>
            <Copy />
            Копия «{active.name}»
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => open("rename")}>
            <Pencil />
            Переименовать
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => invoke("open_instance_dir", { id: active.id })}>
            <FolderOpen />
            Открыть папку сборки
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            variant="destructive"
            disabled={info.instances.length < 2 || running.includes(active.id)}
            onClick={() => {
              setError("")
              setRemoving(true)
            }}
          >
            <Trash2 />
            Удалить «{active.name}»
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <Dialog open={mode !== null} onOpenChange={(o) => !o && !working && setMode(null)}>
        <DialogContent className="sm:max-w-sm">
          {mode && (
            <>
              <DialogHeader>
                <DialogTitle>{TITLES[mode].title}</DialogTitle>
                {TITLES[mode].text && <DialogDescription>{TITLES[mode].text}</DialogDescription>}
              </DialogHeader>
              <form
                onSubmit={(e) => {
                  e.preventDefault()
                  submit()
                }}
                className="grid gap-3"
              >
                {mode === "create" && (
                  <div className="grid gap-2" role="radiogroup" aria-label="Тип сборки">
                    {KINDS.map((k) => {
                      const off = k.kind === "horizon" && !info.horizon_supported
                      return (
                        <button
                          key={k.kind}
                          type="button"
                          role="radio"
                          aria-checked={kind === k.kind}
                          disabled={off}
                          onClick={() => setKind(k.kind)}
                          className={cn(
                            "grid gap-0.5 rounded-md border px-3 py-2 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50",
                            kind === k.kind ? "border-primary bg-muted/60" : "hover:bg-muted/40"
                          )}
                        >
                          <span className="text-sm font-medium">{k.title}</span>
                          <span className="text-xs text-muted-foreground">
                            {off ? "Пока только для Windows x64, Linux и Mac на Apple silicon." : k.text}
                          </span>
                        </button>
                      )
                    })}
                  </div>
                )}
                <Input
                  autoFocus
                  placeholder="Название, например «С шейдерами»"
                  maxLength={40}
                  value={name}
                  onChange={(e) => {
                    setName(e.target.value)
                    setError("")
                  }}
                />
                {error && (
                  <Alert variant="destructive">
                    <AlertDescription>{error}</AlertDescription>
                  </Alert>
                )}
                <DialogFooter>
                  <Button type="submit" disabled={!name.trim() || working}>
                    {working && <Loader2 className="animate-spin" />}
                    {TITLES[mode].button}
                  </Button>
                </DialogFooter>
              </form>
            </>
          )}
        </DialogContent>
      </Dialog>

      <AlertDialog open={removing} onOpenChange={(o) => !o && !working && setRemoving(false)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Удалить сборку «{active.name}»?</AlertDialogTitle>
            <AlertDialogDescription>
              Папка сборки удалится вместе с модами, паками, настройками и мирами одиночной игры. Всё, что у вас
              на сервере MineHouse, останется.
            </AlertDialogDescription>
          </AlertDialogHeader>
          {error && (
            <Alert variant="destructive">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={working}>Отмена</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={working}
              onClick={(e) => {
                e.preventDefault()
                run(() => invoke("instance_remove", { id: active.id }))
              }}
            >
              {working && <Loader2 className="animate-spin" />}
              Удалить
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
