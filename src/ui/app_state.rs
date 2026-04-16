//! Tipos de estado compartilhados entre todos os painéis da UI.

use std::path::PathBuf;
use std::sync::mpsc;

// ---------------------------------------------------------------------------
// Fases do pipeline
// ---------------------------------------------------------------------------

/// Fases exibidas no painel de progresso.
#[derive(Debug, Clone, PartialEq)]
pub enum PipelinePhase {
    Idle,
    Converting,
    Processing,
    Transcribing,
    Summarizing,
    GeneratingMinutes,
    Done,
    Error(String),
}

impl PipelinePhase {
    /// Índice numérico (0–4) para o cálculo da barra de progresso.
    pub fn step_index(&self) -> Option<usize> {
        match self {
            Self::Converting        => Some(0),
            Self::Processing        => Some(1),
            Self::Transcribing      => Some(2),
            Self::Summarizing       => Some(3),
            Self::GeneratingMinutes => Some(4),
            Self::Done              => Some(5),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Eventos do pipeline (thread → UI)
// ---------------------------------------------------------------------------

pub enum PipelineEvent {
    PhaseChanged(PipelinePhase),
    LogLine(String),
    StepCostReady(StepCost),
    AudioDuration(f64),   // minutos de áudio
    Done(PipelineResult),
    Error(String),
}

// ---------------------------------------------------------------------------
// Custo de etapas LLM
// ---------------------------------------------------------------------------

pub struct StepCost {
    pub label:             String,
    pub prompt_tokens:     u64,
    pub completion_tokens: u64,
    pub total_tokens:      u64,
    pub cost_usd:          f64,
}

#[derive(Default)]
pub struct SessionCost {
    pub audio_minutes:   f64,
    pub speech_cost_usd: f64,
    pub steps:           Vec<StepCost>,
    pub total_tokens:    u64,
    pub total_cost_usd:  f64,
}

// ---------------------------------------------------------------------------
// Resultado final
// ---------------------------------------------------------------------------

pub struct PipelineResult {
    pub transcript_json: String,
    pub summary_json:    String,
    pub minutes_json:    String,
}

// ---------------------------------------------------------------------------
// Configuração persistida
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct AppConfig {
    pub openai_endpoint:       String,
    pub openai_key:            String,
    pub openai_deployment:     String,
    pub openai_version:        String,
    pub speech_endpoint:       String,
    pub speech_key:            String,
    pub speech_language:       String,
    pub speech_max_speakers:   u32,
    pub price_input_per_1m:    f64,
    pub price_output_per_1m:   f64,
    pub price_speech_per_hour: f64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            openai_endpoint:       String::new(),
            openai_key:            String::new(),
            openai_deployment:     "gpt-5.4-mini".to_string(),
            openai_version:        "2025-01-01-preview".to_string(),
            speech_endpoint:       String::new(),
            speech_key:            String::new(),
            speech_language:       "pt-BR".to_string(),
            speech_max_speakers:   10,
            price_input_per_1m:    0.750,
            price_output_per_1m:   4.500,
            price_speech_per_hour: 1.000,
        }
    }
}

// ---------------------------------------------------------------------------
// Histórico de custos
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    pub date:                         String,
    pub meeting_title:                String,
    pub audio_minutes:                f64,
    pub speech_cost_usd:              f64,
    pub summarizer_prompt_tokens:     u64,
    pub summarizer_completion_tokens: u64,
    pub summarizer_cost_usd:          f64,
    pub minutes_prompt_tokens:        u64,
    pub minutes_completion_tokens:    u64,
    pub minutes_cost_usd:             f64,
    pub total_tokens:                 u64,
    pub total_cost_usd:               f64,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct CostHistory {
    pub sessions: Vec<HistoryEntry>,
}

// ---------------------------------------------------------------------------
// Estado global do app
// ---------------------------------------------------------------------------

pub struct AppState {
    pub mp4_path:    Option<PathBuf>,
    pub vtt_path:    Option<PathBuf>,
    pub config:      AppConfig,
    pub phase:       PipelinePhase,
    pub log_lines:   Vec<String>,
    pub session_cost: SessionCost,
    pub history:     CostHistory,
    pub result:      Option<PipelineResult>,
    pub temp_dir:    PathBuf,
    pub show_config: bool,
    pub show_costs:  bool,
    pub event_rx:    Option<mpsc::Receiver<PipelineEvent>>,
}

impl AppState {
    pub fn new(temp_dir: PathBuf) -> Self {
        let config  = crate::ui::panel_config::load_config();
        let history = crate::ui::panel_costs::load_history();
        Self {
            mp4_path: None,
            vtt_path: None,
            config,
            phase: PipelinePhase::Idle,
            log_lines: Vec::new(),
            session_cost: SessionCost::default(),
            history,
            result: None,
            temp_dir,
            show_config: false,
            show_costs:  false,
            event_rx: None,
        }
    }

    /// Retorna true se mp4 e vtt estão selecionados e a fase é Idle ou Error.
    pub fn can_start(&self) -> bool {
        self.mp4_path.is_some()
            && self.vtt_path.is_some()
            && matches!(self.phase, PipelinePhase::Idle | PipelinePhase::Error(_))
    }

    /// Reinicia o estado para uma nova execução.
    pub fn reset_for_run(&mut self) {
        self.phase        = PipelinePhase::Idle;
        self.log_lines    = Vec::new();
        self.session_cost = SessionCost::default();
        self.result       = None;
        self.event_rx     = None;
    }
}
