//! Painel de resultado — botões para salvar os três artefatos.

use crate::ui::app_state::AppState;

/// Renderiza botões de salvar (visível apenas quando phase == Done).
pub fn render(ui: &mut egui::Ui, state: &AppState) {
    let result = match &state.result {
        Some(r) => r,
        None    => return,
    };

    ui.separator();
    ui.heading("Resultado");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        save_button(ui, "💾  Salvar Transcrição", "transcript", &result.transcript_json);
        save_button(ui, "💾  Salvar Resumo",      "summary",    &result.summary_json);
        save_button(ui, "💾  Salvar Ata",         "minutes",    &result.minutes_json);
    });
}

fn save_button(ui: &mut egui::Ui, label: &str, default_name: &str, content: &str) {
    if ui.button(label).clicked() {
        save_json(default_name, content);
    }
}

fn save_json(default_name: &str, content: &str) {
    let path = rfd::FileDialog::new()
        .set_file_name(&format!("{default_name}.json"))
        .add_filter("JSON", &["json"])
        .save_file();

    if let Some(p) = path {
        if let Err(e) = std::fs::write(&p, content) {
            eprintln!("Erro ao salvar {}: {e}", p.display());
        }
    }
}
