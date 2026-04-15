mod audio_processor;
mod converter;

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Uso: {} <caminho_do_arquivo.mp4>", args[0]);
        std::process::exit(1);
    }

    let input = Path::new(&args[1]);
    let output_dir = Path::new("temp");

    match run_pipeline(input, output_dir) {
        Ok(output) => println!("\n✓ Pipeline concluído → {}", output.display()),
        Err(msg) => {
            eprintln!("\n✗ {msg}");
            std::process::exit(1);
        }
    }
}

/// Executa o pipeline completo: MP4 → WAV intermediário (temp do SO) → WAV processado.
///
/// O WAV intermediário é criado em um diretório temporário do sistema e removido
/// ao final, independentemente de sucesso ou falha.
fn run_pipeline(input: &Path, output_dir: &Path) -> Result<std::path::PathBuf, String> {
    let intermediate_dir = std::env::temp_dir().join("rust_stt_pipeline");

    // ------------------------------------------------------------------
    // Etapa 1/2 — Conversão MP4 → WAV (mono, 16 kHz, 16-bit)
    // ------------------------------------------------------------------
    println!("[1/2] Convertendo MP4 → WAV...");

    let intermediate_wav = converter::convert_mp4_to_wav(input, &intermediate_dir)
        .map_err(|e| format!("Conversão falhou: {e}"))?;

    println!("      ✓ WAV intermediário: {}", intermediate_wav.display());

    // ------------------------------------------------------------------
    // Etapa 2/2 — Processamento de áudio
    // ------------------------------------------------------------------
    println!("[2/2] Processando áudio...");
    println!("      • Bandpass HPF/LPF");
    println!("      • Remoção de cliques (adeclick)");
    println!("      • Noise reduction FFT (afftdn)");
    println!("      • Noise reduction NLM — vozes de fundo (anlmdn)");
    println!("      • Voice EQ (realce 2–4 kHz)");
    println!("      • Compressão leve");
    println!("      • Noise gate — suprime fundo entre falas");
    println!("      • Normalização de loudness (dynaudnorm)");
    println!("      • Limiter — proteção contra picos (alimiter)");
    println!("      • Remoção de silêncios (VAD)");

    let config = audio_processor::AudioProcessingConfig::default();
    let result = audio_processor::process_audio(&intermediate_wav, output_dir, &config)
        .map_err(|e| format!("Processamento falhou: {e}"));

    let _ = std::fs::remove_file(&intermediate_wav);
    let _ = std::fs::remove_dir(&intermediate_dir);

    println!("      ✓ WAV intermediário removido");

    result
}
