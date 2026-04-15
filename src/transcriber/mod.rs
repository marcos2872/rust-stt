//! Transcrição de áudio via Azure OpenAI.
//!
//! Suporta o modelo `gpt-4o-transcribe-diarize` e compatíveis.
//! A identificação de falantes é detectada automaticamente a partir de
//! padrões no texto retornado pela API (ex.: `"Speaker 1:"`, `"[SPEAKER_00]:"`).
//!
//! # Credenciais (`.env`)
//! ```env
//! AZURE_OPENAI_API_KEY=<chave>
//! AZURE_OPENAI_ENDPOINT=https://<recurso>.cognitiveservices.azure.com
//! AZURE_OPENAI_DEPLOYMENT=gpt-4o-transcribe-diarize
//! AZURE_OPENAI_API_VERSION=2025-04-01-preview   # opcional
//! AZURE_OPENAI_LANGUAGE=pt                       # opcional, auto-detect se omitido
//! ```
//!
//! # Exemplo
//! ```no_run
//! use std::path::Path;
//! use rust_stt::transcriber::{TranscriptionConfig, transcribe};
//!
//! let config = TranscriptionConfig::from_env().unwrap();
//! let result = transcribe(Path::new("audio.wav"), &config).unwrap();
//! println!("{}", result.format_output());
//! ```

pub mod azure;

use std::fmt;
use std::path::{Path, PathBuf};

/// Duração máxima por chunk em segundos.
/// WAV 16-bit mono 16kHz = 32 KB/s; Azure OpenAI limita a 25 MB por arquivo.
/// 700s × 32 KB/s ≈ 22 MB — margem segura abaixo do limite.
const CHUNK_MAX_SECS: u64 = 700;

// ---------------------------------------------------------------------------
// Configuração
// ---------------------------------------------------------------------------

/// Credenciais e opções para o cliente Azure OpenAI.
#[derive(Debug, Clone)]
pub struct TranscriptionConfig {
    /// Chave de API (`AZURE_OPENAI_API_KEY`).
    pub api_key: String,
    /// URL base do recurso (`AZURE_OPENAI_ENDPOINT`).
    pub endpoint: String,
    /// Nome do deployment (`AZURE_OPENAI_DEPLOYMENT`).
    pub deployment: String,
    /// Versão da API (`AZURE_OPENAI_API_VERSION`, padrão: `2025-04-01-preview`).
    pub api_version: String,
    /// Idioma forçado, ex.: `"pt"`. `None` = auto-detect.
    pub language: Option<String>,
}

impl TranscriptionConfig {
    /// Lê as credenciais das variáveis de ambiente.
    ///
    /// Chame `dotenvy::dotenv().ok()` antes para carregar o `.env`.
    pub fn from_env() -> Result<Self, TranscriberError> {
        let api_key    = require_env("AZURE_OPENAI_API_KEY")?;
        let endpoint   = require_env("AZURE_OPENAI_ENDPOINT")?;
        let deployment = require_env("AZURE_OPENAI_DEPLOYMENT")?;

        let api_version = std::env::var("AZURE_OPENAI_API_VERSION")
            .unwrap_or_else(|_| "2025-04-01-preview".to_string());
        let language = std::env::var("AZURE_OPENAI_LANGUAGE").ok();

        Ok(Self { api_key, endpoint, deployment, api_version, language })
    }
}

fn require_env(key: &str) -> Result<String, TranscriberError> {
    std::env::var(key).map_err(|_| {
        TranscriberError::Config(format!("Variável de ambiente ausente: {key}"))
    })
}

// ---------------------------------------------------------------------------
// Tipos de resultado
// ---------------------------------------------------------------------------

/// Resultado completo da transcrição.
#[derive(Debug)]
pub struct TranscriptionResult {
    /// Texto completo retornado pela API.
    pub full_text: String,
    /// Segmentos por falante — populados se a API ou parser detectar marcadores.
    pub segments: Vec<Segment>,
    /// Tokens consumidos (para referência de custo).
    pub usage: Option<UsageInfo>,
}

