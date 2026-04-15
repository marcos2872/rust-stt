//! Binário `transcribe` — transcreve um arquivo de áudio via Azure AI Speech.
//!
//! Usa a Fast Transcription API com diarização por falante, timestamps e
//! estimativa de custo por hora de áudio.
//!
//! # Uso
//! ```sh
//! cargo run --bin transcribe -- <caminho_do_audio.wav>
//! ```

use std::path::Path;
use std::process;

// Pricing: Azure AI Speech Fast Transcription (standard tier)
// https://azure.microsoft.com/pricing/details/cognitive-services/speech-services/
const SPEECH_PRICE_PER_HOUR: f64 = 1.00; // USD / hora de áudio

fn main() {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Uso: {} <caminho_do_audio.wav>", args[0]);
        eprintln!("");
        eprintln!("Exemplo:");
        eprintln!("  cargo run --bin transcribe -- temp/audio.wav");
        process::exit(1);
    }

    match run(Path::new(&args[1])) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("\n✗ Erro: {e}");
            process::exit(1);
        }
    }
}

fn run(audio_path: &Path) -> Result<(), String> {
    // ── Validação ─────────────────────────────────────────────────────────
    if !audio_path.exists() {
        return Err(format!("Arquivo não encontrado: {}", audio_path.display()));
    }

    // ── Duração e tamanho ─────────────────────────────────────────────────
    let duration_secs = audio_duration_secs(audio_path);
    let duration_mins = duration_secs.map(|s| s / 60.0);
    let file_size_mb  = audio_path.metadata()
        .map(|m| m.len() as f64 / 1_048_576.0)
        .unwrap_or(0.0);

    // ── Configuração ──────────────────────────────────────────────────────
    let config = rust_stt::transcriber::TranscriptionConfig::from_env()
        .map_err(|e| e.to_string())?;

    println!("Transcrição de áudio — Azure AI Speech");
    println!("  Arquivo    : {}", audio_path.display());
    println!("  Tamanho    : {file_size_mb:.1} MB");
    if let Some(mins) = duration_mins {
        let m = mins as u64;
        let s = ((mins - m as f64) * 60.0) as u64;
        println!("  Duração    : {m:02}:{s:02}");
    }
    println!("  Endpoint   : {}", config.speech_endpoint);
    println!("  Idioma     : {}", config.language.as_deref().unwrap_or("pt-BR"));
    println!("  Max speakers: {}", config.max_speakers);
    println!();
    println!("Enviando para Azure AI Speech...");

    // ── Transcrição ───────────────────────────────────────────────────────
    let start  = std::time::Instant::now();
    let result = rust_stt::transcriber::transcribe(audio_path, &config)
        .map_err(|e| e.to_string())?;
    let elapsed = start.elapsed();

    // ── Estatísticas ──────────────────────────────────────────────────────
    let speakers: std::collections::HashSet<String> = result.segments
        .iter()
        .filter_map(|s| s.speaker.clone())
        .collect();

    let mut speaker_list: Vec<&String> = speakers.iter().collect();
    speaker_list.sort();

    println!("Tempo de resposta : {:.1}s", elapsed.as_secs_f64());

    if let Some(dur_ms) = result.duration_ms {
        let total_s = dur_ms / 1000;
        println!("Duração (API)     : {:02}:{:02}", total_s / 60, total_s % 60);
    }

    println!("Segmentos         : {}", result.segments.len());

    if speakers.is_empty() {
        println!("Falantes          : não detectados");
    } else {
        println!("Falantes          : {} detectado(s)", speakers.len());
        for sp in &speaker_list {
            let count = result.segments.iter().filter(|s| s.speaker.as_ref() == Some(sp)).count();
            println!("  {} — {} segmento(s)", sp, count);
        }
    }

    // ── Custo estimado ────────────────────────────────────────────────────
    if let Some(mins) = duration_mins {
        let hours = mins / 60.0;
        let cost  = hours * SPEECH_PRICE_PER_HOUR;
        println!();
        println!("─── Custo estimado (Azure AI Speech) ──────────────────");
        println!("  {:.2} min × $1.00/hora = ${:.6}", mins, cost);
        println!("  (preço padrão: $1.00/hora de áudio transcrito)");
    }

    // ── Transcrição ───────────────────────────────────────────────────────
    println!();
    println!("{}", "─".repeat(60));
    println!();
    println!("{}", result.format_output());
    println!();
    println!("{}", "─".repeat(60));

    // ── Salvar JSON ───────────────────────────────────────────────────────
    let json_path = build_output_path(audio_path);
    std::fs::write(&json_path, result.to_json())
        .map_err(|e| format!("Falha ao salvar JSON: {e}"))?;

    println!();
    println!("✓ JSON salvo em: {}", json_path.display());

    Ok(())
}

fn audio_duration_secs(path: &Path) -> Option<f64> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",            "error",
            "-show_entries", "format=duration",
            "-of",           "default=noprint_wrappers=1:nokey=1",
            &path.to_string_lossy(),
        ])
        .output().ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse::<f64>().ok()
}

fn build_output_path(audio_path: &Path) -> std::path::PathBuf {
    let stem = audio_path.file_stem().and_then(|s| s.to_str()).unwrap_or("audio");
    let dir  = audio_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(format!("{stem}_transcript.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_output_path_deriva_json_correto() {
        let p = build_output_path(Path::new("temp/audio.wav"));
        assert_eq!(p.to_str().unwrap(), "temp/audio_transcript.json");
    }

    #[test]
    fn custo_por_hora_calculado_corretamente() {
        // 60 min = 1 hora = $1.00
        let cost = (60.0_f64 / 60.0) * SPEECH_PRICE_PER_HOUR;
        assert!((cost - 1.00).abs() < 1e-9);
        // 30 min = $0.50
        let cost = (30.0_f64 / 60.0) * SPEECH_PRICE_PER_HOUR;
        assert!((cost - 0.50).abs() < 1e-9);
    }
}
