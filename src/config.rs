use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;

use crate::models::AccountConfig;

const SERVICE: &str = "dev.airmail";

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("dev", "AirMail", "airmail").context("cannot determine config directory")
}

pub fn accounts_dir() -> Result<PathBuf> {
    let dir = project_dirs()?.config_dir().join("accounts");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn db_path() -> Result<PathBuf> {
    let dir = project_dirs()?.data_dir().to_path_buf();
    fs::create_dir_all(&dir)?;
    Ok(dir.join("airmail.db"))
}

fn account_file_path(email: &str) -> Result<PathBuf> {
    // sanitize: keep only safe filename chars
    let safe: String = email
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    Ok(accounts_dir()?.join(format!("{safe}.toml")))
}

pub fn load_accounts() -> Result<Vec<AccountConfig>> {
    let mut accounts = Vec::new();
    let dir = accounts_dir()?;
    for entry in fs::read_dir(&dir)?.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text = fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            match toml::from_str::<AccountConfig>(&text) {
                Ok(cfg) => accounts.push(cfg),
                Err(e) => tracing::warn!("skipping invalid account file {}: {e}", path.display()),
            }
        }
    }
    accounts.sort_by(|a, b| a.email.cmp(&b.email));
    Ok(accounts)
}

pub fn save_account(cfg: &AccountConfig) -> Result<()> {
    let path = account_file_path(&cfg.email)?;
    let text = toml::to_string_pretty(cfg)?;
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

pub fn delete_account(email: &str) -> Result<()> {
    let path = account_file_path(email)?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    let _ = keyring::Entry::new(SERVICE, email).and_then(|e| e.delete_credential());
    Ok(())
}

pub fn store_password(email: &str, password: &str) -> Result<()> {
    keyring::Entry::new(SERVICE, email)
        .and_then(|e| e.set_password(password))
        .context("storing password in the OS keyring (is a secret-service provider available?)")
}

pub fn get_password(email: &str) -> Result<String> {
    keyring::Entry::new(SERVICE, email)
        .and_then(|e| e.get_password())
        .with_context(|| format!("no stored password for {email}"))
}
