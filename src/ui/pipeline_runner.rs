//! Executa o pipeline completo em thread separada, enviando eventos para a UI.

use std::path::PathBuf;
use std::sync::mpsc;

use crate::ui::app_state::{AppConfig, PipelineEvent, PipelinePhase, PipelineResult, StepCost};

// ---------------------------------------------------------------------------
// Ponto de entrada público
// ---------------------------------------------------------------------------

/// Lança o pipeline em thread dedicada.
pub fn run(
    mp4_path: PathBuf,
    vtt_path:  PathBuf,
    temp_dir:  PathBuf,
    config:    AppConfig,
    tx:        mpsc::Sender<PipelineEvent>,
) {
    std::thread::spawn(move || {
        if let Err(e) = run_inner(mp4_path, vtt_path, temp_dir, config, &tx) {
            let _ = tx.send(PipelineEvent::Error(e));
        }
    });
}

// ---------------------------------------------------------------------------
// Pipeline interno
// ---------------------------------------------------------------------------

fn run_inner(
    mp4_path: PathBuf,
    vtt_path:  PathBuf,
    temp_dir:  PathBuf,
    config:    AppConfig,
    tx:        &mpsc::Sender<PipelineEvent>,
) -> Result<(), String> {
    // 1. Conversão MP4 → WAV intermediário (subdir próprio para evitar colisão de nome)
    send(tx, PipelineEvent::PhaseChanged(PipelinePhase::Converting));
    send(tx, PipelineEvent::LogLine("Convertendo MP4 → WAV...".into()));

    let intermediate_dir = temp_dir.join("intermediate");
    std::fs::create_dir_all(&intermediate_dir)
        .map_err(|e| format!("Criação de subdir: {e}"))?;

    let intermediate = crate::converter::convert_mp4_to_wav(&mp4_path, &intermediate_dir)
        .map_err(|e| format!("Conversão: {e}"))?;

    if let Some(mins) = audio_duration_mins(&intermediate) {
        send(tx, PipelineEvent::AudioDuration(mins));
        send(tx, PipelineEvent::LogLine(format!("Duração: {mins:.1} min")));
    }

    // 2. Processamento de áudio (saída em temp_dir — caminho diferente do intermediário)
    send(tx, PipelineEvent::PhaseChanged(PipelinePhase::Processing));
    send(tx, PipelineEvent::LogLine("Processando áudio (9 filtros)...".into()));

    let ap_cfg    = crate::audio_processor::AudioProcessingConfig::default();
    let processed = crate::audio_processor::process_audio(&intermediate, &temp_dir, &ap_cfg)
        .map_err(|e| format!("Processamento: {e}"))?;
    let _ = std::fs::remove_file(&intermediate);
    let _ = std::fs::remove_dir(&intermediate_dir);

    // 3. Transcrição
    send(tx, PipelineEvent::PhaseChanged(PipelinePhase::Transcribing));
    send(tx, PipelineEvent::LogLine("Transcrevendo via Azure AI Speech...".into()));

    let trans_cfg    = trans_config(&config);
    let trans_result = crate::transcriber::transcribe(&processed, &trans_cfg)
        .map_err(|e| format!("Transcrição: {e}"))?;
    let transcript_json = trans_result.to_json();

    let trans_path = temp_dir.join("transcript.json");
    std::fs::write(&trans_path, &transcript_json)
        .map_err(|e| format!("Escrita transcript: {e}"))?;
    send(tx, PipelineEvent::LogLine(
        format!("{} segmentos transcritos", trans_result.segments.len())
    ));

    // 4. Summarizer
    send(tx, PipelineEvent::PhaseChanged(PipelinePhase::Summarizing));
    send(tx, PipelineEvent::LogLine("Gerando resumo via Azure OpenAI...".into()));

    let summ_cfg    = summ_config(&config);
    let summ_result = crate::summarizer::summarize(&trans_path, &vtt_path, &summ_cfg)
        .map_err(|e| format!("Summarizer: {e}"))?;
    let summary_json = summ_result.to_json();

    send(tx, PipelineEvent::StepCostReady(step_cost_summ(&summ_result, &config)));

    let summ_path = temp_dir.join("summary.json");
    std::fs::write(&summ_path, &summary_json)
        .map_err(|e| format!("Escrita summary: {e}"))?;

    // 5. Minutes
    send(tx, PipelineEvent::PhaseChanged(PipelinePhase::GeneratingMinutes));
    send(tx, PipelineEvent::LogLine("Gerando ata via Azure OpenAI...".into()));

    let min_cfg    = min_config(&config);
    let min_result = crate::minutes::generate_minutes(&summ_path, &min_cfg)
        .map_err(|e| format!("Minutes: {e}"))?;
    let minutes_json = min_result.to_json();

    send(tx, PipelineEvent::StepCostReady(step_cost_min(&min_result, &config)));

    // 6. Limpeza e envio do resultado
    let _ = std::fs::remove_file(&trans_path);
    let _ = std::fs::remove_file(&summ_path);
    let _ = std::fs::remove_file(&processed);

    send(tx, PipelineEvent::Done(PipelineResult {
        transcript_json,
        summary_json,
        minutes_json,
    }));
    Ok(())
}

