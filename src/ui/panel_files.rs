//! Painel de seleção de arquivos MP4 e VTT.

use crate::ui::app_state::AppState;

/// Renderiza os dois seletores de arquivo.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Arquivos");
    ui.add_space(6.0);

    egui::Grid::new("files_grid")
        .num_columns(3)
        .spacing([8.0, 8.0])
        .show(ui, |ui| {
            // MP4
            ui.label("📹 Vídeo MP4");
            if ui.button("Selecionar…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("MP4", &["mp4", "MP4"])
                    .pick_file()
                {
                    state.mp4_path = Some(p);
                }
            }
            file_label(ui, state.mp4_path.as_deref());
            ui.end_row();

            // VTT
            ui.label("📄 Legenda VTT");
            if ui.button("Selecionar…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("VTT", &["vtt", "VTT"])
                    .pick_file()
                {
                    state.vtt_path = Some(p);
                }
            }
            file_label(ui, state.vtt_path.as_deref());
            ui.end_row();
        });
}

fn file_label(ui: &mut egui::Ui, path: Option<&std::path::Path>) {
    match path.and_then(|p| p.file_name()).and_then(|n| n.to_str()) {
        Some(name) => { ui.colored_label(egui::Color32::GREEN, format!("✓ {name}")); }
        None       => { ui.colored_label(egui::Color32::GRAY,  "não selecionado"); }
    }
}
