//! Cliente HTTP para o Azure AI Speech — Fast Transcription API.
//!
//! Endpoint:
//! ```text
//! POST {endpoint}/speechtotext/transcriptions:transcribe?api-version=2024-11-15
//! ```
//!
//! ## Schema real da resposta
//! ```json
//! {
//!   "durationMilliseconds": 38000,
//!   "combinedPhrases": [{ "channel": 0, "text": "Texto completo..." }],
//!   "phrases": [
//!     {
//!       "channel": 0,
//!       "speaker": 1,
//!       "offsetMilliseconds": 0,
//!       "durationMilliseconds": 2500,
//!       "text": "Bom dia a todos.",
//!       "words": [{ "text": "Bom", "offsetMilliseconds": 0, "durationMilliseconds": 300 }]
//!     }
//!   ]
//! }
//! ```

use reqwest::blocking::multipart;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

use super::{TranscriberError, TranscriptionConfig};

const API_VERSION: &str = "2024-11-15";

// ---------------------------------------------------------------------------
// Estruturas da resposta
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SpeechResponse {
    #[serde(rename = "durationMilliseconds")]
    pub duration_ms: Option<u64>,

    #[serde(rename = "combinedPhrases")]
    pub combined_phrases: Option<Vec<CombinedPhrase>>,

    pub phrases: Option<Vec<SpeechPhrase>>,
}

#[derive(Debug, Deserialize)]
pub struct CombinedPhrase {
    pub channel: Option<u32>,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct SpeechPhrase {
    #[serde(rename = "offsetMilliseconds")]
    pub offset_ms: u64,

    #[serde(rename = "durationMilliseconds")]
    pub duration_ms: u64,

    pub text: String,

    /// ID numérico do falante (1, 2, 3...). Presente quando diarização ativada.
    pub speaker: Option<u32>,

    pub confidence: Option<f64>,
    pub locale: Option<String>,
}

// ---------------------------------------------------------------------------
// Função pública
// ---------------------------------------------------------------------------

/// Envia o áudio para o Azure AI Speech e devolve a resposta com diarização.
pub fn transcribe(
    audio_path: &Path,
    config: &TranscriptionConfig,
) -> Result<SpeechResponse, TranscriberError> {
    let url  = build_url(config);
    let form = build_form(audio_path, config)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| TranscriberError::Http(e.to_string()))?;

    let response = client
        .post(&url)
        // Azure AI Speech usa Ocp-Apim-Subscription-Key, não api-key
        .header("Ocp-Apim-Subscription-Key", &config.speech_key)
        .multipart(form)
        .send()
        .map_err(|e| TranscriberError::Http(format!("Falha na requisição: {e}")))?;

    let status = response.status();
    let body   = response
        .text()
        .map_err(|e| TranscriberError::Http(format!("Falha ao ler resposta: {e}")))?;

    if !status.is_success() {
        return Err(TranscriberError::Http(format!(
            "Azure Speech retornou {status}:\n{body}"
        )));
    }

    serde_json::from_str::<SpeechResponse>(&body)
        .map_err(|e| TranscriberError::Parse(format!("JSON inválido: {e}\nBody: {body}")))
}

// ---------------------------------------------------------------------------
// Helpers privados
// ---------------------------------------------------------------------------

fn build_url(config: &TranscriptionConfig) -> String {
    let endpoint = config.speech_endpoint.trim_end_matches('/');
    format!(
        "{}/speechtotext/transcriptions:transcribe?api-version={API_VERSION}",
        endpoint
    )
}

fn build_form(
    audio_path: &Path,
    config: &TranscriptionConfig,
) -> Result<multipart::Form, TranscriberError> {
    let bytes    = std::fs::read(audio_path).map_err(|e| TranscriberError::Io(e.to_string()))?;
    let filename = audio_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.wav")
        .to_string();

    let audio_part = multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str("audio/wav")
        .map_err(|e| TranscriberError::Http(e.to_string()))?;

    // Definição de transcrição com diarização
    let definition = json!({
        "locales": [config.language.as_deref().unwrap_or("pt-BR")],
        "diarization": {
            "enabled": true,
            "maxSpeakers": config.max_speakers
        },
        "channels": [0],
        "profanityFilterMode": "None"
    });

    let def_part = multipart::Part::text(definition.to_string())
        .mime_str("application/json")
        .map_err(|e| TranscriberError::Http(e.to_string()))?;

    Ok(multipart::Form::new()
        .part("audio", audio_part)
        .part("definition", def_part))
}
