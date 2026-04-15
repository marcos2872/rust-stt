//! Binário `transcribe` — transcreve um arquivo de áudio via Azure OpenAI.
//!
//! # Uso
//! ```sh
//! cargo run --bin transcribe -- <caminho_do_audio.wav>
//! ```
//!
//! # Saída
//! - Transcrição formatada impressa no stdout
//! - `<stem>_transcript.json` salvo no mesmo diretório do áudio

use std::path::Path;
use std::process;

// ---------------------------------------------------------------------------
// Tabela de preços — gpt-4o-transcribe-diarize (USD por 1 M tokens)
// Fonte: tabela oficial do modelo
// ---------------------------------------------------------------------------
const PRICE_INPUT_PER_M:  f64 = 2.50;   // USD / 1 M input tokens
const PRICE_OUTPUT_PER_M: f64 = 10.00;  // USD / 1 M output tokens
const PRICE_PER_MINUTE:   f64 = 0.006;  // USD / minuto de áudio (estimativa oficial)

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

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

    let audio_path = Path::new(&args[1]);

    match run(audio_path) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("\n✗ Erro: {e}");
            process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Pipeline principal
// ---------------------------------------------------------------------------

fn run(audio_path: &Path) -> Result<(), String> {
    // ── Validação ─────────────────────────────────────────────────────────
    if !audio_path.exists() {
        return Err(format!("Arquivo não encontrado: {}", audio_path.display()));
    }

    let ext = audio_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if !["wav", "mp3", "m4a", "flac", "ogg"].contains(&ext.as_str()) {
        eprintln!(
            "Aviso: extensão '.{ext}' pode não ser suportada — use wav, mp3 ou m4a"
        );
    }

    // ── Duração do áudio (para estimativa por minuto) ─────────────────────
    let duration_secs = audio_duration_secs(audio_path);
    let duration_mins = duration_secs.map(|s| s / 60.0);

    // ── Configuração ──────────────────────────────────────────────────────
    let config = rust_stt::transcriber::TranscriptionConfig::from_env()
        .map_err(|e| e.to_string())?;

    let file_size_mb = audio_path
        .metadata()
        .map(|m| m.len() as f64 / 1_048_576.0)
        .unwrap_or(0.0);

    println!("Transcrição de áudio via Azure OpenAI");
    println!("  Arquivo    : {}", audio_path.display());
    println!("  Tamanho    : {file_size_mb:.1} MB");
    if let Some(mins) = duration_mins {
        let m = mins as u64;
        let s = ((mins - m as f64) * 60.0) as u64;
        println!("  Duração    : {m:02}:{s:02}");
    }
    println!("  Endpoint   : {}", config.endpoint);
    println!("  Deployment : {}", config.deployment);
    println!("  API version: {}", config.api_version);
    if let Some(lang) = &config.language {
        println!("  Idioma     : {lang}");
    }
    println!();
    println!("Enviando para Azure OpenAI...");

    // ── Transcrição ───────────────────────────────────────────────────────
    let start  = std::time::Instant::now();
    let result = rust_stt::transcriber::transcribe(audio_path, &config)
        .map_err(|e| e.to_string())?;
    let elapsed = start.elapsed();

    // ── Exibir resultado ──────────────────────────────────────────────────
    let has_speakers = result.segments.iter().any(|s| s.speaker.is_some());

    println!("Tempo de resposta  : {:.1}s", elapsed.as_secs_f64());
    println!(
        "Falantes detectados: {}",
        if has_speakers { "sim" } else { "não (texto único)" }
    );
    if has_speakers {
        println!("Segmentos          : {}", result.segments.len());
    }

    // ── Tokens e custo ────────────────────────────────────────────────────
    if let Some(usage) = &result.usage {
        let input  = usage.input_tokens.unwrap_or(0);
        let output = usage.output_tokens.unwrap_or(0);
        let total  = usage.total_tokens.unwrap_or(0);

        println!();
        println!("─── Tokens ───────────────────────────────────────────");
        println!("  Total   : {total}");
        println!("  Input   : {input}  (áudio → ${:.6})", token_cost(input, PRICE_INPUT_PER_M));
        println!("  Output  : {output}  (texto → ${:.6})", token_cost(output, PRICE_OUTPUT_PER_M));

        let cost_by_tokens = token_cost(input, PRICE_INPUT_PER_M)
            + token_cost(output, PRICE_OUTPUT_PER_M);

        println!();
        println!("─── Custo estimado ({}) ──────────────────────────", config.deployment);
        println!(
            "  Por tokens    : ${:.6}  ({input} in × $2.50/M + {output} out × $10.00/M)",
            cost_by_tokens
        );
        if let Some(mins) = duration_mins {
            let cost_by_min = mins * PRICE_PER_MINUTE;
            println!(
                "  Por minuto    : ${:.6}  ({:.2} min × $0.006/min)",
                cost_by_min, mins
            );
            let avg = (cost_by_tokens + cost_by_min) / 2.0;
            println!("  Média         : ${:.6}", avg);
        } else {
            println!("  Por minuto    : N/A (duração não disponível)");
            println!("  Média         : ${:.6}", cost_by_tokens);
        }
    }

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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Custo em USD para `n` tokens ao preço de `price_per_million`.
fn token_cost(n: u64, price_per_million: f64) -> f64 {
    n as f64 / 1_000_000.0 * price_per_million
}

/// Duração do arquivo de áudio em segundos via `ffprobe`.
/// Retorna `None` se ffprobe não estiver disponível ou falhar.
fn audio_duration_secs(path: &Path) -> Option<f64> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",            "error",
            "-show_entries", "format=duration",
            "-of",           "default=noprint_wrappers=1:nokey=1",
            &path.to_string_lossy(),
        ])
        .output()
        .ok()?;

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .ok()
}

/// `temp/audio.wav` → `temp/audio_transcript.json`
fn build_output_path(audio_path: &Path) -> std::path::PathBuf {
    let stem = audio_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    let dir = audio_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(format!("{stem}_transcript.json"))
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_cost_calcula_corretamente() {
        // 1 M tokens de input = $2.50
        assert!((token_cost(1_000_000, 2.50) - 2.50).abs() < 1e-9);
        // 1 M tokens de output = $10.00
        assert!((token_cost(1_000_000, 10.00) - 10.00).abs() < 1e-9);
        // 257 input tokens (caso real do teste)
        let custo = token_cost(257, 2.50);
        assert!(custo > 0.0 && custo < 0.001);
    }

    #[test]
    fn token_cost_zero_tokens() {
        assert_eq!(token_cost(0, 2.50), 0.0);
    }

    #[test]
    fn build_output_path_deriva_json_correto() {
        let p = build_output_path(Path::new("temp/audio.wav"));
        assert_eq!(p.to_str().unwrap(), "temp/audio_transcript.json");
    }
}
