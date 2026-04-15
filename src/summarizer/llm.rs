//! Cliente Azure OpenAI para o summarizer — chat completions.
//!
//! Endpoint:
//! ```text
//! POST {endpoint}/openai/deployments/{deployment}/chat/completions
//!      ?api-version={api_version}
//! ```

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::{SummarizerError, SummarizerConfig};

// ---------------------------------------------------------------------------
// Estruturas da requisição
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ChatRequest<'a> {
    messages:               Vec<Message<'a>>,
    temperature:            f32,
    max_completion_tokens:  u32,
    response_format:        ResponseFormat,
}

#[derive(Serialize)]
struct Message<'a> {
    role:    &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

// ---------------------------------------------------------------------------
// Estruturas da resposta
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: String,
}

// ---------------------------------------------------------------------------
// Função pública
// ---------------------------------------------------------------------------

/// Chama o Azure OpenAI com o prompt montado e devolve o JSON de resposta.
pub fn complete(
    system_prompt: &str,
    user_prompt:   &str,
    config:        &SummarizerConfig,
) -> Result<String, SummarizerError> {
    let url = format!(
        "{}/openai/deployments/{}/chat/completions?api-version={}",
        config.endpoint.trim_end_matches('/'),
        config.deployment,
        config.api_version
    );

    let body = ChatRequest {
        messages: vec![
            Message { role: "system", content: system_prompt },
            Message { role: "user",   content: user_prompt   },
        ],
        temperature:           0.1,
        max_completion_tokens: 4_096,
        response_format:       ResponseFormat { kind: "json_object" },
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| SummarizerError::Http(e.to_string()))?;

    let resp = client
        .post(&url)
        .header("api-key",       &config.api_key)
        .header("Content-Type",  "application/json")
        .json(&body)
        .send()
        .map_err(|e| SummarizerError::Http(format!("Falha na requisição: {e}")))?;

    let status = resp.status();
    let text   = resp.text()
        .map_err(|e| SummarizerError::Http(format!("Falha ao ler resposta: {e}")))?;

    if !status.is_success() {
        return Err(SummarizerError::Http(format!(
            "Azure OpenAI retornou {status}:\n{text}"
        )));
    }

    let parsed: ChatResponse = serde_json::from_str(&text)
        .map_err(|e| SummarizerError::Parse(format!("Resposta inválida: {e}\n{text}")))?;

    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| SummarizerError::Parse("Resposta sem choices".to_string()))
}
