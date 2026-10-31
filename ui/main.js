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
  $("open-login").onclick = () => T.opener.openUrl(code.verification_uri)
  $("copy-code").onclick = () => navigator.clipboard.writeText(code.user_code)
  T.opener.openUrl(code.verification_uri).catch(() => {})
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

$("play").onclick = play
$("settings-btn").onclick = openSettings
$("settings-save").onclick = saveSettings
$("mem").oninput = () => { $("mem-value").textContent = mb(Number($("mem").value)) }
$("log-btn").onclick = () => { $("log-panel").hidden = !$("log-panel").hidden }
$("open-dir").onclick = () => invoke("open_game_dir")
$("logout").onclick = async () => { await invoke("logout"); $("settings").close(); await refresh() }
for (const b of document.querySelectorAll("[data-url]")) b.onclick = () => T.opener.openUrl(b.dataset.url)

refresh().then(() => { pingServer(); checkUpdate() })
setInterval(pingServer, 60_000)
