import { useEffect, useState } from "react"
import { FolderOpen, LogOut } from "lucide-react"

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

export function SettingsDialog({
  open,
  onOpenChange,
  info,
  onChanged,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  info: Info
  onChanged: () => Promise<void>
}) {
  const [s, setS] = useState<Settings>(info.settings)
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

        <div className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={() => invoke("open_game_dir")}>
            <FolderOpen />
            Открыть папку игры
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
    </Dialog>
  )
}
