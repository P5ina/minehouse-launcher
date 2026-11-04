import { useEffect, useState } from "react"
import { motion, useReducedMotion } from "motion/react"
import { ArrowRight, Newspaper, Pin } from "lucide-react"

import { Button } from "@/components/ui/button"
import { invoke, LINKS, newsDate, openUrl, type NewsPost } from "@/lib/api"

// Последние посты с www.minehouse.pro/news на главном экране.
export function NewsStrip() {
  const [posts, setPosts] = useState<NewsPost[] | null>(null)
  const reduced = useReducedMotion()

  useEffect(() => {
    const load = () =>
      invoke<{ posts: NewsPost[] }>("news")
        .then((r) => setPosts(r.posts.slice(0, 3)))
        .catch(() => setPosts((p) => p ?? []))
    load()
    const t = setInterval(load, 10 * 60_000)
    return () => clearInterval(t)
  }, [])

  if (!posts?.length) return null
  return (
    <section className="relative z-10 w-full px-5 pb-4">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="flex items-center gap-2 text-sm font-medium text-muted-foreground">
          <Newspaper className="size-4" />
          Новости
        </h2>
        <Button
          variant="link"
          size="sm"
          className="h-auto p-0 text-muted-foreground"
          onClick={() => openUrl(`${LINKS.site}/news`)}
        >
          Все новости
          <ArrowRight />
        </Button>
      </div>
      <div className="grid grid-cols-3 gap-3">
        {posts.map((p, i) => (
          <motion.button
            key={p.slug}
            type="button"
            initial={reduced ? { opacity: 0 } : { opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.4, delay: 0.2 + i * 0.08 }}
            onClick={() => openUrl(p.url)}
            className="group flex min-w-0 gap-3 rounded-xl border bg-card/70 p-2.5 text-left backdrop-blur transition-colors hover:bg-card focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none"
          >
            {p.cover_url ? (
              <img
                src={p.cover_url}
                alt=""
                draggable={false}
                className="size-14 shrink-0 rounded-lg border object-cover"
                onError={(e) => (e.currentTarget.style.display = "none")}
              />
            ) : null}
            <div className="flex min-w-0 flex-col gap-0.5">
              <span className="flex items-center gap-1 text-xs text-muted-foreground">
                {p.pinned && <Pin className="size-3" />}
                {newsDate(p.published_at)}
                {p.kind === "blog" && " · блог"}
              </span>
              <span className="line-clamp-2 text-sm leading-snug font-medium group-hover:underline">
                {p.title}
              </span>
            </div>
          </motion.button>
        ))}
      </div>
    </section>
  )
}
