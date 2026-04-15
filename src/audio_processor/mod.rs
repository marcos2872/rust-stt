//! Processador de áudio para pipelines de Speech-to-Text.
//!
//! Aplica uma cadeia de filtros em **9 etapas** sobre um arquivo de áudio WAV,
//! dividida em dois passes ffmpeg:
//!
//! **Passo 1 — Filtros principais**
//! 1. `Bandpass`              — HPF + LPF (isola faixa da voz)
//! 2. `Click Removal`         — remove cliques e ruídos impulsivos (`adeclick`)
//! 3. `Noise Reduction (FFT)` — reduz ruído estacionário (`afftdn`)
//! 4. `Noise Reduction (NLM)` — reduz vozes de fundo (`anlmdn`)
//! 5. `Voice EQ`              — realce paramétrico 2–4 kHz
//! 6. `Compression`           — equaliza dinâmica de picos
//! 7. `Noise Gate`            — suprime fundo entre segmentos de fala (`agate`)
//! 8. `Loudness Normalization` — normalização dinâmica (`dynaudnorm`)
//! 9. `Limiter`               — proteção final contra picos (`alimiter`)
//!
//! **Passo 2 — VAD**
//! 10. `Silence Removal`      — corte de silêncios excessivos (`silenceremove`)
//!
//! > `dynaudnorm` e `silenceremove` conflitam na mesma chain `-af` no
//! > ffmpeg 7.x, por isso o VAD roda em passe separado.
//!
//! Etapas opcionais podem ser desabilitadas via [`AudioProcessingConfig`].
//!
//! # Exemplo
//! ```no_run
//! use std::path::Path;
//! use rust_stt::audio_processor::{AudioProcessingConfig, process_audio};
//!
//! let config = AudioProcessingConfig::default();
//! let output = process_audio(Path::new("audio.wav"), Path::new("temp"), &config).unwrap();
//! println!("Processado: {}", output.display());
//! ```

pub mod filters;

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Configuração
// ---------------------------------------------------------------------------

/// Parâmetros de cada etapa do processamento de áudio.
///
/// Os valores padrão (`Default`) são calibrados para fala em STT.
/// Etapas opcionais podem ser desabilitadas com o campo `enable_*`.
#[derive(Debug, Clone)]
pub struct AudioProcessingConfig {
    // ── Etapa 1 — Bandpass ─────────────────────────────────────────────────
    /// Frequência de corte do HPF em Hz (padrão: 100 Hz).
    pub hpf_hz: u32,
    /// Frequência de corte do LPF em Hz (padrão: 8 000 Hz).
    /// Deve ser ≤ Nyquist do sample rate de saída (16 kHz → 8 kHz);
    /// valores acima disso são cortados pelo resampler antes de o filtro agir.
    pub lpf_hz: u32,

    // ── Etapa 2a — Click Removal ───────────────────────────────────────────
    /// Habilita remoção de cliques e ruídos impulsivos via `adeclick` (padrão: true).
    pub enable_click_removal: bool,
    /// Tamanho da janela de análise em ms para `adeclick` (padrão: 55.0).
    pub click_window_ms: f32,
    /// Sobreposição entre janelas em % para `adeclick` — range [50–95] (padrão: 75).
    pub click_overlap_pct: u32,

    // ── Etapa 2b — Noise Reduction FFT ────────────────────────────────────
    /// Piso de ruído estimado em dBFS para `afftdn` (padrão: -25).
    pub noise_floor_db: i32,

    // ── Etapa 2c — Noise Reduction NLM ────────────────────────────────────
    /// Habilita denoiser Non-Local Means `anlmdn` para vozes de fundo (padrão: true).
    pub enable_nlmeans: bool,
    /// Força da denoising NLM (padrão: 7.0, range 1–100).
    pub nlmeans_strength: f32,
    /// Raio do patch em segundos para `anlmdn` (padrão: 0.002).
    pub nlmeans_patch_radius_s: f32,
    /// Raio de busca em segundos para `anlmdn` (padrão: 0.002).
    pub nlmeans_research_radius_s: f32,
    /// Ganho máximo de correção para `anlmdn` (padrão: 15.0).
    pub nlmeans_max_gain: f32,