// ---------------------------------------------------------------------------
// Conversões de configuração
// ---------------------------------------------------------------------------

fn trans_config(cfg: &AppConfig) -> crate::transcriber::TranscriptionConfig {
    crate::transcriber::TranscriptionConfig {
        speech_key:      if cfg.speech_key.is_empty() { cfg.openai_key.clone() }
                         else { cfg.speech_key.clone() },
        speech_endpoint: if cfg.speech_endpoint.is_empty() { cfg.openai_endpoint.clone() }
                         else { cfg.speech_endpoint.clone() },
        language:        Some(cfg.speech_language.clone()),
        max_speakers:    cfg.speech_max_speakers,
    }
}

fn summ_config(cfg: &AppConfig) -> crate::summarizer::SummarizerConfig {
    crate::summarizer::SummarizerConfig {
        api_key:     cfg.openai_key.clone(),
        endpoint:    cfg.openai_endpoint.clone(),
        deployment:  cfg.openai_deployment.clone(),
        api_version: cfg.openai_version.clone(),
    }
}

fn min_config(cfg: &AppConfig) -> crate::minutes::MinutesConfig {
    crate::minutes::MinutesConfig {
        api_key:     cfg.openai_key.clone(),
        endpoint:    cfg.openai_endpoint.clone(),
        deployment:  cfg.openai_deployment.clone(),
        api_version: cfg.openai_version.clone(),
    }
}

// ---------------------------------------------------------------------------
// Helpers de custo
// ---------------------------------------------------------------------------

fn step_cost_summ(r: &crate::summarizer::SummaryResult, cfg: &AppConfig) -> StepCost {
    let u = &r.token_usage;
    let cost = cost_usd(u.prompt_tokens, u.completion_tokens, cfg);
    StepCost {
        label:             "Summarizer".into(),
        prompt_tokens:     u.prompt_tokens,
        completion_tokens: u.completion_tokens,
        total_tokens:      u.total_tokens,
        cost_usd:          cost,
    }
}

fn step_cost_min(r: &crate::minutes::MinutesResult, cfg: &AppConfig) -> StepCost {
    let u = &r.token_usage;
    let cost = cost_usd(u.prompt_tokens, u.completion_tokens, cfg);
    StepCost {
        label:             "Minutes".into(),
        prompt_tokens:     u.prompt_tokens,
        completion_tokens: u.completion_tokens,
        total_tokens:      u.total_tokens,
        cost_usd:          cost,
    }
}

fn cost_usd(prompt: u64, completion: u64, cfg: &AppConfig) -> f64 {
    prompt     as f64 / 1_000_000.0 * cfg.price_input_per_1m
    + completion as f64 / 1_000_000.0 * cfg.price_output_per_1m
}

fn audio_duration_mins(path: &std::path::Path) -> Option<f64> {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration",
               "-of", "default=noprint_wrappers=1:nokey=1",
               &path.to_string_lossy()])
        .output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().ok()
        .map(|s| s / 60.0)
}

fn send(tx: &mpsc::Sender<PipelineEvent>, ev: PipelineEvent) {
    let _ = tx.send(ev);
}
