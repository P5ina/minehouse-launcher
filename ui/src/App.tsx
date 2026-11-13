import { useState } from "react"

import { BottomBar } from "@/components/launcher/bottom-bar"
import { DropOverlay } from "@/components/launcher/drop-overlay"
import { Hero } from "@/components/launcher/hero"
import { Onboarding } from "@/components/launcher/onboarding"
import { NewsStrip } from "@/components/launcher/news-strip"
import { LogPanel } from "@/components/launcher/log-panel"
import { LoginDialog } from "@/components/launcher/login-dialog"
import { ModsDialog, type ModsTab } from "@/components/launcher/mods-dialog"
import { PacksDialog } from "@/components/launcher/packs-dialog"
import { SettingsDialog } from "@/components/launcher/settings-dialog"
import { TopBar } from "@/components/launcher/top-bar"
import { UpdateBanner } from "@/components/launcher/update-banner"
import { TooltipProvider } from "@/components/ui/tooltip"
import { useLauncher } from "@/hooks/use-launcher"
import { activeInstance, type PackKind } from "@/lib/api"

export function App() {
  const l = useLauncher()
  const [loginOpen, setLoginOpen] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [mods, setMods] = useState<{ open: boolean; tab: ModsTab; dropped: string[] }>({
    open: false,
    tab: "mine",
    dropped: [],
  })
  const [packs, setPacks] = useState<{ open: boolean; kind: PackKind; dropped: string[] }>({
    open: false,
    kind: "resource",
    dropped: [],
  })
  const [showLog, setShowLog] = useState(false)
  // Знакомство: само при первом запуске, ещё раз из настроек.
  const [tour, setTour] = useState(false)
  const onboarding = !!l.info && (tour || !l.info.setup_done)

  const onPlay = () => {
    if (!l.info?.account) setLoginOpen(true)
    else l.play()
  }

  return (
    <TooltipProvider delay={300}>
      <div className="relative flex h-full flex-col">
        <UpdateBanner enabled={l.info?.self_update ?? false} />
        <TopBar info={l.info} running={l.runningIds} onLog={l.pushLog} onChanged={l.refresh} />
        <main className="relative isolate flex min-h-0 flex-1 flex-col">
          <Hero server={l.server} paused={l.anyPlaying} compact={showLog} />
          {showLog ? (
            <LogPanel
              lines={l.log}
              names={Object.fromEntries((l.info?.instances ?? []).map((i) => [i.id, i.name]))}
            />
          ) : (
            <NewsStrip />
          )}
        </main>
        <BottomBar
          info={l.info}
          playing={l.playing}
          stage={l.stage}
          progress={l.progress}
          logOpen={showLog}
          onPlay={onPlay}
          onMods={() => setMods({ open: true, tab: "mine", dropped: [] })}
          onPacks={() => setPacks((p) => ({ ...p, open: true, dropped: [] }))}
          onLog={() => setShowLog((v) => !v)}
          onSettings={() => setSettingsOpen(true)}
          onLogin={() => setLoginOpen(true)}
          onChanged={l.refresh}
        />
        {onboarding && l.info && (
          <Onboarding info={l.info} onChanged={l.refresh} onClose={() => setTour(false)} onPlay={l.play} />
        )}
      </div>

      <LoginDialog
        open={loginOpen}
        onOpenChange={setLoginOpen}
        configured={l.info?.login_configured ?? false}
        onDone={l.refresh}
      />
      {l.info && (
        <SettingsDialog
          open={settingsOpen}
          onOpenChange={setSettingsOpen}
          info={l.info}
          playing={l.anyPlaying}
          onChanged={l.refresh}
          onOnboarding={() => setTour(true)}
        />
      )}
      <ModsDialog
        open={mods.open}
        tab={mods.tab}
        dropped={mods.dropped}
        onTabChange={(tab) => setMods((m) => ({ ...m, tab }))}
        onOpenChange={(open) => setMods((m) => ({ ...m, open, dropped: [] }))}
      />
      <PacksDialog
        open={packs.open}
        kind={packs.kind}
        dropped={packs.dropped}
        onKindChange={(kind) => setPacks((p) => ({ ...p, kind }))}
        onOpenChange={(open) => setPacks((p) => ({ ...p, open, dropped: [] }))}
      />
      <DropOverlay
        onDrop={(jars, zips) => {
          if (activeInstance(l.info)?.kind === "horizon") return
          if (jars.length) setMods({ open: true, tab: "mine", dropped: jars })
          else if (zips.length) setPacks((p) => ({ ...p, open: true, dropped: zips }))
        }}
      />
    </TooltipProvider>
  )
}