/// Um turno de fala identificado no texto.
#[derive(Debug, PartialEq)]
pub struct Segment {
    /// Rótulo do falante, ex.: `"Speaker 1"`, `"SPEAKER_00"`. `None` se não detectado.
    pub speaker: Option<String>,
    /// Texto deste turno.
    pub text: String,
}

/// Estatísticas de tokens da requisição.
#[derive(Debug)]
pub struct UsageInfo {
    pub total_tokens:  Option<u64>,
    pub input_tokens:  Option<u64>,
    pub output_tokens: Option<u64>,
}

// ---------------------------------------------------------------------------
// Tipo de erro
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum TranscriberError {
    /// Variável de ambiente obrigatória ausente.
    Config(String),
    /// Erro de I/O ao ler o arquivo de áudio.
    Io(String),
    /// Erro HTTP (conexão, timeout, status >= 400).
    Http(String),
    /// Falha ao deserializar a resposta JSON.
    Parse(String),
}

impl fmt::Display for TranscriberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(m) => write!(f, "Configuração: {m}"),
            Self::Io(m)     => write!(f, "I/O: {m}"),
            Self::Http(m)   => write!(f, "HTTP: {m}"),
            Self::Parse(m)  => write!(f, "Parse: {m}"),
        }
    }
}

impl std::error::Error for TranscriberError {}

// ---------------------------------------------------------------------------
// Função pública
// ---------------------------------------------------------------------------

/// Transcreve um arquivo de áudio usando o Azure OpenAI.
///
/// Se o áudio for mais longo que [`CHUNK_MAX_SECS`] (1200s), divide automaticamente
/// em partes com ffmpeg e concatena os resultados.
pub fn transcribe(
    audio_path: &Path,
    config: &TranscriptionConfig,
) -> Result<TranscriptionResult, TranscriberError> {
    if !audio_path.exists() {
        return Err(TranscriberError::Io(format!(
            "Arquivo de áudio não encontrado: {}",
            audio_path.display()
        )));
    }

    let duration_s = get_audio_duration(audio_path)?;

    if duration_s > CHUNK_MAX_SECS {
        transcribe_chunked(audio_path, config, duration_s)
    } else {
        transcribe_single(audio_path, config)
    }
}

// ---------------------------------------------------------------------------
// Chunking e transcrição de arquivo único
// ---------------------------------------------------------------------------

/// Transcreve um arquivo sem divisão.
fn transcribe_single(
    audio_path: &Path,
    config: &TranscriptionConfig,
) -> Result<TranscriptionResult, TranscriberError> {
    let raw       = azure::transcribe(audio_path, config)?;
    let full_text = raw.text.trim().to_string();
    let segments  = parse_speaker_segments(&full_text);
    let usage = raw.usage.map(|u| UsageInfo {
        total_tokens:  u.total_tokens,
        input_tokens:  u.input_tokens,
        output_tokens: u.output_tokens,
    });
    Ok(TranscriptionResult { full_text, segments, usage })
}

