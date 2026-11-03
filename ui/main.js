// MineHouse launcher UI. Talks to the Rust side through Tauri commands
// (src-tauri/src/main.rs) and listens to "progress" / "game" events.
const T = window.__TAURI__
const invoke = T.core.invoke
const $ = (id) => document.getElementById(id)

let info = null
let playing = false
let logLines = []

function setStage(text) { $("stage").textContent = text || "" }
function setBar(done, total) { $("bar").style.width = total > 0 ? `${Math.min(100, (done / total) * 100)}%` : "0" }
function log(line) {
  logLines.push(line)
  if (logLines.length > 2000) logLines = logLines.slice(-1500)
  const el = $("log")
  el.textContent = logLines.join("\n")
  el.parentElement.scrollTop = el.parentElement.scrollHeight
}
function openUrl(url) {
  return invoke("open_url", { url }).catch((e) => log(`Не удалось открыть ссылку: ${e}`))
}
function mb(n) { return n >= 1024 ? `${(n / 1024).toFixed(n % 1024 ? 1 : 0)} ГБ` : `${n} МБ` }

function render() {
  const acc = info?.account
  $("account-name").textContent = acc ? acc.name : "Не выполнен вход"
  $("pack").textContent = info?.pack_version ? `Сборка ${info.pack_version}` : "Сборка не установлена"
  const av = $("avatar")
  if (acc) { av.src = `https://mc-heads.net/avatar/${acc.uuid}/40`; av.hidden = false } else av.hidden = true
  $("logout").hidden = !acc
  const play = $("play")
  play.disabled = playing
  play.textContent = playing ? "Играем…" : acc ? "Играть" : "Войти"
  $("version").textContent = `Лаунчер ${info?.version ?? ""}`
}

async function refresh() {
  info = await invoke("info")
  render()
}

async function pingServer() {
  const s = $("status")
  try {
    const st = await invoke("server_status")
    s.className = "status online"
    $("status-text").textContent = `Онлайн: ${st.online} из ${st.max}`
  } catch {
    s.className = "status offline"
    $("status-text").textContent = "Сервер недоступен"
  }
}

async function login() {
  const dlg = $("login")
  $("login-error").hidden = true
  $("code").textContent = "…"
  if (!info.login_configured) {
    $("login-error").textContent = "Вход через Microsoft ещё не настроен в этой сборке лаунчера."
    $("login-error").hidden = false
    dlg.showModal()
    return
  }
  dlg.showModal()
  let code
  try {
    code = await invoke("login_start")
  } catch (e) {
    $("login-error").textContent = String(e)
    $("login-error").hidden = false
    return
  }
  $("code").textContent = code.user_code
  $("open-login").onclick = () => openUrl(code.verification_uri)
  $("copy-code").onclick = () => navigator.clipboard.writeText(code.user_code)
  openUrl(code.verification_uri).catch(() => {})
  try {
    await invoke("login_finish", { code })
    if (dlg.open) dlg.close()
    await refresh()
  } catch (e) {
    if (!dlg.open) return
    $("login-error").textContent = String(e)
    $("login-error").hidden = false
  }
}

async function play() {
  if (!info?.account) return login()
  playing = true
  render()
  try {
    await invoke("play")
    setStage("Игра закрыта")
  } catch (e) {
    setStage(String(e))
    log(`Ошибка: ${e}`)
    if (String(e).includes("войти")) { await refresh() }
  } finally {
    playing = false
    setBar(0, 0)
    await refresh()
  }
}

function openSettings() {
  const s = info.settings
  $("mem").value = s.memory_mb
  $("mem-value").textContent = mb(s.memory_mb)
  $("close-on-launch").checked = s.close_on_launch
  $("jvm").value = s.extra_jvm_args
  $("settings").showModal()
}

async function saveSettings() {
  await invoke("save_settings", {
    settings: {
      memory_mb: Number($("mem").value),
      close_on_launch: $("close-on-launch").checked,
      extra_jvm_args: $("jvm").value,
    },
  })
  await refresh()
}

async function checkUpdate() {
  try {
    const update = await T.updater.check()
    if (!update) return
    $("update-text").textContent = `Доступна версия ${update.version}`
    $("update").hidden = false
    $("update-btn").onclick = async () => {
      $("update-btn").disabled = true
      $("update-text").textContent = "Скачиваем обновление…"
      await update.downloadAndInstall()
      await T.process.relaunch()
    }
  } catch (e) {
    console.warn("update check failed", e)
  }
}

