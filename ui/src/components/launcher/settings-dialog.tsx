import { useEffect, useState } from "react"
import { FolderOpen, Loader2, LogOut } from "lucide-react"

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
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Separator } from "@/components/ui/separator"
import { Slider } from "@/components/ui/slider"
import { Switch } from "@/components/ui/switch"
import { invoke, memLabel, type Info, type Settings } from "@/lib/api"

import { DataDirField, useMoveDataDir } from "./data-dir"

export function SettingsDialog({
  open,
  onOpenChange,
  info,
  playing,
  onChanged,
  onOnboarding,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  info: Info
  playing: boolean
  onChanged: () => Promise<void>
  onOnboarding: () => void
}) {
  const [s, setS] = useState<Settings>(info.settings)
  const [target, setTarget] = useState<string | null>(null)
  const mover = useMoveDataDir()
  useEffect(() => {
    if (open) setS(info.settings)
  }, [open, info.settings])

  const save = async () => {
    await invoke("save_settings", { settings: s })
    await onChanged()
    onOpenChange(false)
  }
  const logout = async () => {
    await invoke("logout")
    onOpenChange(false)
    await onChanged()
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="gap-5 sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Настройки</DialogTitle>
        </DialogHeader>

        <div className="grid gap-3">
          <div className="flex items-center justify-between">
            <Label>Память для игры</Label>
            <span className="text-sm font-medium tabular-nums">{memLabel(s.memory_mb)}</span>
          </div>
          <Slider
            min={2048}
            max={16384}
            step={512}
            value={[s.memory_mb]}
            onValueChange={(v) => {
              const n = Array.isArray(v) ? v[0] : v
              setS((p) => ({ ...p, memory_mb: n }))
            }}
          />
        </div>

        <div className="flex items-center justify-between gap-4">
          <Label htmlFor="close-on-launch">Скрывать лаунчер во время игры</Label>
          <Switch
            id="close-on-launch"
            checked={s.close_on_launch}
            onCheckedChange={(v) => setS((p) => ({ ...p, close_on_launch: v }))}
          />
        </div>

        <div className="flex items-center justify-between gap-4">
          <div className="grid gap-1">
            <Label htmlFor="system-java">Java с компьютера</Label>
            <p className="text-xs text-muted-foreground">
              Если нужная версия уже установлена, лаунчер не скачивает свою
            </p>
          </div>
          <Switch
            id="system-java"
            checked={s.system_java}
            onCheckedChange={(v) => setS((p) => ({ ...p, system_java: v }))}
          />
        </div>

        <div className="grid gap-2">
          <Label htmlFor="jvm">Доп. параметры Java</Label>
          <Input
            id="jvm"
            className="font-mono"
            placeholder="например -XX:+UseZGC"
            spellCheck={false}
            value={s.extra_jvm_args}
            onChange={(e) => setS((p) => ({ ...p, extra_jvm_args: e.target.value }))}
          />
        </div>

        <Separator />

        {info.can_move_data && (
          <div className="grid gap-2">
            <Label>Папка для игры и сборок</Label>
            <DataDirField
              path={info.data_dir}
              defaultPath={info.default_data_dir}
              disabled={playing || mover.moving}
              onPick={setTarget}
            />
            {mover.moving && (
              <p className="flex items-center gap-2 text-xs text-muted-foreground">
                <Loader2 className="size-3.5 animate-spin" />
                {mover.stage}
              </p>
            )}
            {mover.error && (
              <Alert variant="destructive">
                <AlertDescription>{mover.error}</AlertDescription>
              </Alert>
            )}
          </div>
        )}

        <div className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={() => invoke("open_game_dir")}>
            <FolderOpen />
            Папка сборки
          </Button>
          <Button
            variant="ghost"
            onClick={() => {
              onOpenChange(false)
              onOnboarding()
            }}
          >
            Знакомство с лаунчером
          </Button>
          {info.account && (
            <Button variant="destructive" onClick={logout}>
              <LogOut />
              Выйти из аккаунта
            </Button>
          )}
        </div>

        <DialogFooter className="items-center sm:justify-between">
          <span className="text-xs text-muted-foreground">Лаунчер {info.version}</span>
          <Button onClick={save}>Готово</Button>
        </DialogFooter>
      </DialogContent>

      <AlertDialog open={target !== null} onOpenChange={(o) => !o && setTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Перенести игру?</AlertDialogTitle>
            <AlertDialogDescription>
              Java, Minecraft и все сборки с модами и мирами переедут в{" "}
              <span className="font-mono break-all text-foreground">{target}</span>. На другой диск копирование может
              занять несколько минут, старая папка удалится только после полной копии.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Отмена</AlertDialogCancel>
            <AlertDialogAction
              onClick={async () => {
                const t = target
                setTarget(null)
                if (t && (await mover.move(t))) await onChanged()
              }}
            >
              Перенести
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Dialog>
  )
}
