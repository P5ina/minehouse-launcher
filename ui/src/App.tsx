import { useState } from "react"

import { BottomBar } from "@/components/launcher/bottom-bar"
import { DropOverlay } from "@/components/launcher/drop-overlay"
import { Hero } from "@/components/launcher/hero"
import { NewsStrip } from "@/components/launcher/news-strip"
import { LogPanel } from "@/components/launcher/log-panel"
import { LoginDialog } from "@/components/launcher/login-dialog"
import { ModsDialog, type ModsTab } from "@/components/launcher/mods-dialog"
import { SettingsDialog } from "@/components/launcher/settings-dialog"
import { TopBar } from "@/components/launcher/top-bar"
import { UpdateBanner } from "@/components/launcher/update-banner"
import { TooltipProvider } from "@/components/ui/tooltip"
import { useLauncher } from "@/hooks/use-launcher"

export function App() {
  const l = useLauncher()
  const [loginOpen, setLoginOpen] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [mods, setMods] = useState<{ open: boolean; tab: ModsTab; dropped: string[] }>({
    open: false,
    tab: "mine",
    dropped: [],
  })
  const [showLog, setShowLog] = useState(false)

  const onPlay = () => {
    if (!l.info?.account) setLoginOpen(true)
    else l.play()
  }

  return (
    <TooltipProvider delay={300}>
      <div className="relative flex h-full flex-col">
        <UpdateBanner />
        <TopBar onLog={l.pushLog} />
        <main className="relative isolate flex min-h-0 flex-1 flex-col">
          <Hero server={l.server} paused={l.playing} compact={showLog} />
          {showLog ? <LogPanel lines={l.log} /> : <NewsStrip />}
        </main>
        <BottomBar
          info={l.info}
          playing={l.playing}
          stage={l.stage}
          progress={l.progress}
          logOpen={showLog}
          onPlay={onPlay}
          onMods={() => setMods({ open: true, tab: "mine", dropped: [] })}
          onLog={() => setShowLog((v) => !v)}
          onSettings={() => setSettingsOpen(true)}
          onLogin={() => setLoginOpen(true)}
          onChanged={l.refresh}
        />
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
          onChanged={l.refresh}
        />
      )}
      <ModsDialog
        open={mods.open}
        tab={mods.tab}
        dropped={mods.dropped}
        onTabChange={(tab) => setMods((m) => ({ ...m, tab }))}
        onOpenChange={(open) => setMods((m) => ({ ...m, open, dropped: [] }))}
      />
      <DropOverlay
        onDrop={(paths) => setMods({ open: true, tab: "mine", dropped: paths })}
      />
    </TooltipProvider>
  )
}
