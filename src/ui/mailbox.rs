use crate::models::MessageSummary;
use crate::ui::{AirMailApp, View};

pub fn show_accounts_panel(app: &mut AirMailApp, ui: &mut egui::Ui) {
    ui.heading("AirMail");
    ui.separator();

    let unified_selected = matches!(app.view, View::Unified);
    if ui
        .selectable_label(unified_selected, "Unified inbox")
        .clicked()
    {
        app.view = View::Unified;
        app.detail = None;
        app.refresh_summaries();
    }
    ui.separator();

    let cache = app.folders_cache.clone();
    for (email, folders) in &cache {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(email).strong());
            if ui.small_button("x").on_hover_text("Remove account").clicked()
                && let Err(e) = remove_account(app, email)
            {
                app.status = format!("removing account failed: {e:#}");
            }
        });
        for folder in folders {
            let selected = matches!(app.view, View::Folder(id, _) if id == folder.id);
            if ui.selectable_label(selected, &folder.name).clicked() {
                app.view = View::Folder(folder.id, folder.name.clone());
                app.detail = None;
                app.refresh_summaries();
            }
        }
        ui.add_space(4.0);
    }
}

fn remove_account(app: &mut AirMailApp, email: &str) -> anyhow::Result<()> {
    config_delete(email)?;
    app.db.remove_account(email)?;
    if app.accounts.iter().all(|a| a.email != email) {
        app.view = View::Unified;
    }
    app.restart_sync();
    app.refresh_folders();
    app.refresh_summaries();
    app.status = format!("account {email} removed");
    Ok(())
}

fn config_delete(email: &str) -> anyhow::Result<()> {
    crate::config::delete_account(email)
}

pub fn show_message_list(app: &mut AirMailApp, ui: &mut egui::Ui) {
    ui.heading(match &app.view {
        View::Unified => "All accounts".to_string(),
        View::Folder(_, name) => name.clone(),
    });
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        let summaries: Vec<MessageSummary> = app.summaries.clone();
        for summary in summaries {
            let date = summary
                .date
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
            let text = egui::RichText::new(format!(
                "{}\n{} — {}",
                truncate(&summary.subject, 60),
                truncate(&summary.from, 40),
                date
            ));
            let text = if summary.seen { text } else { text.strong() };
            if ui
                .selectable_label(app.detail_is(&summary.id), text)
                .clicked()
            {
                open_message(app, summary.id);
            }
        }
    });
}

pub fn show_reading_pane(app: &mut AirMailApp, ui: &mut egui::Ui) {
    match app.detail.clone() {
        Some(detail) => {
            ui.horizontal(|ui| {
                ui.heading(truncate(&detail.summary.subject, 90));
            });
            ui.label(egui::RichText::new(format!("From: {}", detail.summary.from)).weak());
            ui.label(egui::RichText::new(format!("To: {}", detail.to)).weak());
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                let body = if detail.body_text.is_empty() {
                    "[HTML-only message — plain text preview unavailable]"
                } else {
                    &detail.body_text
                };
                ui.label(body);
            });
        }
        None => {
            ui.centered_and_justified(|ui| {
                ui.label("Select a message");
            });
        }
    }
}

fn open_message(app: &mut AirMailApp, id: i64) {
    match app.db.message_detail(id) {
        Ok(Some(detail)) => {
            app.detail = Some(detail);
            if let Err(e) = app.db.set_seen(id, true) {
                app.status = format!("could not mark seen: {e:#}");
            }
            app.refresh_summaries_keep_selection();
        }
        Ok(None) => app.status = "message no longer exists".to_string(),
        Err(e) => app.status = format!("loading message failed: {e:#}"),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
    }
}
