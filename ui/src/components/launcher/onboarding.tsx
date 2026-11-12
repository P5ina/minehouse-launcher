import { useEffect, useState } from "react"
import { AnimatePresence, motion, useReducedMotion } from "motion/react"
import {
  ArrowLeft,
  ArrowRight,
  Check,
  Copy,
  ExternalLink,
  FolderInput,
  Layers,
  Loader2,
  MessageCircle,
  Play,
  UserRound,
} from "lucide-react"

import { CipherReveal } from "@/components/react-bits/cipher-reveal"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { useDeviceLogin } from "@/hooks/use-device-login"
import { activeInstance, invoke, LINKS, openUrl, type Info } from "@/lib/api"
import { cn } from "@/lib/utils"

import { DataDirField, useMoveDataDir } from "./data-dir"

type StepId = "welcome" | "folder" | "instance" | "account" | "discord" | "done"

const LABELS: Record<StepId, string> = {
  welcome: "Привет",
  folder: "Папка",
  instance: "Сборка",
  account: "Microsoft",
  discord: "Discord",
  done: "Готово",
}

function Link({ onClick, children }: { onClick: () => void; children: React.ReactNode }) {
  return (
    <button type="button" onClick={onClick} className="text-foreground underline underline-offset-4 hover:text-primary">
      {children}
    </button>
  )
}