T.event.listen("progress", ({ payload: e }) => {
  if (e.kind === "stage") { setStage(e.name); setBar(0, 0); log(`== ${e.name}`) }
  else if (e.kind === "progress") {
    setBar(e.done, e.total)
  } else if (e.kind === "log") log(e.line)
})
T.event.listen("game", ({ payload }) => {
  if (payload === "started") setBar(1, 1)
})

// ---- Own mods: list, Modrinth search, own jars ----
function el(tag, cls, text) {
  const e = document.createElement(tag)
  if (cls) e.className = cls
  if (text != null) e.textContent = text
  return e
}
function icon(url, title) {
  if (url) {
    const i = el("img"); i.src = url; i.alt = ""; i.loading = "lazy"
    i.onerror = () => i.replaceWith(icon(null, title))
    return i
  }
  return el("div", "noicon", (title || "?").slice(0, 1).toUpperCase())
}
function modsMsg(text, isError) {
  const m = $("mods-msg")
  m.textContent = text || ""
  m.className = isError ? "mods-msg error" : "mods-msg"
  m.hidden = !text
}
function downloads(n) {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} млн`
  if (n >= 1e3) return `${Math.round(n / 1e3)} тыс.`
  return String(n)
}
let modsBusy = false
async function modsAction(fn) {
  if (modsBusy) return
  modsBusy = true
  document.body.style.cursor = "progress"
  try { return await fn() } catch (e) { modsMsg(String(e), true) } finally {
    modsBusy = false
    document.body.style.cursor = ""
  }
}

async function loadMods() {
  let l
  try { l = await invoke("mods_list") } catch (e) { modsMsg(String(e), true); return }
  const list = $("mods-list")
  list.replaceChildren()
  $("mods-empty").hidden = l.mods.length > 0
  for (const m of l.mods) {
    const li = el("li", m.enabled ? "mod" : "mod off")
    li.append(icon(m.icon, m.title))
    const text = el("div", "text")
    text.append(el("span", "title", m.title))
    const meta = [m.version, m.project ? "Modrinth" : "свой .jar"]
    if (m.required_by?.length) meta.push("нужен другим модам")
    text.append(el("span", "meta", meta.filter(Boolean).join(" · ")))
    if (m.note) text.append(el("span", "note", `Выключен: ${m.note}`))
    li.append(text)
    const side = el("div", "side")
    const sw = el("label", "switch")
    sw.title = m.enabled ? "Выключить" : "Включить"
    const cb = el("input"); cb.type = "checkbox"; cb.checked = m.enabled
    cb.onchange = () => modsAction(async () => {
      try { await invoke("mods_toggle", { file: m.file, enabled: cb.checked }); modsMsg("") }
      finally { await loadMods() }
    })
    sw.append(cb, el("span"))
    const del = el("button", "ghost danger", "Удалить")
    del.onclick = () => modsAction(async () => {
      if (!confirm(`Удалить «${m.title}»?`)) return
      await invoke("mods_remove", { file: m.file })
      modsMsg(`Удалён: ${m.title}`)
      await loadMods()
    })
    side.append(sw, del)
    li.append(side)
    list.append(li)
  }
  $("pack-summary").textContent = `Моды сборки MineHouse Minecraft ${l.minecraft} (${l.pack.length}), их ставить не нужно`
  $("pack-files").replaceChildren(...l.pack.map((f) => el("li", null, f.replace(/\.jar$/, ""))))
}

let searchQuery = ""
let searchOffset = 0
let searchSeq = 0
async function search(more) {
  const seq = ++searchSeq
  if (!more) { searchOffset = 0 }
  let res
  try {
    res = await invoke("mods_search", { query: searchQuery, offset: searchOffset })
  } catch (e) { if (seq === searchSeq) modsMsg(String(e), true); return }
  if (seq !== searchSeq) return
  const list = $("search-list")
  if (!more) list.replaceChildren()
  for (const h of res.hits) list.append(searchRow(h))
  searchOffset += res.hits.length
  $("search-more").hidden = searchOffset >= res.total
  if (!more && res.hits.length === 0) list.append(el("li", "muted empty", "Ничего не нашлось для Fabric этой версии"))
}

function searchRow(h) {
  const li = el("li", "mod")
  li.append(icon(h.icon_url, h.title))
  const text = el("div", "text")
  text.append(el("span", "title", h.title))
  text.append(el("span", "meta", `${h.author} · ${downloads(h.downloads)} загрузок`))
  if (h.description) text.append(el("span", "desc", h.description))
  if (h.verdict.kind === "server") text.append(el("span", "badge", "Есть на сервере, поставится та же версия"))
  li.append(text)
  const side = el("div", "side")
  const page = el("button", "ghost", "Страница")
  page.onclick = () => openUrl(`https://modrinth.com/mod/${h.slug}`)
  side.append(page)
  if (h.installed) {
    const b = el("button", "secondary", "Установлен"); b.disabled = true; side.append(b)
  } else if (h.reason) {
    side.append(el("span", "blocked", h.reason))
  } else {
    const b = el("button", null, "Установить")
    b.onclick = () => modsAction(async () => {
      b.disabled = true
      b.textContent = "Ставим…"
      try {
        const titles = await invoke("mods_install", { project: h.project_id, icon: h.icon_url ?? null })
        modsMsg(`Установлено: ${titles.join(", ")}`)
        b.textContent = "Установлен"
        b.className = "secondary"
        loadMods()
      } catch (e) {
        b.disabled = false
        b.textContent = "Установить"
        throw e
      }
    })
    side.append(b)
  }
  li.append(side)
  return li
}

