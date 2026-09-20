use crate::models::AccountConfig;

pub struct SendRequest {
    pub account_email: String,
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// Compose window state. Plain-text only in v1.
pub struct ComposerState {
    accounts: Vec<String>,
    from_index: usize,
    to: String,
    subject: String,
    body: String,
}

impl ComposerState {
    pub fn new(accounts: &[AccountConfig]) -> Self {
        Self {
            accounts: accounts.iter().map(|a| a.email.clone()).collect(),
            from_index: 0,
            to: String::new(),
            subject: String::new(),
            body: String::new(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, send: &mut Option<SendRequest>) {
        egui::Grid::new("composer_grid").num_columns(2).show(ui, |ui| {
            ui.label("From");
            egui::ComboBox::from_id_salt("composer_from")
                .selected_text(
                    self.accounts
                        .get(self.from_index)
                        .map(String::as_str)
                        .unwrap_or("<no account>"),
                )
                .show_ui(ui, |ui| {
                    for (i, email) in self.accounts.iter().enumerate() {
                        ui.selectable_value(&mut self.from_index, i, email);
                    }
                });
            ui.end_row();

            ui.label("To");
            ui.text_edit_singleline(&mut self.to);
            ui.end_row();

            ui.label("Subject");
            ui.text_edit_singleline(&mut self.subject);
            ui.end_row();
        });

        ui.add_space(4.0);
        ui.label("Body");
        ui.add_sized(
            ui.available_size() - egui::vec2(0.0, 36.0),
            egui::TextEdit::multiline(&mut self.body),
        );

        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_send = !self.to.trim().is_empty()
                    && !self.subject.trim().is_empty()
                    && self.accounts.get(self.from_index).is_some();
                if ui.add_enabled(can_send, egui::Button::new("Send")).clicked() {
                    *send = Some(SendRequest {
                        account_email: self.accounts[self.from_index].clone(),
                        to: self.to.trim().to_string(),
                        subject: self.subject.trim().to_string(),
                        body: std::mem::take(&mut self.body),
                    });
                }
            });
        });
    }
}