// Знакомство при первом запуске: куда ставить игру, первая сборка, вход
// через Microsoft и Discord. Как /start на сайте, только внутри лаунчера.
export function Onboarding({
  info,
  onChanged,
  onClose,
  onPlay,
}: {
  info: Info
  onChanged: () => Promise<void>
  onClose: () => void
  onPlay: () => void
}) {
  const reduced = useReducedMotion()
  const steps: StepId[] = [
    "welcome",
    ...(info.can_move_data ? (["folder"] as const) : []),
    "instance",
    "account",
    "discord",
    "done",
  ]
  const [index, setIndex] = useState(0)
  const step = steps[index]
  const [dir, setDir] = useState(info.data_dir)
  const instance = activeInstance(info)
  const [name, setName] = useState(instance?.name ?? "MineHouse")
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)
  const mover = useMoveDataDir()

  const login = useDeviceLogin({
    active: step === "account" && !info.account,
    configured: info.login_configured,
    onDone: onChanged,
  })

  useEffect(() => setError(""), [step])

  const finish = async (play: boolean) => {
    await invoke("setup_finish", { done: true })
    await onChanged()
    onClose()
    if (play) onPlay()
  }

  const next = async () => {
    setError("")
    if (step === "folder" && dir !== info.data_dir) {
      if (!(await mover.move(dir))) return
      await onChanged()
    }
    if (step === "instance" && instance && name.trim() !== instance.name) {
      setBusy(true)
      try {
        await invoke("instance_rename", { id: instance.id, name })
        await onChanged()
      } catch (e) {
        setError(String(e))
        return
      } finally {
        setBusy(false)
      }
    }
    setIndex((i) => Math.min(i + 1, steps.length - 1))
  }

  const working = busy || mover.moving
  const anim = reduced
    ? { initial: { opacity: 0 }, animate: { opacity: 1 }, exit: { opacity: 0 } }
    : {
        initial: { opacity: 0, x: 24 },
        animate: { opacity: 1, x: 0 },
        exit: { opacity: 0, x: -24 },
        transition: { duration: 0.25 },
      }

  return (
    <div className="absolute inset-0 z-40 flex flex-col bg-background">
      <header className="flex h-12 shrink-0 items-center gap-2 border-b px-4">
        <img src="/logo.png" alt="" className="size-6" draggable={false} />
        <span className="text-sm font-semibold tracking-tight">MineHouse</span>
        <ol className="mx-auto flex items-center gap-1.5">
          {steps.map((s, i) => (
            <li key={s} className="flex items-center gap-1.5">
              <span
                className={cn(
                  "flex h-6 items-center gap-1.5 rounded-full px-2.5 text-xs transition-colors",
                  i === index ? "bg-primary text-primary-foreground" : "text-muted-foreground",
                  i < index && "text-foreground"
                )}
              >
                {i < index && <Check className="size-3" />}
                {LABELS[s]}
              </span>
            </li>
          ))}
        </ol>
        {step !== "done" && (
          <Button variant="ghost" size="sm" disabled={working} onClick={() => finish(false)}>
            Пропустить
          </Button>
        )}
      </header>

      <main className="flex min-h-0 flex-1 items-center justify-center overflow-y-auto px-6 py-8">
        <AnimatePresence mode="wait">
          <motion.section key={step} {...anim} className="flex w-full max-w-lg flex-col gap-5">
            {step === "welcome" && (
              <>
                <img src="/logo.png" alt="" className="size-16" draggable={false} />
                <div>
                  <h1 className="text-3xl font-medium tracking-tight">Добро пожаловать на MineHouse</h1>
                  <p className="mt-3 text-muted-foreground">
                    Лаунчер сам поставит Java, Minecraft и нашу сборку и зайдёт сразу на сервер. Пара шагов, и
                    можно играть.
                  </p>
                </div>
              </>
            )}

            {step === "folder" && (
              <>
                <Title icon={<FolderInput />} title="Куда ставить игру">
                  Сюда лягут Java, Minecraft и все сборки, около 1,5 ГБ, а с мирами и шейдерами больше. Если на
                  системном диске мало места, выберите другой. Поменять можно потом в настройках.
                </Title>
                <DataDirField path={dir} defaultPath={info.default_data_dir} disabled={working} onPick={setDir} />
                {mover.moving && <Working text={mover.stage} />}
                {mover.error && <ErrorNote text={mover.error} />}
              </>
            )}

            {step === "instance" && (
              <>
                <Title icon={<Layers />} title="Ваша сборка">
                  Сборка это отдельная папка игры: свои моды, ресурспаки, шейдеры, настройки и миры. Моды
                  MineHouse ставятся в каждую сами. Потом можно завести ещё, например лёгкую и с шейдерами, в меню
                  сборок в шапке.
                </Title>
                <div className="grid gap-2">
                  <Label htmlFor="instance-name">Название</Label>
                  <Input
                    id="instance-name"
                    maxLength={40}
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && name.trim() && next()}
                  />
                </div>
                {error && <ErrorNote text={error} />}
              </>
            )}

            {step === "account" && (
              <>
                <Title icon={<UserRound />} title="Вход через Microsoft">
                  Нужен аккаунт, на котором куплена Minecraft: Java Edition. Пароль вводится на сайте Microsoft,
                  лаунчер его не видит.
                </Title>
                {info.account ? (
                  <div className="flex items-center gap-3 rounded-lg border bg-muted/40 p-3">
                    <img
                      src={`https://mc-heads.net/avatar/${info.account.uuid}/40`}
                      alt=""
                      className="size-10 rounded-md [image-rendering:pixelated]"
                    />
                    <div className="flex flex-col">
                      <span className="font-medium">{info.account.name}</span>
                      <span className="text-xs text-muted-foreground">Вход выполнен</span>
                    </div>
                    <Check className="ml-auto size-5 text-primary" />
                  </div>
                ) : (
                  <>
                    <div className="flex h-20 items-center justify-center rounded-lg border bg-muted/40 select-text">
                      {login.code ? (
                        <CipherReveal
                          text={login.code.user_code}
                          trigger="mount"
                          fontSize="2rem"
                          fontWeight={600}
                          letterSpacing="0.14em"
                          color="#fafafa"
                          className="font-mono"
                        />
                      ) : (
                        <span className="font-mono text-2xl tracking-[0.14em] text-muted-foreground">
                          {login.error ? "—" : "· · · ·"}
                        </span>
                      )}
                    </div>
                    <p className="text-sm text-muted-foreground">
                      Страница входа уже открылась в браузере: введите там этот код. Лаунчер подхватит вход сам.
                    </p>
                    <div className="flex flex-wrap gap-2">
                      <Button variant="outline" disabled={!login.code} onClick={login.copy}>
                        {login.copied ? <Check /> : <Copy />}
                        {login.copied ? "Скопировано" : "Скопировать код"}
                      </Button>
                      <Button
                        variant="outline"
                        disabled={!login.code}
                        onClick={() => login.code && openUrl(login.code.verification_uri)}
                      >
                        <ExternalLink />
                        Открыть страницу входа
                      </Button>
                      {login.error && (
                        <Button variant="ghost" onClick={login.retry}>
                          Новый код
                        </Button>
                      )}
                    </div>
                    {login.error && <ErrorNote text={login.error} />}
                    <p className="text-xs text-muted-foreground">
                      Нет лицензии? Тогда играйте через свой лаунчер, на сайте есть{" "}
                      <Link onClick={() => openUrl(LINKS.start)}>инструкция</Link>.
                    </p>
                  </>
                )}
              </>
            )}

            {step === "discord" && (
              <>
                <Title icon={<MessageCircle />} title="Discord">
                  Играть могут участники нашего Discord. Там же новости, ивенты и помощь.
                </Title>
                <ol className="flex flex-col gap-3 text-sm text-muted-foreground">
                  <Item n={1}>
                    <p>Вступите в Discord MineHouse.</p>
                    <Button className="mt-2 w-fit" size="sm" onClick={() => openUrl(LINKS.discord)}>
                      <MessageCircle />
                      Вступить в Discord
                    </Button>
                  </Item>
                  <Item n={2}>
                    <p>
                      Войдите на сайт через Discord: там профиль, банк и настройки. Если сейчас на сервер пускают по
                      заявкам, <Link onClick={() => openUrl(LINKS.apply)}>подайте заявку</Link>, ответ придёт в
                      личку Discord.
                    </p>
                    <Button className="mt-2 w-fit" variant="outline" size="sm" onClick={() => openUrl(LINKS.login)}>
                      <ExternalLink />
                      Войти на сайт
                    </Button>
                  </Item>
                  <Item n={3}>
                    При первом входе на сервер игра покажет одноразовый код. Подтвердите его на сайте или у бота в
                    Discord, и ник закрепится за вами.
                  </Item>
                </ol>
              </>
            )}

            {step === "done" && (
              <>
                <Title icon={<Check />} title="Всё готово">
                  Нажмите «Играть»: лаунчер скачает всё нужное в сборку «{activeInstance(info)?.name}» и зайдёт на
                  сервер. Первый запуск займёт пару минут.
                </Title>
                <ul className="flex flex-col gap-2 text-sm text-muted-foreground">
                  <li>«Моды» и «Паки» внизу: свои моды, ресурспаки и шейдеры для выбранной сборки.</li>
                  <li>Меню сборок в шапке: новая сборка, копия, переименовать, удалить.</li>
                  <li>В настройках: память для игры и папка, куда всё ставится.</li>
                </ul>
              </>
            )}
          </motion.section>
        </AnimatePresence>
      </main>

      <footer className="flex shrink-0 items-center gap-2 border-t px-5 py-3.5">
        {index > 0 && step !== "done" && (
          <Button variant="ghost" disabled={working} onClick={() => setIndex((i) => i - 1)}>
            <ArrowLeft />
            Назад
          </Button>
        )}
        <div className="ml-auto flex gap-2">
          {step === "done" ? (
            <>
              <Button variant="outline" size="lg" onClick={() => finish(false)}>
                В лаунчер
              </Button>
              <Button size="lg" className="min-w-36 font-semibold" onClick={() => finish(!!info.account)}>
                {info.account ? <Play /> : null}
                {info.account ? "Играть" : "Готово"}
              </Button>
            </>
          ) : (
            <Button
              size="lg"
              className="min-w-36"
              disabled={working || (step === "instance" && !name.trim())}
              onClick={next}
            >
              {working && <Loader2 className="animate-spin" />}
              {step === "welcome"
                ? "Начать"
                : step === "account" && !info.account
                  ? "Позже"
                  : "Дальше"}
              {!working && <ArrowRight />}
            </Button>
          )}
        </div>
      </footer>
    </div>
  )
}

function Title({ icon, title, children }: { icon: React.ReactNode; title: string; children: React.ReactNode }) {
  return (
    <div>
      <span className="flex size-10 items-center justify-center rounded-lg border bg-muted/40 text-muted-foreground [&_svg]:size-5">
        {icon}
      </span>
      <h1 className="mt-4 text-2xl font-medium tracking-tight">{title}</h1>
      <p className="mt-2 text-sm text-muted-foreground">{children}</p>
    </div>
  )
}

function Item({ n, children }: { n: number; children: React.ReactNode }) {
  return (
    <li className="flex gap-3">
      <span className="flex size-6 shrink-0 items-center justify-center rounded-full border text-xs tabular-nums text-foreground">
        {n}
      </span>
      <div className="flex min-w-0 flex-col pt-0.5">{children}</div>
    </li>
  )
}

function Working({ text }: { text: string }) {
  return (
    <p className="flex items-center gap-2 text-sm text-muted-foreground">
      <Loader2 className="size-4 animate-spin" />
      {text}
    </p>
  )
}

function ErrorNote({ text }: { text: string }) {
  return (
    <Alert variant="destructive">
      <AlertDescription>{text}</AlertDescription>
    </Alert>
  )
}
