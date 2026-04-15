//! Transcrição de áudio com diarização via Azure AI Speech.
//!
//! Usa a **Fast Transcription API** (`api-version=2024-11-15`) que retorna:
//! - Texto por falante com ID numérico (`speaker: 1`, `speaker: 2`…)
//! - Timestamps de início e fim por segmento (`offsetMilliseconds`)
//! - Confiança por segmento (`confidence`)
//!
//! Suporta arquivos até ~200 MB / 4 horas em uma única requisição —
//! sem necessidade de chunking para a maioria dos casos de uso.
//!
//! # Credenciais (`.env`)
//! ```env
//! AZURE_SPEECH_KEY=<chave>            # ou usa AZURE_OPENAI_API_KEY como fallback
//! AZURE_SPEECH_ENDPOINT=<url>         # ou usa AZURE_OPENAI_ENDPOINT como fallback
//! AZURE_SPEECH_LANGUAGE=pt-BR         # opcional (padrão: pt-BR)
//! AZURE_SPEECH_MAX_SPEAKERS=10        # opcional (padrão: 10)
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

pub mod azure_speech;

use std::fmt;
use std::path::Path;

// ---------------------------------------------------------------------------
// Configuração
// ---------------------------------------------------------------------------

/// Credenciais e opções para o Azure AI Speech.
#[derive(Debug, Clone)]
pub struct TranscriptionConfig {
    /// Chave de API do Speech (`AZURE_SPEECH_KEY`).
    /// Fallback: `AZURE_OPENAI_API_KEY`.
    pub speech_key: String,

    /// URL base do recurso (`AZURE_SPEECH_ENDPOINT`).
    /// Fallback: `AZURE_OPENAI_ENDPOINT`.
    pub speech_endpoint: String,

    /// Idioma para transcrição, ex.: `"pt-BR"`.
    /// `None` usa o padrão do servidor.
    pub language: Option<String>,

    /// Número máximo de falantes para diarização (padrão: 10).
    pub max_speakers: u32,
}

impl TranscriptionConfig {
    /// Lê as credenciais das variáveis de ambiente.
    ///
    /// Chame `dotenvy::dotenv().ok()` antes para carregar o `.env`.
    pub fn from_env() -> Result<Self, TranscriberError> {
        // Speech key: tenta AZURE_SPEECH_KEY, depois AZURE_OPENAI_API_KEY
        let speech_key = std::env::var("AZURE_SPEECH_KEY")
            .or_else(|_| std::env::var("AZURE_OPENAI_API_KEY"))
            .map_err(|_| TranscriberError::Config(
                "AZURE_SPEECH_KEY ou AZURE_OPENAI_API_KEY não definido".to_string()
            ))?;

        // Endpoint: tenta AZURE_SPEECH_ENDPOINT, depois AZURE_OPENAI_ENDPOINT
        let speech_endpoint = std::env::var("AZURE_SPEECH_ENDPOINT")
            .or_else(|_| std::env::var("AZURE_OPENAI_ENDPOINT"))
            .map_err(|_| TranscriberError::Config(
                "AZURE_SPEECH_ENDPOINT ou AZURE_OPENAI_ENDPOINT não definido".to_string()
            ))?;

        let language = std::env::var("AZURE_SPEECH_LANGUAGE")
            .ok()
            .or_else(|| Some("pt-BR".to_string()));

        let max_speakers = std::env::var("AZURE_SPEECH_MAX_SPEAKERS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        Ok(Self { speech_key, speech_endpoint, language, max_speakers })
    }
}

// ---------------------------------------------------------------------------
// Tipos de resultado
// ---------------------------------------------------------------------------

/// Resultado completo da transcrição.
#[derive(Debug)]
pub struct TranscriptionResult {
    /// Texto completo concatenado de todos os falantes.
    pub full_text: String,

    /// Segmentos por falante com timestamps e confiança.
    pub segments: Vec<Segment>,

