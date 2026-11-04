// Команды и события Rust-стороны (src-tauri/src/main.rs).
import { invoke } from "@tauri-apps/api/core"

export { invoke }
export { listen } from "@tauri-apps/api/event"

export type Settings = {
  memory_mb: number
  close_on_launch: boolean
  extra_jvm_args: string
  offline_profiles: string[]
  active_profile: string | null
}

export type Account = { name: string; uuid: string }

export type Info = {
  version: string
  account: Account | null
  pack_version: string | null
  login_configured: boolean
  settings: Settings
  /** false in Flatpak: updates come from Flathub */
  self_update: boolean
}

export type ServerStatus = { online: number; max: number }

export type DeviceCode = {
  user_code: string
  verification_uri: string
  [key: string]: unknown
}

export type UserMod = {
  file: string
  title: string
  version?: string | null
  project?: string | null
  icon?: string | null
  enabled: boolean
  note?: string | null
  required_by?: string[]
}

export type ModListing = { mods: UserMod[]; pack: string[]; minecraft: string }

export type SearchHit = {
  project_id: string
  slug: string
  title: string
  description: string
  icon_url?: string | null
  downloads: number
  author: string
  installed: boolean
  reason?: string | null
  verdict: { kind: string }
}

export type Search = { hits: SearchHit[]; total: number }

export type ProgressEvent =
  | { kind: "stage"; name: string }
  | { kind: "progress"; done: number; total: number }
  | { kind: "log"; line: string }
  | ({ kind: "game" } & GameLine)

// Строка лога игры, разобранная из log4j XML.
export type GameLine = {
  time: number
  level: string
  logger: string
  thread: string
  message: string
}

export function openUrl(url: string) {
  return invoke("open_url", { url })
}

export const LINKS = {
  site: "https://www.minehouse.pro",
  wiki: "https://wiki.minehouse.pro",
  discord: "https://discord.gg/gCYGxDzaCb",
}

export function memLabel(n: number) {
  return n >= 1024 ? `${(n / 1024).toFixed(n % 1024 ? 1 : 0)} ГБ` : `${n} МБ`
}

export function downloadsLabel(n: number) {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} млн`
  if (n >= 1e3) return `${Math.round(n / 1e3)} тыс.`
  return String(n)
}

export type NewsPost = {
  slug: string
  kind: "news" | "blog"
  title: string
  excerpt: string
  cover_url: string | null
  pinned: boolean
  published_at: string | null
  author_name: string | null
  url: string
}

export function newsDate(iso: string | null) {
  if (!iso) return ""
  return new Date(iso).toLocaleDateString("ru-RU", { day: "numeric", month: "long" })
}

/** Ник, за который будем играть: твинк или основной аккаунт. */
export function playingAs(info: Info | null) {
  const alt = info?.settings.active_profile
  if (alt && info?.settings.offline_profiles.includes(alt)) return { name: alt, alt: true }
  return info?.account ? { name: info.account.name, alt: false } : null
}
