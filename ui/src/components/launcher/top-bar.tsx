import { BookOpen, Globe, MessageCircle } from "lucide-react"

import { Button } from "@/components/ui/button"
import { LINKS, openUrl, type Info } from "@/lib/api"

import { InstanceMenu } from "./instance-menu"

// Шапка как на сайте: логотип и выбор сборки слева, ссылки справа.
export function TopBar({
  info,
  playing,
  onLog,
  onChanged,
}: {
  info: Info | null
  playing: boolean
  onLog: (line: string) => void
  onChanged: () => Promise<void>
}) {
  const open = (url: string) =>
    openUrl(url).catch((e) => onLog(`Не удалось открыть ссылку: ${e}`))
  return (
    <header className="relative z-10 flex h-12 shrink-0 items-center gap-2 border-b bg-background/60 px-4 backdrop-blur">
      <img src="/logo.png" alt="" className="size-6" draggable={false} />
      <span className="text-sm font-semibold tracking-tight">MineHouse</span>
      <span aria-hidden className="ml-1 text-muted-foreground/50">/</span>
      <InstanceMenu info={info} disabled={playing} onChanged={onChanged} />
      <nav className="ml-auto flex items-center gap-1">
        <Button variant="ghost" size="sm" onClick={() => open(LINKS.site)}>
          <Globe />
          Сайт
        </Button>
        <Button variant="ghost" size="sm" onClick={() => open(LINKS.wiki)}>
          <BookOpen />
          Вики
        </Button>
        <Button variant="ghost" size="sm" onClick={() => open(LINKS.discord)}>
          <MessageCircle />
          Discord
        </Button>
      </nav>
    </header>
  )
}