    /// Duração total do áudio em millisegundos.
    pub duration_ms: Option<u64>,
}

/// Um segmento de fala com falante, texto e timestamps.
#[derive(Debug, PartialEq)]
pub struct Segment {
    /// Rótulo do falante, ex.: `"Speaker 1"`. `None` se não detectado.
    pub speaker: Option<String>,
    /// Texto transcrito.
    pub text: String,
    /// Início em millisegundos.
    pub start_ms: Option<u64>,
    /// Fim em millisegundos.
    pub end_ms: Option<u64>,
    /// Confiança da transcrição (0.0–1.0).
    pub confidence: Option<f64>,
}

// ---------------------------------------------------------------------------
// Tipo de erro
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum TranscriberError {
    Config(String),
    Io(String),
    Http(String),
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

/// Transcreve um arquivo de áudio com diarização via Azure AI Speech.
///
/// A Fast Transcription API suporta até ~200 MB / 4 horas em uma requisição.
/// Para arquivos maiores, [`transcribe_chunked`] divide em partes de 100 MB.
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

    let file_size = audio_path.metadata()
        .map(|m| m.len())
        .unwrap_or(0);

    // Limite conservador: 180 MB (API aceita até ~200 MB)
    if file_size > 180 * 1024 * 1024 {
        let duration_s = audio_duration_secs(audio_path).unwrap_or(0);
        return transcribe_chunked(audio_path, config, duration_s);
    }

    transcribe_single(audio_path, config)
}

// ---------------------------------------------------------------------------
// Transcrição simples e com chunking
// ---------------------------------------------------------------------------

fn transcribe_single(
    audio_path: &Path,
    config: &TranscriptionConfig,
) -> Result<TranscriptionResult, TranscriberError> {
    let raw = azure_speech::transcribe(audio_path, config)?;

    // Texto completo de combinedPhrases (já concatenado pela API)
    let full_text = raw.combined_phrases
        .as_ref()
        .and_then(|p| p.first())
        .map(|p| p.text.trim().to_string())
        .unwrap_or_default();

    let segments: Vec<Segment> = raw.phrases
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !p.text.trim().is_empty())
        .map(|p| Segment {
            speaker:    p.speaker.map(|id| format!("Speaker {id}")),
            text:       p.text.trim().to_string(),
            start_ms:   Some(p.offset_ms),
            end_ms:     Some(p.offset_ms + p.duration_ms),
            confidence: p.confidence,
        })
        .collect();

    Ok(TranscriptionResult {
        full_text,
        segments,
        duration_ms: raw.duration_ms,
    })
}

/// Divide o áudio em partes de `chunk_secs` segundos e concatena os resultados.
fn transcribe_chunked(
    audio_path: &Path,
    config: &TranscriptionConfig,
    duration_s: u64,
) -> Result<TranscriptionResult, TranscriberError> {
    const CHUNK_SECS: u64 = 3_000; // ~50 min, ~96 MB por chunk (WAV 32 KB/s)

    let n_chunks = (duration_s as f64 / CHUNK_SECS as f64).ceil() as u64;
    eprintln!("[chunking] {duration_s}s → {n_chunks} partes de {CHUNK_SECS}s");

    let tmp_dir = std::env::temp_dir().join("rust_stt_chunks");
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| TranscriberError::Io(e.to_string()))?;

    let stem = audio_path.file_stem().and_then(|s| s.to_str()).unwrap_or("chunk");

    let mut all_segments: Vec<Segment> = Vec::new();
    let mut full_texts: Vec<String>    = Vec::new();
    let mut total_duration_ms: u64     = 0;

    for i in 0..n_chunks {
        let start_s     = i * CHUNK_SECS;
        let chunk_path  = tmp_dir.join(format!("{stem}_chunk_{i:02}.wav"));
        let offset_ms   = start_s * 1000;

        eprintln!("  Parte {}/{n_chunks} ({start_s}s…)", i + 1);
        extract_chunk(audio_path, &chunk_path, start_s, CHUNK_SECS)?;

        let result = transcribe_single(&chunk_path, config)?;
        let _ = std::fs::remove_file(&chunk_path);

        full_texts.push(result.full_text);
        total_duration_ms += result.duration_ms.unwrap_or(0);

        // Ajusta timestamps para posição global no áudio
        for mut seg in result.segments {
            seg.start_ms = seg.start_ms.map(|t| t + offset_ms);
            seg.end_ms   = seg.end_ms.map(|t| t + offset_ms);
            all_segments.push(seg);
        }
    }

    let _ = std::fs::remove_dir(&tmp_dir);

    Ok(TranscriptionResult {
        full_text:   full_texts.join(" "),
        segments:    all_segments,
        duration_ms: Some(total_duration_ms),
    })
}

/// Extrai um trecho do áudio com ffmpeg.
fn extract_chunk(
    input: &Path,
    output: &std::path::PathBuf,
    start_s: u64,
    duration_s: u64,
) -> Result<(), TranscriberError> {
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y", "-i", &input.to_string_lossy(),
            "-ss", &start_s.to_string(),
            "-t",  &duration_s.to_string(),
            "-acodec", "pcm_s16le", "-ac", "1", "-ar", "16000",
            &output.to_string_lossy(),
        ])
        .output()
        .map_err(|e| TranscriberError::Io(format!("ffmpeg: {e}")))?;

    if !status.status.success() {
        return Err(TranscriberError::Io(
            String::from_utf8_lossy(&status.stderr).into_owned()
        ));
    }
    Ok(())
}

/// Duração do arquivo em segundos via ffprobe.
fn audio_duration_secs(path: &Path) -> Option<u64> {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v", "error", "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
            &path.to_string_lossy(),
        ])
        .output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().map(|d| d as u64).ok()
}

// ---------------------------------------------------------------------------
// Formatação
// ---------------------------------------------------------------------------

