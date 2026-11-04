import { BookOpen, Globe, MessageCircle } from "lucide-react"

import { Button } from "@/components/ui/button"
import { LINKS, openUrl } from "@/lib/api"

// Шапка как на сайте: логотип слева, ссылки справа.
export function TopBar({ onLog }: { onLog: (line: string) => void }) {
  const open = (url: string) =>
    openUrl(url).catch((e) => onLog(`Не удалось открыть ссылку: ${e}`))
  return (
    <header className="relative z-10 flex h-12 shrink-0 items-center gap-2 border-b bg-background/60 px-4 backdrop-blur">
      <img src="/logo.png" alt="" className="size-6" draggable={false} />
      <span className="text-sm font-semibold tracking-tight">MineHouse</span>
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
