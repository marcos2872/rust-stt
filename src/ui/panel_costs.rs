//! Painel de custos — sessão atual e histórico persistido.

use std::path::PathBuf;
use crate::ui::app_state::{AppState, CostHistory, HistoryEntry, SessionCost};

// ---------------------------------------------------------------------------
// Persistência
// ---------------------------------------------------------------------------

fn history_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rust-stt")
        .join("cost_history.json")
}

pub fn load_history() -> CostHistory {
    let path = history_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn append_session(history: &mut CostHistory, entry: HistoryEntry) {
    history.sessions.push(entry);
    let path = history_path();
    if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
    if let Ok(json) = serde_json::to_string_pretty(history) {
        let _ = std::fs::write(&path, json);
    }
}

pub fn clear_history(history: &mut CostHistory) {
    history.sessions.clear();
    let _ = std::fs::remove_file(history_path());
}

// ---------------------------------------------------------------------------
// Renderização
// ---------------------------------------------------------------------------

pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("Custos");
    ui.add_space(6.0);

    render_session(ui, &state.session_cost);

    ui.add_space(8.0);
    ui.separator();
    ui.strong("Histórico");
    ui.add_space(4.0);

    render_history(ui, &mut state.history);
}

fn render_session(ui: &mut egui::Ui, sc: &SessionCost) {
    if sc.audio_minutes > 0.0 {
        ui.label(format!(
            "Azure AI Speech: {:.1} min × $1.00/h = ${:.4}",
            sc.audio_minutes, sc.speech_cost_usd
        ));
    }
    for step in &sc.steps {
        ui.add_space(4.0);
        ui.strong(format!("Azure OpenAI — {}", step.label));
        ui.label(format!(
            "  Entrada: {:>8} tokens  (${:.6})",
            step.prompt_tokens,
            step.prompt_tokens as f64 / 1_000_000.0 * 0.750
        ));
        ui.label(format!(
            "  Saída:   {:>8} tokens  (${:.6})",
            step.completion_tokens,
            step.completion_tokens as f64 / 1_000_000.0 * 4.500
        ));
    }
    if sc.total_tokens > 0 || sc.audio_minutes > 0.0 {
        ui.add_space(4.0);
        ui.separator();
        ui.label(format!("Total tokens   : {}", sc.total_tokens));
        ui.strong(format!("Total estimado : ${:.4}", sc.total_cost_usd));
    }
}

fn render_history(ui: &mut egui::Ui, history: &mut CostHistory) {
    if history.sessions.is_empty() {
        ui.colored_label(egui::Color32::GRAY, "Nenhum registro ainda.");
        return;
    }

    egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
        render_history_table(ui, history);
    });

    render_history_stats(ui, history);
    ui.add_space(4.0);

    if ui.button("🗑  Limpar histórico").clicked() {
        clear_history(history);
    }
}

fn render_history_table(ui: &mut egui::Ui, history: &CostHistory) {
    egui::Grid::new("hist_grid")
        .num_columns(5)
        .striped(true)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.strong("Data"); ui.strong("Reunião");
            ui.strong("Áudio"); ui.strong("Tokens"); ui.strong("Custo");
            ui.end_row();
            for e in &history.sessions {
                ui.label(&e.date);
                ui.label(e.meeting_title.chars().take(24).collect::<String>());
                ui.label(format!("{:.0} min", e.audio_minutes));
                ui.label(format!("{}", e.total_tokens));
                ui.label(format!("${:.4}", e.total_cost_usd));
                ui.end_row();
            }
        });
}

fn render_history_stats(ui: &mut egui::Ui, history: &CostHistory) {
    let n = history.sessions.len() as f64;
    if n == 0.0 { return; }
    let total_tokens: u64 = history.sessions.iter().map(|e| e.total_tokens).sum();
    let total_cost: f64   = history.sessions.iter().map(|e| e.total_cost_usd).sum();
    let total_mins: f64   = history.sessions.iter().map(|e| e.audio_minutes).sum();
    ui.separator();
    ui.label(format!(
        "MÉDIA  {:.0} min · {} tokens · ${:.4}   |   TOTAL ({}) ${:.4}",
        total_mins / n, total_tokens / n as u64, total_cost / n, n as u32, total_cost
    ));
}