impl TranscriptionResult {
    /// Formata a transcrição com timestamps e falantes para exibição no terminal.
    ///
    /// Exemplo:
    /// ```text
    /// [00:00] Speaker 1: Bom dia a todos.
    /// [00:03] Speaker 2: Olá, vamos começar.
    /// ```
    pub fn format_output(&self) -> String {
        if self.segments.is_empty() {
            return self.full_text.clone();
        }

        let has_speakers = self.segments.iter().any(|s| s.speaker.is_some());
        let has_timestamps = self.segments.iter().any(|s| s.start_ms.is_some());

        self.segments
            .iter()
            .map(|s| {
                let ts = if has_timestamps {
                    s.start_ms
                        .map(|ms| format!("[{}] ", ms_to_time(ms)))
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                if has_speakers {
                    let speaker = s.speaker.as_deref().unwrap_or("Speaker ?");
                    format!("{ts}{speaker}: {}", s.text)
                } else {
                    format!("{ts}{}", s.text)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Serializa o resultado completo como JSON formatado.
    pub fn to_json(&self) -> String {
        let speakers: std::collections::HashSet<String> = self.segments
            .iter()
            .filter_map(|s| s.speaker.clone())
            .collect();

        let segments: Vec<serde_json::Value> = self.segments
            .iter()
            .map(|s| serde_json::json!({
                "speaker":     s.speaker,
                "text":        s.text,
                "start_ms":    s.start_ms,
                "end_ms":      s.end_ms,
                "start_time":  s.start_ms.map(ms_to_time),
                "confidence":  s.confidence
            }))
            .collect();

        serde_json::to_string_pretty(&serde_json::json!({
            "full_text":        self.full_text,
            "duration_ms":      self.duration_ms,
            "speakers_detected": !speakers.is_empty(),
            "speaker_count":    speakers.len(),
            "speakers":         speakers.into_iter().collect::<Vec<_>>(),
            "segment_count":    self.segments.len(),
            "segments":         segments
        }))
        .unwrap_or_default()
    }
}

/// Converte millisegundos para `MM:SS`.
pub fn ms_to_time(ms: u64) -> String {
    let total_s = ms / 1000;
    format!("{:02}:{:02}", total_s / 60, total_s % 60)
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(speaker: &str, text: &str, start_ms: u64, end_ms: u64) -> Segment {
        Segment {
            speaker:    Some(speaker.to_string()),
            text:       text.to_string(),
            start_ms:   Some(start_ms),
            end_ms:     Some(end_ms),
            confidence: Some(0.95),
        }
    }

    #[test]
    fn ms_to_time_formata_corretamente() {
        assert_eq!(ms_to_time(0),       "00:00");
        assert_eq!(ms_to_time(5_000),   "00:05");
        assert_eq!(ms_to_time(65_000),  "01:05");
        assert_eq!(ms_to_time(3_600_000), "60:00");
    }

    #[test]
    fn format_output_com_speaker_e_timestamp() {
        let result = TranscriptionResult {
            full_text:   "Bom dia. Olá.".to_string(),
            duration_ms: Some(5_000),
            segments: vec![
                seg("Speaker 1", "Bom dia.", 0, 2_000),
                seg("Speaker 2", "Olá.",     2_500, 4_000),
            ],
        };
        let out = result.format_output();
        assert!(out.contains("[00:00] Speaker 1: Bom dia."), "got: {out}");
        assert!(out.contains("[00:02] Speaker 2: Olá."),    "got: {out}");
    }

    #[test]
    fn format_output_sem_segmentos_retorna_full_text() {
        let result = TranscriptionResult {
            full_text:   "Texto.".to_string(),
            duration_ms: None,
            segments:    vec![],
        };
        assert_eq!(result.format_output(), "Texto.");
    }

    #[test]
    fn to_json_contem_campos_obrigatorios() {
        let result = TranscriptionResult {
            full_text:   "Teste.".to_string(),
            duration_ms: Some(3_000),
            segments:    vec![seg("Speaker 1", "Teste.", 0, 3_000)],
        };
        let json = result.to_json();
        assert!(json.contains("\"full_text\""),        "got: {json}");
        assert!(json.contains("\"speakers_detected\""),"got: {json}");
        assert!(json.contains("\"speaker_count\""),    "got: {json}");
        assert!(json.contains("\"start_ms\""),         "got: {json}");
        assert!(json.contains("\"start_time\""),       "got: {json}");
        assert!(json.contains("\"confidence\""),       "got: {json}");
    }

    #[test]
    fn to_json_lista_falantes_unicos() {
        let result = TranscriptionResult {
            full_text:   "A. B. A.".to_string(),
            duration_ms: None,
            segments: vec![
                seg("Speaker 1", "A.", 0,      1_000),
                seg("Speaker 2", "B.", 1_500,  2_500),
                seg("Speaker 1", "A.", 3_000,  4_000),
            ],
        };
        let json = result.to_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["speaker_count"], 2);
        assert_eq!(v["segment_count"], 3);
    }

    #[test]
    fn config_from_env_falha_sem_credenciais() {
        // Testa que a leitura de env var ausente retorna Config error
        let result = std::env::var("AZURE_VARIAVEL_INEXISTENTE_XYZ");
        assert!(result.is_err());
    }
}
