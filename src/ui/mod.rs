//! Módulo de interface gráfica nativa (egui/eframe).

pub mod app_state;
pub mod pipeline_runner;
pub mod panel_config;
pub mod panel_files;
pub mod panel_progress;
pub mod panel_result;
pub mod panel_costs;

use std::path::PathBuf;
use std::sync::mpsc;

use app_state::{AppState, PipelinePhase};

// ---------------------------------------------------------------------------
// Struct principal
// ---------------------------------------------------------------------------

pub struct App {
    state: AppState,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>, temp_dir: PathBuf) -> Self {
        Self { state: AppState::new(temp_dir) }
    }
}

// ---------------------------------------------------------------------------
// Loop de eventos
// ---------------------------------------------------------------------------

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drena eventos do pipeline a cada frame
        panel_progress::poll_events(&mut self.state);

        // Solicita repaint contínuo enquanto o pipeline está rodando
        if !matches!(
            self.state.phase,
            PipelinePhase::Idle | PipelinePhase::Done | PipelinePhase::Error(_)
        ) {
            ctx.request_repaint();
        }

        render_top_bar(ctx, &mut self.state);
        render_central(ctx, &mut self.state);
        render_config_modal(ctx, &mut self.state);
        render_costs_modal(ctx, &mut self.state);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = std::fs::remove_dir_all(&self.state.temp_dir);
    }
}

// ---------------------------------------------------------------------------
// Barra superior
// ---------------------------------------------------------------------------

fn render_top_bar(ctx: &egui::Context, state: &mut AppState) {
    egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.heading("rust-stt");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("💰  Custos").clicked()  { state.show_costs  = !state.show_costs;  }
                if ui.button("⚙  Config").clicked()   { state.show_config = !state.show_config; }
            });
        });
    });
}

// ---------------------------------------------------------------------------
// Painel central
// ---------------------------------------------------------------------------

fn render_central(ctx: &egui::Context, state: &mut AppState) {
    egui::CentralPanel::default().show(ctx, |ui| {
        panel_files::render(ui, state);
        ui.add_space(8.0);

        render_start_button(ui, state);

        if !matches!(state.phase, PipelinePhase::Idle) {
            ui.add_space(8.0);
            ui.separator();
            panel_progress::render(ui, state);
        }

        if matches!(state.phase, PipelinePhase::Done) {
            panel_result::render(ui, state);
        }
    });
}

fn render_start_button(ui: &mut egui::Ui, state: &mut AppState) {
    let enabled = state.can_start();

    ui.add_enabled_ui(enabled, |ui| {
        if ui.button("▶  Iniciar pipeline").clicked() {
            start_pipeline(state);
        }
    });

    if !enabled && state.mp4_path.is_none() {
        ui.colored_label(egui::Color32::GRAY, "Selecione o arquivo MP4 e o VTT para continuar.");
    }
}

// ---------------------------------------------------------------------------
// Modais
// ---------------------------------------------------------------------------

fn render_config_modal(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_config { return; }
    let mut open = state.show_config;
    egui::Window::new("⚙  Configuração")
        .open(&mut open)
        .resizable(true)
        .default_width(480.0)
        .show(ctx, |ui| {
            if panel_config::render(ui, &mut state.config) {
                state.show_config = false;
            }
        });
    state.show_config = open;
}

fn render_costs_modal(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_costs { return; }
    let mut open = state.show_costs;
    egui::Window::new("💰  Custos")
        .open(&mut open)
        .resizable(true)
        .default_width(520.0)
        .show(ctx, |ui| {
            panel_costs::render(ui, state);
        });
    state.show_costs = open;
}

// ---------------------------------------------------------------------------
// Lançamento do pipeline
// ---------------------------------------------------------------------------

fn start_pipeline(state: &mut AppState) {
    state.reset_for_run();
    state.phase = PipelinePhase::Converting;

    let (tx, rx) = mpsc::channel();
    state.event_rx = Some(rx);

    pipeline_runner::run(
        state.mp4_path.clone().expect("mp4 selecionado"),
        state.vtt_path.clone().expect("vtt selecionado"),
        state.temp_dir.clone(),
        state.config.clone(),
        tx,
    );
}