/// Divide o áudio em chunks de [`CHUNK_MAX_SECS`] segundos, transcreve cada um
/// e concatena os resultados em ordem.
fn transcribe_chunked(
    audio_path: &Path,
    config: &TranscriptionConfig,
    duration_s: u64,
) -> Result<TranscriptionResult, TranscriberError> {
    let n_chunks = (duration_s as f64 / CHUNK_MAX_SECS as f64).ceil() as u64;
    eprintln!(
        "[áudio {:.0}s > {CHUNK_MAX_SECS}s] dividindo em {n_chunks} partes...",
        duration_s
    );

    let tmp_dir = std::env::temp_dir().join("rust_stt_chunks");
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| TranscriberError::Io(e.to_string()))?;

    let stem = audio_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("chunk");

    let mut all_texts: Vec<String> = Vec::new();
    let mut total_usage = UsageInfo {
        total_tokens:  Some(0),
        input_tokens:  Some(0),
        output_tokens: Some(0),
    };

    for i in 0..n_chunks {
        let start_s  = i * CHUNK_MAX_SECS;
        let chunk_path = tmp_dir.join(format!("{stem}_chunk_{i:02}.wav"));

        eprintln!("  Parte {}/{n_chunks}: {}s - {}s", i + 1, start_s, start_s + CHUNK_MAX_SECS);
        extract_chunk(audio_path, &chunk_path, start_s, CHUNK_MAX_SECS)?;

        let result = transcribe_single(&chunk_path, config)?;
        all_texts.push(result.full_text);

        // Acumula tokens
        if let Some(u) = result.usage {
            accumulate(&mut total_usage.total_tokens,  u.total_tokens);
            accumulate(&mut total_usage.input_tokens,  u.input_tokens);
            accumulate(&mut total_usage.output_tokens, u.output_tokens);
        }

        let _ = std::fs::remove_file(&chunk_path);
    }

    let _ = std::fs::remove_dir(&tmp_dir);

    let full_text = all_texts.join(" ");
    let segments  = parse_speaker_segments(&full_text);

    Ok(TranscriptionResult {
        full_text,
        segments,
        usage: Some(total_usage),
    })
}

/// Extrai um trecho do áudio com ffmpeg.
fn extract_chunk(
    input:    &Path,
    output:   &PathBuf,
    start_s:  u64,
    duration: u64,
) -> Result<(), TranscriberError> {
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-i",       &input.to_string_lossy(),
            "-ss",      &start_s.to_string(),
            "-t",       &duration.to_string(),
            "-acodec",  "pcm_s16le",
            "-ac",      "1",
            "-ar",      "16000",
            &output.to_string_lossy(),
        ])
        .output()
        .map_err(|e| TranscriberError::Io(format!("ffmpeg não encontrado: {e}")))?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr).into_owned();
        return Err(TranscriberError::Io(format!("ffmpeg falhou: {stderr}")));
    }
    Ok(())
}

