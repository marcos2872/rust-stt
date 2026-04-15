//! Cliente HTTP para o endpoint de transcrição de áudio do Azure OpenAI.
//!
//! Faz uma requisição `multipart/form-data` para:
//! ```text
//! POST {endpoint}/openai/deployments/{deployment}/audio/transcriptions
//!      ?api-version={api_version}
//! ```
//!
//! ## Schema real da resposta (`gpt-4o-transcribe-diarize`)
//! ```json
//! {
//!   "text": "Transcrição completa...",
//!   "usage": {
//!     "type": "tokens",
//!     "total_tokens": 414,
//!     "input_tokens": 100,
//!     "output_tokens": 314,
//!     "input_token_details": { "text_tokens": 0, "audio_tokens": 100 }
//!   }
//! }
//! ```
//!
//! > **Nota:** `verbose_json` e `segments` não são suportados por este modelo.
//! > A diarização pode aparecer embutida no campo `text` como marcadores
//! > (ex.: `"Speaker 1: ... Speaker 2: ..."`) dependendo do conteúdo.

use reqwest::blocking::multipart;
use serde::Deserialize;
use std::path::Path;

use super::{TranscriberError, TranscriptionConfig};

// ---------------------------------------------------------------------------
// Estruturas da resposta Azure OpenAI
// ---------------------------------------------------------------------------

/// Resposta do endpoint `audio/transcriptions` com `response_format=json`.
#[derive(Debug, Deserialize)]
pub struct AzureResponse {
    /// Transcrição completa. Pode conter marcadores de falante embutidos
    /// dependendo do modelo e conteúdo do áudio.
    pub text: String,
    /// Estatísticas de uso de tokens (cobranças).
    pub usage: Option<AzureUsage>,
}

/// Estatísticas de tokens consumidos na requisição.
#[derive(Debug, Deserialize)]
pub struct AzureUsage {
    pub total_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

// ---------------------------------------------------------------------------
// Função pública
// ---------------------------------------------------------------------------

/// Envia o arquivo de áudio para o Azure OpenAI e devolve a resposta.
pub fn transcribe(
    audio_path: &Path,
    config: &TranscriptionConfig,
) -> Result<AzureResponse, TranscriberError> {
    let url  = build_url(config);
    let form = build_form(audio_path, config)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| TranscriberError::Http(e.to_string()))?;

    let response = client
        .post(&url)
        .header("api-key", &config.api_key)
        .multipart(form)
        .send()
        .map_err(|e| TranscriberError::Http(format!("Falha na requisição: {e}")))?;

    let status = response.status();
    let body   = response
        .text()
        .map_err(|e| TranscriberError::Http(format!("Falha ao ler resposta: {e}")))?;

    if !status.is_success() {
        return Err(TranscriberError::Http(format!(
            "Azure OpenAI retornou {status}:\n{body}"
        )));
    }

    serde_json::from_str::<AzureResponse>(&body)
        .map_err(|e| TranscriberError::Parse(format!("JSON inválido: {e}\nBody: {body}")))
}

// ---------------------------------------------------------------------------
// Helpers privados
// ---------------------------------------------------------------------------

fn build_url(config: &TranscriptionConfig) -> String {
    let endpoint = config.endpoint.trim_end_matches('/');
    format!(
        "{}/openai/deployments/{}/audio/transcriptions?api-version={}",
        endpoint, config.deployment, config.api_version
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

    let file_part = multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str("audio/wav")
        .map_err(|e| TranscriberError::Http(e.to_string()))?;

    let mut form = multipart::Form::new()
        .part("file", file_part)
        // verbose_json não é suportado; json retorna { text, usage }
        .text("response_format", "json")
        // obrigatório para modelos de diarização
        .text("chunking_strategy", "auto");

    if let Some(lang) = &config.language {
        form = form.text("language", lang.clone());
    }

    Ok(form)
}
