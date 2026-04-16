//! Painel de configuração — credenciais Azure e preços.

use std::path::PathBuf;
use crate::ui::app_state::AppConfig;

// ---------------------------------------------------------------------------
// Persistência
// ---------------------------------------------------------------------------

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rust-stt")
        .join("config.json")
}

/// Carrega configuração do disco; retorna `AppConfig::default()` em caso de falha.
pub fn load_config() -> AppConfig {
    let path = config_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Salva configuração em `~/.config/rust-stt/config.json`.
pub fn save_config(cfg: &AppConfig) {
    let path = config_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(&path, json);
    }
}

// ---------------------------------------------------------------------------
// Renderização
// ---------------------------------------------------------------------------

/// Renderiza o modal de configuração. Chame dentro de `egui::Window`.
pub fn render(ui: &mut egui::Ui, cfg: &mut AppConfig) -> bool {
    let mut saved = false;

    egui::Grid::new("cfg_grid")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            render_azure_openai(ui, cfg);
            render_azure_speech(ui, cfg);
            render_prices(ui, cfg);
        });

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("💾  Salvar").clicked() {
            save_config(cfg);
            saved = true;
        }
    });
    saved
}

fn render_azure_openai(ui: &mut egui::Ui, cfg: &mut AppConfig) {
    ui.strong("Azure OpenAI");
    ui.end_row();

    ui.label("Endpoint");
    ui.text_edit_singleline(&mut cfg.openai_endpoint);
    ui.end_row();

    ui.label("API Key");
    ui.add(egui::TextEdit::singleline(&mut cfg.openai_key).password(true));
    ui.end_row();

    ui.label("Deployment");
    ui.text_edit_singleline(&mut cfg.openai_deployment);
    ui.end_row();

    ui.label("API Version");
    ui.text_edit_singleline(&mut cfg.openai_version);
    ui.end_row();
}

fn render_azure_speech(ui: &mut egui::Ui, cfg: &mut AppConfig) {
    ui.separator();
    ui.end_row();
    ui.strong("Azure AI Speech");
    ui.end_row();

    ui.label("Endpoint");
    ui.text_edit_singleline(&mut cfg.speech_endpoint);
    ui.end_row();

    ui.label("API Key");
    ui.add(egui::TextEdit::singleline(&mut cfg.speech_key).password(true));
    ui.end_row();

    ui.label("Idioma");
    ui.text_edit_singleline(&mut cfg.speech_language);
    ui.end_row();

    ui.label("Máx. falantes");
    let mut s = cfg.speech_max_speakers.to_string();
    if ui.text_edit_singleline(&mut s).changed() {
        if let Ok(v) = s.parse::<u32>() { cfg.speech_max_speakers = v; }
    }
    ui.end_row();
}

fn render_prices(ui: &mut egui::Ui, cfg: &mut AppConfig) {
    ui.separator();
    ui.end_row();
    ui.strong("Preços (USD / 1M tokens)");
    ui.end_row();

    price_field(ui, "Entrada (OpenAI)", &mut cfg.price_input_per_1m);
    price_field(ui, "Saída (OpenAI)",   &mut cfg.price_output_per_1m);

    ui.label("Speech (por hora)");
    let mut s = format!("{:.4}", cfg.price_speech_per_hour);
    if ui.text_edit_singleline(&mut s).changed() {
        if let Ok(v) = s.parse::<f64>() { cfg.price_speech_per_hour = v; }
    }
    ui.end_row();
}

fn price_field(ui: &mut egui::Ui, label: &str, val: &mut f64) {
    ui.label(label);
    let mut s = format!("{:.4}", val);
    if ui.text_edit_singleline(&mut s).changed() {
        if let Ok(v) = s.parse::<f64>() { *val = v; }
    }
    ui.end_row();
}
