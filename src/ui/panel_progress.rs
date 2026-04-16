//! Painel de progresso — consome eventos do pipeline e exibe fases + log.

use crate::ui::app_state::{AppState, PipelineEvent, PipelinePhase, SessionCost};

const PHASES: &[(&str, PipelinePhase)] = &[
    ("Conversão MP4 → WAV",           PipelinePhase::Converting),
    ("Processamento de áudio (9 filtros)", PipelinePhase::Processing),
    ("Transcrição (Azure AI Speech)", PipelinePhase::Transcribing),
    ("Resumo (Azure OpenAI)",         PipelinePhase::Summarizing),
    ("Ata de reunião (FLOW)",         PipelinePhase::GeneratingMinutes),
];

// ---------------------------------------------------------------------------
// Polling de eventos
// ---------------------------------------------------------------------------

/// Deve ser chamado no início de cada frame para drenar o canal de eventos.
pub fn poll_events(state: &mut AppState) {
    let events: Vec<PipelineEvent> = state
        .event_rx
        .as_ref()
        .map(|rx| rx.try_iter().collect())
        .unwrap_or_default();

    for ev in events {
        apply_event(state, ev);
    }
}

fn apply_event(state: &mut AppState, ev: PipelineEvent) {
    match ev {
        PipelineEvent::PhaseChanged(p) => {
            state.phase = p;
        }
        PipelineEvent::LogLine(line) => {
            state.log_lines.push(line);
            if state.log_lines.len() > 200 {
                state.log_lines.remove(0);
            }
        }
        PipelineEvent::AudioDuration(mins) => {
            let cost = mins / 60.0 * state.config.price_speech_per_hour;
            state.session_cost.audio_minutes   = mins;
            state.session_cost.speech_cost_usd = cost;
        }
        PipelineEvent::StepCostReady(sc) => {
            state.session_cost.total_tokens   += sc.total_tokens;
            state.session_cost.total_cost_usd += sc.cost_usd;
            state.session_cost.total_cost_usd += state.session_cost.speech_cost_usd
                * (state.session_cost.steps.is_empty() as u8 as f64); // soma speech uma vez
            state.session_cost.steps.push(sc);
        }
        PipelineEvent::Done(result) => {
            state.phase  = PipelinePhase::Done;
            state.result = Some(result);
        }
        PipelineEvent::Error(msg) => {
            state.phase = PipelinePhase::Error(msg.clone());
            state.log_lines.push(format!("✗ {msg}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Renderização
// ---------------------------------------------------------------------------

/// Renderiza lista de fases, barra de progresso e log.
pub fn render(ui: &mut egui::Ui, state: &AppState) {
    ui.heading("Progresso");
    ui.add_space(4.0);

    render_phases(ui, &state.phase);
    render_progress_bar(ui, &state.phase);
    render_log(ui, &state.log_lines);

    if let PipelinePhase::Error(msg) = &state.phase {
        ui.add_space(4.0);
        ui.colored_label(egui::Color32::RED, format!("✗ {msg}"));
    }
}

fn render_phases(ui: &mut egui::Ui, current: &PipelinePhase) {
    let current_idx = current.step_index().unwrap_or(0);
    for (i, (label, _)) in PHASES.iter().enumerate() {
        let (icon, color) = if matches!(current, PipelinePhase::Done) || i < current_idx {
            ("✓", egui::Color32::GREEN)
        } else if i == current_idx {
            ("⏳", egui::Color32::YELLOW)
        } else {
            ("○", egui::Color32::GRAY)
        };
        ui.colored_label(color, format!("{icon} {label}"));
    }
    ui.add_space(4.0);
}

fn render_progress_bar(ui: &mut egui::Ui, current: &PipelinePhase) {
    let progress = match current.step_index() {
        Some(i) => i as f32 / PHASES.len() as f32,
        None if matches!(current, PipelinePhase::Done) => 1.0,
        _ => 0.0,
    };
    ui.add(egui::ProgressBar::new(progress).show_percentage());
    ui.add_space(4.0);
}

fn render_log(ui: &mut egui::Ui, lines: &[String]) {
    let height = 120.0;
    egui::ScrollArea::vertical()
        .max_height(height)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for line in lines {
                ui.label(egui::RichText::new(line).monospace().size(11.0));
            }
        });
}

// ---------------------------------------------------------------------------
// Helpers para recalcular custo total de sessão
// ---------------------------------------------------------------------------
#[allow(dead_code)]
fn recalc_total(cost: &mut SessionCost) {
    let llm_total: f64 = cost.steps.iter().map(|s| s.cost_usd).sum();
    cost.total_tokens   = cost.steps.iter().map(|s| s.total_tokens).sum();
    cost.total_cost_usd = cost.speech_cost_usd + llm_total;
}
