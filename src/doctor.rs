use anyhow::{Context, Result};

use crate::models::AccountConfig;

/// Connectivity check for every configured account (`airmail --doctor` / `imlazy doctor`).
pub async fn run() -> Result<()> {
    let accounts: Vec<AccountConfig> = crate::config::load_accounts()?;
    if accounts.is_empty() {
        println!("No accounts configured yet.");
        return Ok(());
    }
    let mut failures = 0usize;
    for cfg in &accounts {
        print!("{} ... ", cfg.email);
        match check(cfg).await {
            Ok(report) => println!("OK ({report})"),
            Err(e) => {
                failures += 1;
                println!("FAILED\n    {e:#}");
            }
        }
    }
    if failures > 0 {
        anyhow::bail!("{failures} account(s) failed the check");
    }
    Ok(())
}

async fn check(cfg: &AccountConfig) -> Result<String> {
    let password =
        crate::config::get_password(&cfg.email).context("add the account through the GUI first")?;

    let mut session = crate::sync::imap::connect(cfg, &password).await?;
    let folders = crate::sync::imap::list_folders(&mut session).await?;
    let _ = session.logout().await;

    crate::smtp::check_account(cfg, &password).await?;

    Ok(format!(
        "IMAP login OK, {} folder(s); SMTP auth OK",
        folders.len()
    ))
}
