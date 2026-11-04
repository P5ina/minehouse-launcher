import { Badge } from "@/components/ui/badge"
import type { ServerState } from "@/hooks/use-launcher"
import { cn } from "@/lib/utils"

// Тот же значок, что в шапке главной сайта.
export function StatusChip({ server, className }: { server: ServerState; className?: string }) {
  const label =
    server.state === "loading"
      ? "Стучимся на сервер…"
      : server.state === "offline"
        ? "Сервер недоступен"
        : `${server.online}/${server.max} онлайн · play.minehouse.pro`
  const dot =
    server.state === "online"
      ? "bg-emerald-500"
      : server.state === "offline"
        ? "bg-destructive"
        : "bg-muted-foreground"
  return (
    <Badge variant="outline" className={cn("h-6 gap-2 px-2.5", className)} aria-live="polite">
      <span className={cn("size-1.5 rounded-full", dot)} />
      <span className="tabular-nums">{label}</span>
    </Badge>
  )
}
