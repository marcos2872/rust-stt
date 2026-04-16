//! Binário `minutes` — gera ata estruturada (FLOW) a partir de um summary JSON.
//!
//! # Uso
//! ```sh
//! cargo run --bin minutes -- <summary.json>
//! ```
//!
//! # Saída
//! - Resumo da ata impresso no terminal
//! - `<stem>_minutes.json` salvo no mesmo diretório do summary

use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;

// Preços Azure OpenAI gpt-5.4-mini
const PRICE_INPUT_PER_1M: f64 = 0.750; // USD por 1M tokens de entrada
const PRICE_OUTPUT_PER_1M: f64 = 4.500; // USD por 1M tokens de saída

fn main() {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Uso: {} <summary.json>", args[0]);
        eprintln!();
        eprintln!("Exemplo:");
        eprintln!("  cargo run --bin minutes -- temp/call_summary.json");
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

fn run(summary_path: &Path) -> Result<(), String> {
    // ── Validação ─────────────────────────────────────────────────────────
    if !summary_path.exists() {
        return Err(format!(
            "summary não encontrado: {}",
            summary_path.display()
        ));
    }

    // ── Configuração ──────────────────────────────────────────────────────
    let config = rust_stt::minutes::MinutesConfig::from_env().map_err(|e| e.to_string())?;

    println!("Minutes — FLOW Registro de Reunião de Alinhamento");
    println!("  Summary    : {}", summary_path.display());
    println!("  Deployment : {}", config.deployment);
    println!("  API version: {}", config.api_version);
    println!();

    // ── Execução ──────────────────────────────────────────────────────────
    let start = std::time::Instant::now();
    let result =
        rust_stt::minutes::generate_minutes(summary_path, &config).map_err(|e| e.to_string())?;
    let elapsed = start.elapsed();

    // ── Exibição ──────────────────────────────────────────────────────────
    print_minutes(&result.minutes, elapsed);
    print_token_cost(&result.token_usage);

    // ── Salvar JSON ───────────────────────────────────────────────────────
    let out = build_output_path(summary_path);
    std::fs::write(&out, result.to_json()).map_err(|e| format!("Falha ao salvar JSON: {e}"))?;

    println!();
    println!("✓ Ata salva em: {}", out.display());

    Ok(())
}

/// Imprime as seções principais da ata no terminal.
fn print_minutes(minutes: &serde_json::Value, elapsed: Duration) {
    println!("{}", "─".repeat(60));
    println!("ATA GERADA (tempo: {:.1}s)", elapsed.as_secs_f64());
    println!("{}", "─".repeat(60));

    let title = minutes["meeting_data"]["title"].as_str().unwrap_or("—");
    let date = minutes["meeting_data"]["date"].as_str().unwrap_or("—");
    println!("Título : {title}");
    println!("Data   : {date}");

    if let Some(parts) = minutes["participants"].as_array() {
        println!("\nParticipantes ({}):", parts.len());
        for p in parts {
            let name = p["name"].as_str().unwrap_or("—");
            let role = p["role"].as_str().unwrap_or("—");
            println!("  • {name} ({role})");
        }
    }

    if let Some(topics) = minutes["topics"].as_array() {
        println!("\nTópicos discutidos: {}", topics.len());
        for t in topics {
            let title = t["title"].as_str().unwrap_or("—");
            println!("  • {title}");
        }
    }

    if let Some(decisions) = minutes["decisions"].as_array() {
        println!("\nDecisões tomadas: {}", decisions.len());
        for d in decisions {
            let id = d["id"].as_str().unwrap_or("—");
            let dec = d["decision"].as_str().unwrap_or("—");
            println!("  [{id}] {dec}");
        }
    }

    if let Some(todos) = minutes["action_plan"]["todos"].as_array() {
        println!("\nAction items: {}", todos.len());
        for t in todos {
            let who = t["who"].as_str().unwrap_or("—");
            let what = t["what"].as_str().unwrap_or("—");
            let when = t["when"].as_str().unwrap_or("—");
            println!("  • [{who} / {when}] {what}");
        }
    }
    println!("{}", "─".repeat(60));
}

/// Imprime uso de tokens e custo estimado.
fn print_token_cost(usage: &rust_stt::minutes::llm::TokenUsage) {
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
}

/// `temp/call_summary.json` → `temp/call_minutes.json`
fn build_output_path(summary_path: &Path) -> PathBuf {
    let stem = summary_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("meeting")
        .trim_end_matches("_summary");
    let dir = summary_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(format!("{stem}_minutes.json"))
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_output_path_remove_sufixo_summary() {
        let p = build_output_path(Path::new("temp/call_summary.json"));
        assert_eq!(p.to_str().unwrap(), "temp/call_minutes.json");
    }

    #[test]
    fn build_output_path_sem_sufixo() {
        let p = build_output_path(Path::new("temp/reuniao.json"));
        assert_eq!(p.to_str().unwrap(), "temp/reuniao_minutes.json");
    }

    #[test]
    fn build_output_path_diretorio_corrente() {
        let p = build_output_path(Path::new("my_summary.json"));
        assert_eq!(p.to_str().unwrap(), "my_minutes.json");
    }
}