    // ── Etapa 3 — Voice EQ ─────────────────────────────────────────────────
    /// Frequência central do realce de voz em Hz (padrão: 3 000 Hz).
    pub voice_eq_freq_hz: u32,
    /// Ganho do realce em dB (padrão: 3.0).
    pub voice_eq_gain_db: f32,
    /// Largura de banda em Hz, usa `t=h` (padrão: 2 000 Hz → cobre 2–4 kHz).
    pub voice_eq_bandwidth_hz: u32,

    // ── Etapa 4a — Compression ─────────────────────────────────────────────
    /// Limiar da compressão em dBFS (padrão: -18).
    pub compress_threshold_db: i32,
    /// Taxa de compressão, ex.: 3.0 = 3:1 (padrão: 3.0).
    pub compress_ratio: f32,
    /// Ganho de makeup pós-compressão em dB (padrão: 2.0).
    pub compress_makeup_db: f32,

    // ── Etapa 4b — Noise Gate ──────────────────────────────────────────────
    /// Habilita noise gate para suprimir fundo entre falas (padrão: true).
    pub enable_noise_gate: bool,
    /// Limiar de abertura do gate em amplitude linear 0.0–1.0 (padrão: 0.01 ≈ -40 dBFS).
    pub gate_threshold: f32,
    /// Taxa de supressão abaixo do limiar (padrão: 10.0).
    pub gate_ratio: f32,
    /// Largura da curva de transição em dB (padrão: 2.828 ≈ suave).
    pub gate_knee: f32,

    // ── Etapa 5a — Loudness Normalization ──────────────────────────────────
    /// Nível de pico alvo do `dynaudnorm` (0.0–1.0, padrão: 0.9).
    pub loudnorm_peak: f32,
    /// Ganho máximo aplicado pelo `dynaudnorm` em dB (padrão: 15.0).
    pub loudnorm_max_gain: f32,

    // ── Etapa 5b — Limiter ─────────────────────────────────────────────────
    /// Habilita limitador lookahead final `alimiter` (padrão: true).
    pub enable_limiter: bool,
    /// Nível de pico máximo permitido 0.0–1.0 (padrão: 0.9 ≈ -0.9 dBFS).
    pub limiter_limit: f32,
    /// Attack do limiter em ms (padrão: 5.0).
    pub limiter_attack_ms: f32,
    /// Release do limiter em ms (padrão: 50.0).
    pub limiter_release_ms: f32,

    // ── Etapa 6 — Silence Removal ──────────────────────────────────────────
    /// Limiar de silêncio em dBFS (padrão: -50).
    pub silence_threshold_db: i32,
    /// Duração mínima de silêncio a ser removida em segundos (padrão: 0.3).
    pub silence_min_duration_s: f32,
}

impl Default for AudioProcessingConfig {
    fn default() -> Self {
        Self {
            // Bandpass
            hpf_hz: 100,
            lpf_hz: 8_000,
            // Click removal
            enable_click_removal: true,
            click_window_ms: 55.0,
            click_overlap_pct: 75,
            // Noise reduction FFT
            noise_floor_db: -35,
            // Noise reduction NLM
            enable_nlmeans: true,
            nlmeans_strength: 7.0,
            nlmeans_patch_radius_s: 0.002,
            nlmeans_research_radius_s: 0.002,
            nlmeans_max_gain: 15.0,
            // Voice EQ
            voice_eq_freq_hz: 3_000,
            voice_eq_gain_db: 3.0,
            voice_eq_bandwidth_hz: 2_000,
            // Compression
            compress_threshold_db: -24,
            compress_ratio: 2.0,
            compress_makeup_db: 2.0,
            // Noise gate
            enable_noise_gate: true,
            gate_threshold: 0.01,
            gate_ratio: 10.0,
            gate_knee: 2.828,
            // Loudness normalization
            loudnorm_peak: 0.7,
            loudnorm_max_gain: 15.0,
            // Limiter
            enable_limiter: true,
            limiter_limit: 0.7,
            limiter_attack_ms: 5.0,
            limiter_release_ms: 50.0,
            // Silence removal
            silence_threshold_db: -50,
            silence_min_duration_s: 0.8,
        }
    }
}

// ---------------------------------------------------------------------------
// Tipo de erro
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ProcessorError {
    /// Arquivo de entrada não encontrado.
    InputNotFound(PathBuf),
    /// Falha ao criar o diretório de saída.
    OutputDirError(String),
    /// O processo `ffmpeg` falhou.
    FfmpegFailed { exit_code: Option<i32>, stderr: String },
}

