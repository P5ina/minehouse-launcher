import { motion, useReducedMotion } from "motion/react"

import { GlyphLottery } from "@/components/react-bits/glyph-lottery"
import { VoxelArc } from "@/components/react-bits/voxel-arc"
import type { ServerState } from "@/hooks/use-launcher"
import { cn } from "@/lib/utils"

import { StatusChip } from "./status-chip"

// Фон окна (zinc-950): сцена эффекта плавно уходит в него.
const BACKGROUND = "#09090b"

// Главный экран как hero сайта: React Bits Pro «Voxel Arc» на фоне,
// «Glyph Lottery» в заголовке. Пока идёт игра, сцена стоит на паузе.
export function Hero({
  server,
  paused,
  compact,
}: {
  server: ServerState
  paused: boolean
  compact: boolean
}) {
  const reduced = useReducedMotion()
  const rise = (delay: number) =>
    reduced
      ? { initial: { opacity: 0 }, animate: { opacity: 1 } }
      : {
          initial: { opacity: 0, y: 12 },
          animate: { opacity: 1, y: 0 },
          transition: { duration: 0.5, delay },
        }

  return (
    <section
      className={cn(
        "relative flex flex-col items-center justify-center px-6 text-center",
        compact ? "py-6" : "flex-1"
      )}
    >
      <div aria-hidden className="absolute inset-0 -z-10 overflow-hidden">
        <VoxelArc
          color="#3f3f46"
          accentColor="#e4e4e7"
          backgroundColor={BACKGROUND}
          horizon={0.72}
          curvature={0.55}
          voxelSize={0.5}
          waveHeight={1.1}
          speed={0.35}
          glow={0.6}
          haze={0.6}
          bloom={0.3}
          grain={0.15}
          dpr={1.25}
          paused={paused}
          className="min-h-0"
        />
        <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_60%_45%_at_50%_42%,var(--background),transparent)] opacity-80" />
        <div className="pointer-events-none absolute inset-x-0 bottom-0 h-24 bg-linear-to-t from-background to-transparent" />
      </div>

      <motion.div {...rise(0)}>
        <StatusChip server={server} className="bg-background/60 backdrop-blur" />
      </motion.div>

      <div className={cn("w-full", compact ? "mt-3" : "mt-5")}>
        <GlyphLottery
          as="h1"
          text="MINEHOUSE"
          trigger="mount"
          direction="center"
          fontSize={compact ? "3rem" : "clamp(3.5rem, 12vw, 7.5rem)"}
          fontWeight={700}
          letterSpacing="-0.04em"
          lineHeight={1}
          align="center"
          color="#fafafa"
          accentColor="#a1a1aa"
          duration={1.6}
          stagger={0.07}
          clickToSpin
          className="w-full font-sans"
        />
      </div>

      {!compact && (
        <motion.p
          {...rise(0.9)}
          className="mt-5 max-w-lg text-lg font-medium tracking-tight text-balance"
        >
          Обычное выживание с друзьями, только чуть удобнее.
        </motion.p>
      )}
    </section>
  )
}