/// Retorna a duração do áudio em segundos via ffprobe.
fn get_audio_duration(path: &Path) -> Result<u64, TranscriberError> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",             "error",
            "-show_entries",  "format=duration",
            "-of",            "default=noprint_wrappers=1:nokey=1",
            &path.to_string_lossy(),
        ])
        .output()
        .map_err(|e| TranscriberError::Io(format!("ffprobe não encontrado: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .trim()
        .parse::<f64>()
        .map(|d| d as u64)
        .map_err(|e| TranscriberError::Io(format!("duração inválida: {e} (got '{}')", stdout.trim())))
}

/// Soma `b` em `a`, ignorando `None`.
fn accumulate(a: &mut Option<u64>, b: Option<u64>) {
    if let (Some(av), Some(bv)) = (a.as_mut(), b) {
        *av += bv;
    }
}



/// Tenta extrair segmentos por falante a partir de padrões comuns no texto.
///
/// Padrões reconhecidos (case-insensitive):
/// - `Speaker 1: texto`
/// - `Speaker_00: texto`
/// - `[SPEAKER_00]: texto`
/// - `[Speaker 1]: texto`
/// - `Falante 1: texto`
///
/// Se nenhum padrão for encontrado, retorna um único segmento sem falante.
fn parse_speaker_segments(text: &str) -> Vec<Segment> {
    // Padrões: opcional "[", label com letras/números/espaços/underscore, opcional "]", ":"
    let patterns: &[&str] = &[
        r"\[?(?i)(speaker[\s_-]?\d+)\]?\s*:",
        r"\[?(?i)(falante[\s_]?\d+)\]?\s*:",
        r"\[?(?i)(locutor[\s_]?\d+)\]?\s*:",
    ];

    for pattern in patterns {
        if let Some(segments) = try_parse_pattern(text, pattern) {
            if segments.len() > 1 {
                return segments;
            }
        }
    }

    // Nenhum padrão detectado — texto único sem falante identificado
    vec![Segment {
        speaker: None,
        text: text.to_string(),
    }]
}

/// Tenta dividir o texto usando um padrão de regex de falante.
fn try_parse_pattern(text: &str, _pattern: &str) -> Option<Vec<Segment>> {
    // Implementação sem regex crate — busca por prefixos comuns
    let mut segments: Vec<Segment> = Vec::new();
    let mut current_speaker: Option<String> = None;
    let mut current_text = String::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Tenta detectar "Label: texto" na linha
        if let Some((label, rest)) = detect_speaker_label(line) {
            // Salva segmento anterior se tiver conteúdo
            if !current_text.trim().is_empty() {
                segments.push(Segment {
                    speaker: current_speaker.clone(),
                    text:    current_text.trim().to_string(),
                });
                current_text.clear();
            }
            current_speaker = Some(label);
            current_text.push_str(rest);
        } else {
            if !current_text.is_empty() {
                current_text.push(' ');
            }
            current_text.push_str(line);
        }
    }

    // Último segmento
    if !current_text.trim().is_empty() {
        segments.push(Segment {
            speaker: current_speaker,
            text:    current_text.trim().to_string(),
        });
    }

    if segments.len() > 1 { Some(segments) } else { None }
}

/// Detecta se uma linha começa com um rótulo de falante.
///
/// Retorna `Some((label, restante_do_texto))` ou `None`.
fn detect_speaker_label(line: &str) -> Option<(String, &str)> {
    let line_lower = line.to_lowercase();

    let prefixes = ["speaker", "falante", "locutor"];
    for prefix in prefixes {
        if !line_lower.starts_with(prefix) && !line_lower.starts_with('[') {
            continue;
        }

        // Remove colchete inicial se presente
        let stripped = if line.starts_with('[') {
            line.trim_start_matches('[')
        } else {
            line
        };

        // Procura ":" após o label
        if let Some(colon_pos) = stripped.find(':') {
            let label_raw = stripped[..colon_pos]
                .trim_end_matches(']')
                .trim()
                .to_string();

            // Valida: deve conter "speaker", "falante" ou "locutor" + número
            let label_lower = label_raw.to_lowercase();
            let has_prefix   = prefixes.iter().any(|p| label_lower.contains(p));
            let has_digit    = label_raw.chars().any(|c| c.is_ascii_digit());

            if has_prefix && has_digit {
                let rest = stripped[colon_pos + 1..].trim();
                return Some((label_raw, rest));
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Formatação
// ---------------------------------------------------------------------------

impl TranscriptionResult {
    /// Formata a transcrição para exibição no terminal.
    ///
    /// - Se falantes foram detectados: mostra segmentos com rótulo.
    /// - Caso contrário: mostra o texto completo.
    pub fn format_output(&self) -> String {
        let has_speakers = self.segments.iter().any(|s| s.speaker.is_some());

        if has_speakers {
            self.segments
                .iter()
                .map(|s| {
                    let speaker = s.speaker.as_deref().unwrap_or("Desconhecido");
                    format!("{speaker}:\n  {}", s.text)
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            self.full_text.clone()
        }
    }

    /// Serializa o resultado completo como JSON formatado.
    pub fn to_json(&self) -> String {
        let segments: Vec<serde_json::Value> = self
            .segments
            .iter()
            .map(|s| serde_json::json!({
                "speaker": s.speaker,
                "text":    s.text
            }))
            .collect();

        let usage = self.usage.as_ref().map(|u| serde_json::json!({
            "total_tokens":  u.total_tokens,
            "input_tokens":  u.input_tokens,
            "output_tokens": u.output_tokens
        }));

        serde_json::to_string_pretty(&serde_json::json!({
            "full_text": self.full_text,
            "speakers_detected": self.segments.iter().any(|s| s.speaker.is_some()),
            "segment_count": self.segments.len(),
            "segments": segments,
            "usage": usage
        }))
        .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── parser de falantes ─────────────────────────────────────────────────

    #[test]
    fn parse_sem_marcadores_retorna_segmento_unico_sem_speaker() {
        let text = "Bom dia a todos. Vamos começar a reunião.";
        let segs = parse_speaker_segments(text);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].speaker, None);
        assert!(segs[0].text.contains("Bom dia"));
    }

    #[test]
    fn parse_detecta_speaker_numerico() {
        let text = "Speaker 1: Olá, tudo bem?\nSpeaker 2: Tudo ótimo, obrigado.";
        let segs = parse_speaker_segments(text);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].speaker.as_deref(), Some("Speaker 1"));
        assert!(segs[0].text.contains("Olá"));
        assert_eq!(segs[1].speaker.as_deref(), Some("Speaker 2"));
        assert!(segs[1].text.contains("Tudo ótimo"));
    }

    #[test]
    fn parse_detecta_falante_em_portugues() {
        let text = "Falante 1: Bom dia.\nFalante 2: Oi!";
        let segs = parse_speaker_segments(text);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].speaker.as_deref(), Some("Falante 1"));
        assert_eq!(segs[1].speaker.as_deref(), Some("Falante 2"));
    }

    #[test]
    fn detect_speaker_label_reconhece_prefixo_speaker() {
        let (label, rest) = detect_speaker_label("Speaker 1: Olá!").unwrap();
        assert_eq!(label, "Speaker 1");
        assert_eq!(rest, "Olá!");
    }

    #[test]
    fn detect_speaker_label_ignora_linha_normal() {
        assert!(detect_speaker_label("Bom dia a todos.").is_none());
        assert!(detect_speaker_label("A reunião vai começar.").is_none());
    }

    // ── formatação ────────────────────────────────────────────────────────

    #[test]
    fn format_output_sem_speakers_retorna_full_text() {
        let result = TranscriptionResult {
            full_text: "Texto sem falantes.".to_string(),
            segments:  vec![Segment { speaker: None, text: "Texto sem falantes.".to_string() }],
            usage:     None,
        };
        assert_eq!(result.format_output(), "Texto sem falantes.");
    }

    #[test]
    fn format_output_com_speakers_inclui_labels() {
        let result = TranscriptionResult {
            full_text: "Speaker 1: Oi. Speaker 2: Olá.".to_string(),
            segments: vec![
                Segment { speaker: Some("Speaker 1".to_string()), text: "Oi.".to_string() },
                Segment { speaker: Some("Speaker 2".to_string()), text: "Olá.".to_string() },
            ],
            usage: None,
        };
        let out = result.format_output();
        assert!(out.contains("Speaker 1:"));
        assert!(out.contains("Speaker 2:"));
    }

    #[test]
    fn to_json_contem_campos_obrigatorios() {
        let result = TranscriptionResult {
            full_text: "Oi.".to_string(),
            segments:  vec![Segment { speaker: None, text: "Oi.".to_string() }],
            usage:     Some(UsageInfo { total_tokens: Some(10), input_tokens: Some(5), output_tokens: Some(5) }),
        };
        let json = result.to_json();
        assert!(json.contains("\"full_text\""));
        assert!(json.contains("\"segments\""));
        assert!(json.contains("\"usage\""));
        assert!(json.contains("\"total_tokens\""));
    }

    // ── config ────────────────────────────────────────────────────────────

    #[test]
    fn config_from_env_falha_sem_variaveis() {
        let result = require_env("AZURE_OPENAI_CHAVE_INEXISTENTE_XYZ");
        assert!(matches!(result, Err(TranscriberError::Config(_))));
    }
}