impl fmt::Display for ProcessorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputNotFound(p) => {
                write!(f, "Arquivo de entrada não encontrado: {}", p.display())
            }
            Self::OutputDirError(msg) => write!(f, "Erro ao criar diretório de saída: {msg}"),
            Self::FfmpegFailed { exit_code, stderr } => {
                write!(f, "ffmpeg falhou (código {:?}):\n{stderr}", exit_code)
            }
        }
    }
}

impl std::error::Error for ProcessorError {}

// ---------------------------------------------------------------------------
// Função pública
// ---------------------------------------------------------------------------

/// Processa um arquivo de áudio aplicando a cadeia completa em dois passes.
///
/// **Passo 1** — todos os filtros de transformação (bandpass, denoising,
/// remoção de cliques, EQ, compressão, gate, normalização e limiter).
///
/// **Passo 2** — VAD (`silenceremove`), executado em passe separado pois
/// `dynaudnorm` e `silenceremove` conflitam na mesma chain `-af` no ffmpeg 7.x.
///
/// O arquivo intermediário é salvo em `output_dir/.tmp_<stem>.wav` e removido
/// ao final, independente de sucesso ou falha.
pub fn process_audio(
    input: &Path,
    output_dir: &Path,
    config: &AudioProcessingConfig,
) -> Result<PathBuf, ProcessorError> {
    if !input.exists() {
        return Err(ProcessorError::InputNotFound(input.to_path_buf()));
    }

    std::fs::create_dir_all(output_dir)
        .map_err(|e| ProcessorError::OutputDirError(e.to_string()))?;

    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    // Passo 1: filtros principais → WAV intermediário
    let intermediate = output_dir.join(format!(".tmp_{stem}.wav"));
    let main_chain = build_filter_chain(config);
    let r1 = run_ffmpeg(input, &intermediate, &[("-af", &main_chain)]);

    if let Err(e) = r1 {
        let _ = std::fs::remove_file(&intermediate);
        return Err(e);
    }

    // Passo 2: VAD → arquivo final
    let output_path = output_dir.join(format!("{stem}.wav"));
    let vad_filter = build_vad_filter(config);
    let r2 = run_ffmpeg(&intermediate, &output_path, &[("-af", &vad_filter)]);

    let _ = std::fs::remove_file(&intermediate);
    r2?;
    Ok(output_path)
}

// ---------------------------------------------------------------------------
// Construção da chain
// ---------------------------------------------------------------------------

/// Monta a cadeia principal de filtros (sem silence removal).
///
/// Ordem das etapas:
/// 1. Bandpass
/// 2. Click removal (opcional)
/// 3. Noise reduction FFT
/// 4. Noise reduction NLM (opcional)
/// 5. Voice EQ
/// 6. Compression
/// 7. Noise gate (opcional)
/// 8. Loudness normalization
/// 9. Limiter (opcional)
pub fn build_filter_chain(config: &AudioProcessingConfig) -> String {
    let mut steps: Vec<String> = Vec::new();

    // 1 — Bandpass
    steps.push(filters::bandpass(config.hpf_hz, config.lpf_hz));

    // 2a — Click removal
    if config.enable_click_removal {
        steps.push(filters::click_removal(
            config.click_window_ms,
            config.click_overlap_pct,
        ));
    }

    // 2b — FFT noise reduction
    steps.push(filters::noise_reduction_fft(config.noise_floor_db));

    // 2c — NLM noise reduction (vozes de fundo)
    if config.enable_nlmeans {
        steps.push(filters::noise_reduction_nlmeans(
            config.nlmeans_strength,
            config.nlmeans_patch_radius_s,
            config.nlmeans_research_radius_s,
            config.nlmeans_max_gain,
        ));
    }

    // 3 — Voice EQ
    steps.push(filters::voice_eq(
        config.voice_eq_freq_hz,
        config.voice_eq_gain_db,
        config.voice_eq_bandwidth_hz,
    ));

    // 4a — Compression
    steps.push(filters::compression(
        config.compress_threshold_db,
        config.compress_ratio,
        config.compress_makeup_db,
    ));

    // 4b — Noise gate
    if config.enable_noise_gate {
        steps.push(filters::noise_gate(
            config.gate_threshold,
            config.gate_ratio,
            config.gate_knee,
        ));
    }

    // 5a — Loudness normalization
    steps.push(filters::loudness_normalization(
        config.loudnorm_peak,
        config.loudnorm_max_gain,
    ));

    // 5b — Limiter
    if config.enable_limiter {
        steps.push(filters::limiter(
            config.limiter_limit,
            config.limiter_attack_ms,
            config.limiter_release_ms,
        ));
    }

    steps.join(",")
}

