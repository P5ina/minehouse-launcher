import { Loader2, Package, Palette, Play, ScrollText, Settings } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import type { Info } from "@/lib/api"
import { cn } from "@/lib/utils"

import { AccountMenu } from "./account-menu"

function IconButton({
  label,
  active,
  onClick,
  children,
}: {
  label: string
  active?: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant="outline"
            size="icon-lg"
            aria-label={label}
            aria-pressed={active}
            className={cn(active && "bg-muted dark:bg-input/50")}
            onClick={onClick}
          />
        }
      >
        {children}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

export function BottomBar({
  info,
  playing,
  stage,
  progress,
  logOpen,
  onPlay,
  onMods,
  onPacks,
  onLog,
  onSettings,
  onLogin,
  onChanged,
}: {
  info: Info | null
  playing: boolean
  stage: string
  progress: number | null
  logOpen: boolean
  onPlay: () => void
  onMods: () => void
  onPacks: () => void
  onLog: () => void
  onSettings: () => void
  onLogin: () => void
  onChanged: () => Promise<void>
}) {
  const acc = info?.account
  return (
    <footer className="relative z-10 flex shrink-0 items-center gap-5 border-t bg-background/80 px-5 py-3.5 backdrop-blur">
      <AccountMenu info={info} disabled={playing} onLogin={onLogin} onChanged={onChanged} />

      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <span className="h-4 truncate text-xs text-muted-foreground">{stage}</span>
        <Progress value={progress} className={cn(!playing && progress === null && "opacity-40")} />
      </div>

      <div className="flex items-center gap-2">
        <Button variant="outline" size="lg" onClick={onMods}>
          <Package />
          Моды
        </Button>
        <Button variant="outline" size="lg" onClick={onPacks}>
          <Palette />
          Паки
        </Button>
        <IconButton label="Журнал" active={logOpen} onClick={onLog}>
          <ScrollText />
        </IconButton>
        <IconButton label="Настройки" onClick={onSettings}>
          <Settings />
        </IconButton>
        <Button
          size="lg"
          className="h-11 min-w-40 px-6 text-base font-semibold"
          disabled={playing || !info}
          onClick={onPlay}
        >
          {playing ? <Loader2 className="animate-spin" /> : acc ? <Play /> : null}
          {playing ? "Играем…" : acc ? "Играть" : "Войти"}
        </Button>
      </div>
    </footer>
  )
}
