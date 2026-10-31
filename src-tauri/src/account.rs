//! Stored Microsoft account. The refresh token goes to the OS keychain
//! (Keychain / Credential Manager / Secret Service); if there is none
//! (minimal Linux), to account.json in the launcher folder, readable only by the user.

use anyhow::Result;
use minehouse_core::auth::Account;
use minehouse_core::paths::Paths;

const SERVICE: &str = "pro.minehouse.launcher";
const USER: &str = "microsoft";

fn entry() -> Option<keyring::Entry> {
    keyring::Entry::new(SERVICE, USER).ok()
}

pub fn load(paths: &Paths) -> Option<Account> {
    if let Some(Ok(json)) = entry().map(|e| e.get_password()) {
        if let Ok(a) = serde_json::from_str(&json) {
            return Some(a);
        }
    }
    let bytes = std::fs::read(paths.account_file()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save(paths: &Paths, account: &Account) -> Result<()> {
    let json = serde_json::to_string(account)?;
    if let Some(e) = entry() {
        if e.set_password(&json).is_ok() {
            let _ = std::fs::remove_file(paths.account_file());
            return Ok(());
        }
    }
    std::fs::create_dir_all(&paths.root)?;
    let file = paths.account_file();
    std::fs::write(&file, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn clear(paths: &Paths) {
    if let Some(e) = entry() {
        let _ = e.delete_credential();
    }
    let _ = std::fs::remove_file(paths.account_file());
}