/// Gera a string do filtro de remoção de silêncio (VAD) — passe separado.
pub fn build_vad_filter(config: &AudioProcessingConfig) -> String {
    filters::silence_removal(config.silence_threshold_db, config.silence_min_duration_s)
}

// ---------------------------------------------------------------------------
// Execução ffmpeg
// ---------------------------------------------------------------------------

/// Núcleo de execução do ffmpeg, compartilhado por todos os passes.
fn run_ffmpeg(
    input: &Path,
    output: &Path,
    extra_args: &[(&str, &str)],
) -> Result<(), ProcessorError> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-i", &input.to_string_lossy()]);

    for (flag, value) in extra_args {
        cmd.args([*flag, value]);
    }

    cmd.args(["-acodec", "pcm_s16le", "-ac", "1", "-ar", "16000"])
        .arg(output.to_string_lossy().as_ref());

    let result = cmd.output().map_err(|e| ProcessorError::FfmpegFailed {
        exit_code: None,
        stderr: format!("Não foi possível executar ffmpeg: {e}"),
    })?;

    if !result.status.success() {
        return Err(ProcessorError::FfmpegFailed {
            exit_code: result.status.code(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn create_test_wav(path: &Path) {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f", "lavfi",
                "-i", "sine=frequency=1000:duration=1,apad=pad_dur=1",
                "-acodec", "pcm_s16le",
                "-ac", "1",
                "-ar", "16000",
                &path.to_string_lossy(),
            ])
            .output()
            .expect("ffmpeg deve estar disponível");
        assert!(status.status.success(), "Falha ao criar WAV de teste");
    }

    // ── build_filter_chain ────────────────────────────────────────────────

    #[test]
    fn filter_chain_contem_todas_etapas_habilitadas() {
        let config = AudioProcessingConfig::default();
        let chain = build_filter_chain(&config);

        assert!(chain.contains("highpass"),    "bandpass ausente: {chain}");
        assert!(chain.contains("adeclick"),    "click removal ausente: {chain}");
        assert!(chain.contains("afftdn"),      "noise fft ausente: {chain}");
        assert!(chain.contains("anlmdn"),      "noise nlm ausente: {chain}");
        assert!(chain.contains("equalizer"),   "voice eq ausente: {chain}");
        assert!(chain.contains("acompressor"), "compression ausente: {chain}");
        assert!(chain.contains("agate"),       "noise gate ausente: {chain}");
        assert!(chain.contains("dynaudnorm"),  "loudnorm ausente: {chain}");
        assert!(chain.contains("alimiter"),    "limiter ausente: {chain}");
        // VAD fica na chain separada
        assert!(!chain.contains("silenceremove"), "silenceremove deve ser VAD chain: {chain}");
    }

    #[test]
    fn filter_chain_sem_etapas_opcionais() {
        let config = AudioProcessingConfig {
            enable_click_removal: false,
            enable_nlmeans: false,
            enable_noise_gate: false,
            enable_limiter: false,
            ..AudioProcessingConfig::default()
        };
        let chain = build_filter_chain(&config);

        assert!(!chain.contains("adeclick"), "adeclick deve estar desabilitado: {chain}");
        assert!(!chain.contains("anlmdn"),   "anlmdn deve estar desabilitado: {chain}");
        assert!(!chain.contains("agate"),    "agate deve estar desabilitado: {chain}");
        assert!(!chain.contains("alimiter"), "alimiter deve estar desabilitado: {chain}");
        // Etapas obrigatórias ainda presentes
        assert!(chain.contains("highpass"),    "bandpass ausente: {chain}");
        assert!(chain.contains("afftdn"),      "noise fft ausente: {chain}");
        assert!(chain.contains("equalizer"),   "voice eq ausente: {chain}");
        assert!(chain.contains("acompressor"), "compression ausente: {chain}");
        assert!(chain.contains("dynaudnorm"),  "loudnorm ausente: {chain}");
    }

    #[test]
    fn vad_filter_contem_silenceremove() {
        let config = AudioProcessingConfig::default();
        let vad = build_vad_filter(&config);
        assert!(vad.contains("silenceremove"),  "silenceremove ausente: {vad}");
        assert!(vad.contains("stop_periods=-1"), "stop_periods=-1 ausente: {vad}");
    }

    #[test]
    fn filter_chain_respeita_config_customizada() {
        let config = AudioProcessingConfig {
            hpf_hz: 150,
            noise_floor_db: -30,
            nlmeans_strength: 10.0,
            gate_threshold: 0.02,
            loudnorm_peak: 0.8,
            limiter_limit: 0.85,
            ..AudioProcessingConfig::default()
        };
        let chain = build_filter_chain(&config);

        assert!(chain.contains("f=150"),         "HPF customizado ausente: {chain}");
        assert!(chain.contains("nf=-30"),         "noise floor customizado ausente: {chain}");
        assert!(chain.contains("s=10"),           "nlmeans strength ausente: {chain}");
        assert!(chain.contains("threshold=0.02"), "gate threshold ausente: {chain}");
        assert!(chain.contains("p=0.8"),          "loudnorm peak ausente: {chain}");
        assert!(chain.contains("limit=0.85"),     "limiter limit ausente: {chain}");
    }

    #[test]
    fn process_audio_falha_com_entrada_inexistente() {
        let config = AudioProcessingConfig::default();
        let err = process_audio(
            Path::new("/nao/existe/audio.wav"),
            Path::new("/tmp/out"),
            &config,
        )
        .unwrap_err();
        assert!(matches!(err, ProcessorError::InputNotFound(_)));
    }

    // ── Integração ────────────────────────────────────────────────────────

    #[test]
    fn process_audio_gera_arquivo_processado() {
        let tmp_dir = std::env::temp_dir().join("rust_stt_proc_integration");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let input_path = tmp_dir.join("test_audio.wav");
        create_test_wav(&input_path);

        let config = AudioProcessingConfig::default();
        let result = process_audio(&input_path, &tmp_dir.join("out"), &config);

        assert!(result.is_ok(), "Processamento falhou: {:?}", result.err());
        let out = result.unwrap();
        assert!(out.exists(), "Arquivo não criado");
        assert!(out.metadata().unwrap().len() > 0, "Arquivo vazio");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn process_audio_mantém_parametros_wav() {
        let tmp_dir = std::env::temp_dir().join("rust_stt_proc_params");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let input_path = tmp_dir.join("test_audio.wav");
        create_test_wav(&input_path);

        let config = AudioProcessingConfig::default();
        let out = process_audio(&input_path, &tmp_dir.join("out"), &config).unwrap();

        let probe = Command::new("ffprobe")
            .args([
                "-v", "error",
                "-select_streams", "a:0",
                "-show_entries", "stream=sample_rate,channels,codec_name",
                "-of", "default=noprint_wrappers=1",
                &out.to_string_lossy(),
            ])
            .output()
            .expect("ffprobe deve estar disponível");

        let info = String::from_utf8_lossy(&probe.stdout);
        assert!(info.contains("sample_rate=16000"), "Deve ser 16 kHz: {info}");
        assert!(info.contains("channels=1"),        "Deve ser mono: {info}");
        assert!(info.contains("codec_name=pcm_s16le"), "Deve ser PCM 16-bit: {info}");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn process_audio_cria_diretorio_de_saida() {
        let tmp_dir = std::env::temp_dir().join("rust_stt_proc_mkdir");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let input_path = tmp_dir.join("test_audio.wav");
        create_test_wav(&input_path);

        let output_dir = tmp_dir.join("novo_dir_que_nao_existe");
        let _ = std::fs::remove_dir_all(&output_dir);

        let config = AudioProcessingConfig::default();
        process_audio(&input_path, &output_dir, &config).unwrap();
        assert!(output_dir.exists());

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn process_audio_com_etapas_opcionais_desabilitadas() {
        let tmp_dir = std::env::temp_dir().join("rust_stt_proc_minimal");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let input_path = tmp_dir.join("test_audio.wav");
        create_test_wav(&input_path);

        let config = AudioProcessingConfig {
            enable_click_removal: false,
            enable_nlmeans: false,
            enable_noise_gate: false,
            enable_limiter: false,
            ..AudioProcessingConfig::default()
        };
        let result = process_audio(&input_path, &tmp_dir.join("out"), &config);
        assert!(result.is_ok(), "Falhou com etapas desabilitadas: {:?}", result.err());

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
