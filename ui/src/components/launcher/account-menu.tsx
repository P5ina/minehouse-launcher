import { useState } from "react"
import { ChevronsUpDown, LogIn, Trash2, UserPlus, UserRound } from "lucide-react"

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
import { invoke, playingAs, type Info } from "@/lib/api"
import { cn } from "@/lib/utils"

const MAIN = "__main__"

function Avatar({ src, className }: { src?: string; className?: string }) {
  return (
    <div
      className={cn(
        "flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-lg border bg-muted",
        className
      )}
    >
      {src ? (
        <img src={src} alt="" className="size-full [image-rendering:pixelated]" draggable={false} />
      ) : (
        <UserRound className="size-5 text-muted-foreground" />
      )}
    </div>
  )
}

// Аккаунт внизу слева. С лицензией в меню можно переключиться на твинка
// (офлайн-ник) или добавить нового, как в Prism Launcher.
export function AccountMenu({
  info,
  disabled,
  onLogin,
  onChanged,
}: {
  info: Info | null
  disabled: boolean
  onLogin: () => void
  onChanged: () => Promise<void>
}) {
  const [adding, setAdding] = useState(false)
  const [nick, setNick] = useState("")
  const [error, setError] = useState("")
  const acc = info?.account
  const as = playingAs(info)
  const avatar = acc
    ? as?.alt
      ? `https://mc-heads.net/avatar/${as.name}/40`
      : `https://mc-heads.net/avatar/${acc.uuid}/40`
    : undefined
  const pack = info?.pack_version ? `Сборка ${info.pack_version}` : "Сборка не установлена"

  const summary = (
    <>
      <Avatar src={avatar} />
      <div className="flex min-w-0 flex-col text-left">
        <span className="truncate text-sm font-medium">{as ? as.name : "Не выполнен вход"}</span>
        <span className="truncate text-xs text-muted-foreground">
          {as?.alt ? `Твинк ${acc?.name}` : pack}
        </span>
      </div>
    </>
  )

  if (!acc)
    return (
      <button
        type="button"
        onClick={onLogin}
        className="flex w-56 min-w-0 items-center gap-3 rounded-lg p-1 -m-1 hover:bg-muted/50"
      >
        {summary}
      </button>
    )

  const select = async (v: string) => {
    await invoke("profile_select", { name: v === MAIN ? null : v })
    await onChanged()
  }
  const add = async () => {
    try {
      await invoke("profile_add", { name: nick })
      setAdding(false)
      await onChanged()
    } catch (e) {
      setError(String(e))
    }
  }

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger
          disabled={disabled}
          className="flex w-56 min-w-0 items-center gap-3 rounded-lg p-1 -m-1 outline-none hover:bg-muted/50 focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none"
        >
          {summary}
          <ChevronsUpDown className="ml-auto size-4 shrink-0 text-muted-foreground" />
        </DropdownMenuTrigger>
        <DropdownMenuContent side="top" align="start" className="w-64">
          <DropdownMenuGroup>
            <DropdownMenuLabel>Играть за</DropdownMenuLabel>
            <DropdownMenuRadioGroup value={as?.alt ? as.name : MAIN} onValueChange={select}>
              <DropdownMenuRadioItem value={MAIN}>
                {acc.name}
                <span className="text-xs text-muted-foreground">Microsoft</span>
              </DropdownMenuRadioItem>
              {info.settings.offline_profiles.map((n) => (
                <DropdownMenuRadioItem key={n} value={n}>
                  {n}
                  <span className="text-xs text-muted-foreground">твинк</span>
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuGroup>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            onClick={() => {
              setNick("")
              setError("")
              setAdding(true)
            }}
          >
            <UserPlus />
            Добавить твинка
          </DropdownMenuItem>
          {info.settings.offline_profiles.map((n) => (
            <DropdownMenuItem
              key={n}
              variant="destructive"
              onClick={async () => {
                await invoke("profile_remove", { name: n })
                await onChanged()
              }}
            >
              <Trash2 />
              Удалить {n}
            </DropdownMenuItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuItem onClick={onLogin}>
            <LogIn />
            Войти в другой Microsoft-аккаунт
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <Dialog open={adding} onOpenChange={setAdding}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>Новый твинк</DialogTitle>
            <DialogDescription>
              Второй ник для игры без отдельной лицензии. Работает, пока в лаунчере выполнен вход в Microsoft-аккаунт
              с купленной игрой. На сервере твинк привязывается к Discord, как обычный игрок.
            </DialogDescription>
          </DialogHeader>
          <form
            onSubmit={(e) => {
              e.preventDefault()
              add()
            }}
            className="grid gap-3"
          >
            <Input
              autoFocus
              placeholder="Ник, например Steve_2"
              maxLength={16}
              spellCheck={false}
              value={nick}
              onChange={(e) => {
                setNick(e.target.value.replace(/[^A-Za-z0-9_]/g, ""))
                setError("")
              }}
            />
            {error && (
              <Alert variant="destructive">
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            )}
            <DialogFooter>
              <Button type="submit" disabled={nick.length < 3}>
                <UserPlus />
                Добавить
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  )
}
