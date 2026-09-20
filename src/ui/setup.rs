use anyhow::{Context, Result};

use crate::models::AccountConfig;

/// Modal form for adding a new account. Collects connection settings and the
/// password (kept only in memory until saved to the keyring).
#[derive(Debug, Clone)]
pub struct SetupDialog {
    email: String,
    display_name: String,
    imap_host: String,
    imap_port: String,
    smtp_host: String,
    smtp_port: String,
    password: String,
    error: String,
    wants_save: bool,
}

impl Default for SetupDialog {
    fn default() -> Self {
        Self {
            email: String::new(),
            display_name: String::new(),
            imap_host: "imap.gmail.com".to_string(),
            imap_port: "993".to_string(),
            smtp_host: "smtp.gmail.com".to_string(),
            smtp_port: "465".to_string(),
            password: String::new(),
            error: String::new(),
            wants_save: false,
        }
    }
}

impl SetupDialog {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("setup_grid").num_columns(2).show(ui, |ui| {
            ui.label("Email");
            ui.text_edit_singleline(&mut self.email);
            ui.end_row();

            ui.label("Display name");
            ui.text_edit_singleline(&mut self.display_name);
            ui.end_row();

            ui.label("IMAP host");
            ui.text_edit_singleline(&mut self.imap_host);
            ui.end_row();

            ui.label("IMAP port");
            ui.text_edit_singleline(&mut self.imap_port);
            ui.end_row();

            ui.label("SMTP host");
            ui.text_edit_singleline(&mut self.smtp_host);
            ui.end_row();

            ui.label("SMTP port");
            ui.text_edit_singleline(&mut self.smtp_port);
            ui.end_row();

            ui.label("Password");
            ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
            ui.end_row();
        });

        if !self.error.is_empty() {
            ui.colored_label(egui::Color32::RED, &self.error);
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Presets:").clicked() {
                // no-op; presets are the two buttons below
            }
            if ui.button("Gmail").clicked() {
                self.imap_host = "imap.gmail.com".into();
                self.imap_port = "993".into();
                self.smtp_host = "smtp.gmail.com".into();
                self.smtp_port = "465".into();
            }
            if ui.button("Outlook").clicked() {
                self.imap_host = "outlook.office365.com".into();
                self.imap_port = "993".into();
                self.smtp_host = "smtp.office365.com".into();
                self.smtp_port = "587".into();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Save").clicked() {
                    if let Err(e) = self.validate() {
                        self.error = format!("{e:#}");
                    } else {
                        self.wants_save = true;
                    }
                }
            });
        });
        ui.label(
            egui::RichText::new(
                "Tip: Gmail needs an app password (Accounts → Security → 2-Step → App passwords).",
            )
            .weak()
            .small(),
        );
    }

    fn validate(&self) -> Result<()> {
        if !self.email.contains('@') {
            anyhow::bail!("enter a valid email address");
        }
        self.imap_port.parse::<u16>().context("IMAP port must be a number")?;
        self.smtp_port.parse::<u16>().context("SMTP port must be a number")?;
        if self.imap_host.is_empty() || self.smtp_host.is_empty() {
            anyhow::bail!("IMAP and SMTP hosts are required");
        }
        if self.password.is_empty() {
            anyhow::bail!("password is required");
        }
        Ok(())
    }

    pub fn wants_save(&self) -> bool {
        self.wants_save
    }

    /// Convert the form into a persisted config + keyring password.
    pub fn save(self) -> Result<(AccountConfig, String)> {
        let display_name = if self.display_name.trim().is_empty() {
            None
        } else {
            Some(self.display_name.trim().to_string())
        };
        let cfg = AccountConfig {
            email: self.email.trim().to_string(),
            display_name,
            username: None,
            imap_host: self.imap_host.trim().to_string(),
            imap_port: self.imap_port.parse().context("IMAP port")?,
            smtp_host: self.smtp_host.trim().to_string(),
            smtp_port: self.smtp_port.parse().context("SMTP port")?,
        };
        Ok((cfg, self.password))
    }
}