function showTab(name) {
  for (const t of document.querySelectorAll(".tab")) t.classList.toggle("active", t.dataset.tab === name)
  $("tab-mine").hidden = name !== "mine"
  $("tab-find").hidden = name !== "find"
  if (name === "find") {
    if (!$("search-list").children.length) search(false)
    $("mods-q").focus()
  }
}

async function openMods(tab) {
  modsMsg("")
  if (!$("mods").open) $("mods").showModal()
  showTab(tab || "mine")
  await loadMods()
}

async function addFiles(files) {
  const jars = files.filter((f) => f.toLowerCase().endsWith(".jar"))
  if (!jars.length) return
  await openMods("mine")
  await modsAction(async () => {
    const lines = await invoke("mods_add_files", { files: jars })
    modsMsg(lines.join("\n"), lines.some((l) => !l.startsWith("Добавлен")))
    await loadMods()
  })
}

let searchTimer = null
$("mods-q").oninput = () => {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(() => { searchQuery = $("mods-q").value; search(false) }, 350)
}
$("search-more").onclick = () => search(true)
for (const t of document.querySelectorAll(".tab")) t.onclick = () => showTab(t.dataset.tab)
$("mods-btn").onclick = () => openMods("mine")
$("mods-dir").onclick = () => invoke("open_mods_dir")
$("mods-add").onclick = () => modsAction(async () => {
  const lines = await invoke("mods_pick_files")
  if (lines.length) modsMsg(lines.join("\n"), lines.some((l) => !l.startsWith("Добавлен")))
  await loadMods()
})
$("mods-update").onclick = () => modsAction(async () => {
  $("mods-update").disabled = true
  try {
    const titles = await invoke("mods_update")
    modsMsg(titles.length ? `Обновлены: ${titles.join(", ")}` : "Все моды уже последних версий")
    await loadMods()
  } finally { $("mods-update").disabled = false }
})
T.event.listen("tauri://drag-enter", () => document.body.classList.add("dropping"))
T.event.listen("tauri://drag-leave", () => document.body.classList.remove("dropping"))
T.event.listen("tauri://drag-drop", ({ payload }) => {
  document.body.classList.remove("dropping")
  addFiles(payload?.paths ?? [])
})

$("play").onclick = play
$("settings-btn").onclick = openSettings
$("settings-save").onclick = saveSettings
$("mem").oninput = () => { $("mem-value").textContent = mb(Number($("mem").value)) }
$("log-btn").onclick = () => { $("log-panel").hidden = !$("log-panel").hidden }
$("open-dir").onclick = () => invoke("open_game_dir")
$("logout").onclick = async () => { await invoke("logout"); $("settings").close(); await refresh() }
for (const b of document.querySelectorAll("[data-url]")) b.onclick = () => openUrl(b.dataset.url)

refresh().then(() => { pingServer(); checkUpdate() })
setInterval(pingServer, 60_000)
