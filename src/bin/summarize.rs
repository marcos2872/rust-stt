//! Binário `summarize` — identifica falantes pelo VTT e gera resumo via LLM.
//!
//! # Uso
//! ```sh
//! cargo run --bin summarize -- <transcript.json> <meeting.vtt>
//! ```
//!
//! # Saída
//! - Transcrição com nomes reais impressa no terminal
//! - `<stem>_summary.json` salvo no mesmo diretório do transcript

use std::path::Path;
use std::process;

// Preços Azure OpenAI gpt-5.4-mini
const PRICE_INPUT_PER_1M: f64 = 0.750; // USD por 1M tokens de entrada
const PRICE_OUTPUT_PER_1M: f64 = 4.500; // USD por 1M tokens de saída

fn main() {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!("Uso: {} <transcript.json> <meeting.vtt>", args[0]);
        eprintln!();
        eprintln!("Exemplo:");
        eprintln!("  cargo run --bin summarize -- temp/audio_transcript.json meeting.vtt");
        process::exit(1);
    }

    match run(Path::new(&args[1]), Path::new(&args[2])) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("\n✗ Erro: {e}");
            process::exit(1);
        }
    }
}

fn run(transcript_path: &Path, vtt_path: &Path) -> Result<(), String> {
    // ── Validação ─────────────────────────────────────────────────────────
    if !transcript_path.exists() {
        return Err(format!(
            "transcript não encontrado: {}",
            transcript_path.display()
        ));
    }
    if !vtt_path.exists() {
        return Err(format!("VTT não encontrado: {}", vtt_path.display()));
    }

    // ── Configuração ──────────────────────────────────────────────────────
    let config = rust_stt::summarizer::SummarizerConfig::from_env().map_err(|e| e.to_string())?;

    println!("Summarizer — Azure OpenAI + Teams VTT");
    println!("  Transcript : {}", transcript_path.display());
    println!("  VTT        : {}", vtt_path.display());
    println!("  Deployment : {}", config.deployment);
    println!("  API version: {}", config.api_version);
    println!();

    // ── Pré-visualização do VTT ───────────────────────────────────────────
    let vtt_content =
        std::fs::read_to_string(vtt_path).map_err(|e| format!("Falha ao ler VTT: {e}"))?;
    let vtt_entries = rust_stt::summarizer::vtt::parse(&vtt_content);

    // Conta participantes únicos
    let mut participants: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for e in &vtt_entries {
        participants.insert(&e.name);
    }
    let mut sorted_participants: Vec<&&str> = participants.iter().collect();
    sorted_participants.sort();

    println!("Participantes no VTT ({}):", sorted_participants.len());
    for p in &sorted_participants {
        println!("  • {p}");
    }
    println!();

    // ── Execução ──────────────────────────────────────────────────────────
    let start = std::time::Instant::now();
    let result = rust_stt::summarizer::summarize(transcript_path, vtt_path, &config)
        .map_err(|e| e.to_string())?;
    let elapsed = start.elapsed();

    // ── Mapeamento identificado ───────────────────────────────────────────
    println!(
        "Mapeamento de falantes (tempo: {:.1}s)",
        elapsed.as_secs_f64()
    );
    let mut mapping: Vec<(&String, &String)> = result.speaker_mapping.iter().collect();
    mapping.sort_by_key(|(k, _)| k.as_str());
    for (speaker, name) in &mapping {
        println!("  {speaker} → {name}");
    }

    // ── Resumo ────────────────────────────────────────────────────────────
    println!();
    println!("{}", "─".repeat(60));
    println!("RESUMO");
    println!("{}", "─".repeat(60));
    println!("{}", result.summary);

    if !result.action_items.is_empty() {
        println!();
        println!("PONTOS DE ACAO");
        println!("{}", "─".repeat(60));
        for item in &result.action_items {
            println!("  • {item}");
        }
    }

    if !result.key_decisions.is_empty() {
        println!();
        println!("DECISOES");
        println!("{}", "─".repeat(60));
        for d in &result.key_decisions {
            println!("  • {d}");
        }
    }

    // ── Tokens e custo estimado ───────────────────────────────────────────
    let usage = &result.token_usage;
    let cost_in = usage.prompt_tokens as f64 / 1_000_000.0 * PRICE_INPUT_PER_1M;
    let cost_out = usage.completion_tokens as f64 / 1_000_000.0 * PRICE_OUTPUT_PER_1M;
    let cost_total = cost_in + cost_out;

    println!();
    println!("{}", "─".repeat(60));
    println!("USO DE TOKENS (Azure OpenAI)");
    println!("{}", "─".repeat(60));
    println!(
        "  Entrada  : {:>8} tokens   (${:.6})",
        usage.prompt_tokens, cost_in
    );
    println!(
        "  Saída    : {:>8} tokens   (${:.6})",
        usage.completion_tokens, cost_out
    );
    println!(
        "  Total    : {:>8} tokens   (${:.6})",
        usage.total_tokens, cost_total
    );
    println!(
        "  (preços: ${}/1M entrada · ${}/1M saída — gpt-5.4-mini)",
        PRICE_INPUT_PER_1M, PRICE_OUTPUT_PER_1M
    );

    // ── Transcrição ───────────────────────────────────────────────────────
    println!();
    println!("{}", "─".repeat(60));
    println!(
        "TRANSCRICAO ({} segmentos — salvo no JSON)",
        result.transcript.len()
    );
    println!("{}", "─".repeat(60));

    // ── Salvar JSON ───────────────────────────────────────────────────────
    let json_path = build_output_path(transcript_path);
    std::fs::write(&json_path, result.to_json())
        .map_err(|e| format!("Falha ao salvar JSON: {e}"))?;

    println!();
    println!("✓ JSON salvo em: {}", json_path.display());

    Ok(())
}

/// `temp/audio_transcript.json` → `temp/audio_summary.json`
fn build_output_path(transcript_path: &Path) -> std::path::PathBuf {
    let stem = transcript_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio")
        .trim_end_matches("_transcript");
    let dir = transcript_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(format!("{stem}_summary.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_output_path_remove_sufixo_transcript() {
        let p = build_output_path(Path::new("temp/audio_transcript.json"));
        assert_eq!(p.to_str().unwrap(), "temp/audio_summary.json");
    }

    #[test]
    fn build_output_path_sem_sufixo() {
        let p = build_output_path(Path::new("temp/reuniao.json"));
        assert_eq!(p.to_str().unwrap(), "temp/reuniao_summary.json");
    }
}
