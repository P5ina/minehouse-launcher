//! Official Microsoft account sign-in: Microsoft OAuth (device code) →
//! Xbox Live → XSTS → Minecraft services, plus the ownership check.
//! No offline/cracked mode exists, by design (Mojang EULA).
//!
//! The Azure app's client ID is public, not a secret: [`DEFAULT_CLIENT_ID`]. The app needs
//! Mojang's approval for the Minecraft API: https://aka.ms/mce-reviewappid

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

const MS_DEVICE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const SCOPE: &str = "XboxLive.signin offline_access";
const XBL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_ENTITLEMENTS: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const MC_PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";

/// Azure app "MineHouse Launcher" (personal Microsoft accounts, public client).
pub const DEFAULT_CLIENT_ID: &str = "f33e99c2-e342-4abc-b829-44bd861ee6f6";

/// `MINEHOUSE_MS_CLIENT_ID` at build or run time overrides the built-in app.
pub fn client_id() -> Option<String> {
    option_env!("MINEHOUSE_MS_CLIENT_ID")
        .map(str::to_owned)
        .or_else(|| std::env::var("MINEHOUSE_MS_CLIENT_ID").ok())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Some(DEFAULT_CLIENT_ID.to_owned()))
}

fn require_client_id() -> Result<String> {
    client_id()
        .ok_or_else(|| anyhow!("Вход через Microsoft ещё не настроен в этой сборке лаунчера"))
}

/// What the game needs to start: a Minecraft access token and profile.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    /// Undashed UUID as Mojang returns it.
    pub uuid: String,
    pub xuid: String,
    pub access_token: String,
    /// Unix seconds.
    pub expires_at: u64,
}

/// What is stored between launches. The refresh token is the only long-lived secret.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub refresh_token: String,
    pub session: Session,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    #[serde(default)]
    pub message: String,
}

#[derive(Deserialize)]
struct MsToken {
    access_token: String,
    refresh_token: String,
}

#[derive(Deserialize)]
struct MsError {
    error: String,
    #[serde(default)]
    error_description: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub async fn start_device_code(client: &reqwest::Client) -> Result<DeviceCode> {
    let id = require_client_id()?;
    let res = client
        .post(MS_DEVICE)
        .form(&[("client_id", id.as_str()), ("scope", SCOPE)])
        .send()
        .await?;
    if !res.status().is_success() {
        let e: MsError = res.json().await?;
        bail!("Microsoft: {} {}", e.error, e.error_description);
    }
    Ok(res.json().await?)
}

/// Waits until the player confirms the code in the browser, then signs in all the way.
pub async fn finish_device_code(client: &reqwest::Client, code: &DeviceCode) -> Result<Account> {
    let id = require_client_id()?;
    let deadline = now() + code.expires_in;
    let mut interval = code.interval.max(1);
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        anyhow::ensure!(now() < deadline, "Код входа истёк, попробуйте ещё раз");
        let res = client
            .post(MS_TOKEN)
            .form(&[
                ("client_id", id.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", code.device_code.as_str()),
            ])
            .send()
            .await?;
        if res.status().is_success() {
            let t: MsToken = res.json().await?;
            return minecraft_from_ms(client, t).await;
        }
        let e: MsError = res.json().await?;
        match e.error.as_str() {
            "authorization_pending" => {}
            "slow_down" => interval += 5,
            "authorization_declined" => bail!("Вход отменён"),
            "expired_token" => bail!("Код входа истёк, попробуйте ещё раз"),
            _ => bail!("Microsoft: {} {}", e.error, e.error_description),
        }
    }
}

/// New Minecraft token from the stored refresh token (every launch).
pub async fn refresh(client: &reqwest::Client, account: &Account) -> Result<Account> {
    let id = require_client_id()?;
    let res = client
        .post(MS_TOKEN)
        .form(&[
            ("client_id", id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", account.refresh_token.as_str()),
            ("scope", SCOPE),
        ])
        .send()
        .await?;
    if !res.status().is_success() {
        let e: MsError = res.json().await?;
        bail!("Нужно войти заново ({})", e.error);
    }
    minecraft_from_ms(client, res.json().await?).await
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XToken {
    token: String,
    display_claims: XClaims,
}

#[derive(Deserialize)]
struct XClaims {
    xui: Vec<XUser>,
}

#[derive(Deserialize)]
struct XUser {
    uhs: String,
    #[serde(default)]
    xid: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XError {
    #[serde(default)]
    x_err: u64,
}

async fn minecraft_from_ms(client: &reqwest::Client, ms: MsToken) -> Result<Account> {
    let xbl: XToken = client
        .post(XBL)
        .json(&json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", ms.access_token),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))
        .send()
        .await?
        .error_for_status()
        .context("Xbox Live")?
        .json()
        .await?;

    let res = client
        .post(XSTS)
        .json(&json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl.token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))
        .send()
        .await?;
    if res.status().as_u16() == 401 {
        let e: XError = res.json().await.unwrap_or(XError { x_err: 0 });
        bail!(match e.x_err {
            2148916233 => "У этого аккаунта Microsoft нет профиля Xbox. Зайдите на minecraft.net и создайте его".to_string(),
            2148916235 => "Xbox Live недоступен в вашей стране".to_string(),
            2148916236 | 2148916237 => "Аккаунту нужно подтверждение взрослого на xbox.com".to_string(),
            2148916238 => "Детский аккаунт: его нужно добавить в семью Microsoft".to_string(),
            code => format!("Xbox отказал во входе (XErr {code})"),
        });
    }
    let xsts: XToken = res.error_for_status().context("XSTS")?.json().await?;
    let user = xsts.display_claims.xui.first().context("XSTS: no user")?;

    #[derive(Deserialize)]
    struct McLogin {
        access_token: String,
        expires_in: u64,
    }
    let res = client
        .post(MC_LOGIN)
        .json(&json!({ "identityToken": format!("XBL3.0 x={};{}", user.uhs, xsts.token) }))
        .send()
        .await?;
    if res.status().as_u16() == 403 {
        bail!("Microsoft-приложение лаунчера ещё не одобрено Mojang для входа в Minecraft");
    }
    let mc: McLogin = res
        .error_for_status()
        .context("Minecraft login")?
        .json()
        .await?;

    #[derive(Deserialize)]
    struct Entitlements {
        #[serde(default)]
        items: Vec<serde_json::Value>,
    }
    let ent: Entitlements = client
        .get(MC_ENTITLEMENTS)
        .bearer_auth(&mc.access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let res = client
        .get(MC_PROFILE)
        .bearer_auth(&mc.access_token)
        .send()
        .await?;
    if res.status().as_u16() == 404 || ent.items.is_empty() {
        bail!("На этом аккаунте нет лицензии Minecraft: Java Edition");
    }
    #[derive(Deserialize)]
    struct Profile {
        id: String,
        name: String,
    }
    let profile: Profile = res.error_for_status()?.json().await?;

    Ok(Account {
        refresh_token: ms.refresh_token,
        session: Session {
            name: profile.name,
            uuid: profile.id,
            xuid: user.xid.clone().unwrap_or_else(|| "0".into()),
            access_token: mc.access_token,
            expires_at: now() + mc.expires_in,
        },
    })
}

impl Session {
    pub fn is_fresh(&self) -> bool {
        self.expires_at > now() + 300
    }
}
