//! Minutes — gera ata estruturada no formato FLOW a partir de um summary JSON.
//!
//! # Fluxo
//! 1. Carrega `summary.json` (saída do `summarize`)
//! 2. Serializa o JSON completo (com toda a transcrição) como contexto do LLM
//! 3. Chama o Azure OpenAI que preenche o template FLOW em JSON
//! 4. Valida os campos obrigatórios e persiste `<stem>_minutes.json`
//!
//! # Credenciais (`.env`)
//! As mesmas utilizadas pelo `summarize`:
//! ```env
//! AZURE_OPENAI_API_KEY=<chave>
//! AZURE_OPENAI_ENDPOINT=<url>
//! AZURE_OPENAI_DEPLOYMENT=gpt-5.4-mini
//! AZURE_OPENAI_API_VERSION=2025-01-01-preview   # opcional
//! ```
//!
//! # Exemplo
//! ```no_run
//! use std::path::Path;
//! use rust_stt::minutes::{MinutesConfig, generate_minutes};
//! let cfg    = MinutesConfig::from_env().unwrap();
//! let result = generate_minutes(Path::new("summary.json"), &cfg).unwrap();
//! println!("{}", result.to_json());
//! ```

pub mod llm;
pub mod prompt;

use std::fmt;
use std::path::Path;

// ---------------------------------------------------------------------------
// Configuração
// ---------------------------------------------------------------------------

/// Configuração do gerador de atas (credenciais Azure OpenAI).
#[derive(Debug, Clone)]
pub struct MinutesConfig {
    pub api_key:     String,
    pub endpoint:    String,
    pub deployment:  String,
    pub api_version: String,
}

impl MinutesConfig {
    /// Lê a configuração das variáveis de ambiente `AZURE_OPENAI_*`.
    pub fn from_env() -> Result<Self, MinutesError> {
        let api_key = std::env::var("AZURE_OPENAI_API_KEY")
            .map_err(|_| MinutesError::Config("AZURE_OPENAI_API_KEY ausente".into()))?;
        let endpoint = std::env::var("AZURE_OPENAI_ENDPOINT")
            .map_err(|_| MinutesError::Config("AZURE_OPENAI_ENDPOINT ausente".into()))?;
        let deployment = std::env::var("AZURE_OPENAI_DEPLOYMENT")
            .map_err(|_| MinutesError::Config("AZURE_OPENAI_DEPLOYMENT ausente".into()))?;
        let api_version = std::env::var("AZURE_OPENAI_API_VERSION")
            .unwrap_or_else(|_| "2025-01-01-preview".to_string());

        Ok(Self { api_key, endpoint, deployment, api_version })
    }
}

// ---------------------------------------------------------------------------
// Resultado
// ---------------------------------------------------------------------------

/// Resultado do gerador de atas — envelopa o JSON completo da ata.
#[derive(Debug)]
pub struct MinutesResult {
    pub minutes:     serde_json::Value,
    /// Uso de tokens reportado pela Azure OpenAI.
    pub token_usage: llm::TokenUsage,
}

impl MinutesResult {
    /// Serializa a ata como JSON formatado (inclui uso de tokens como metadado).
    pub fn to_json(&self) -> String {
        let mut output = self.minutes.clone();
        output["token_usage"] = serde_json::json!({
            "prompt_tokens":     self.token_usage.prompt_tokens,
            "completion_tokens": self.token_usage.completion_tokens,
            "total_tokens":      self.token_usage.total_tokens,
        });
        serde_json::to_string_pretty(&output).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Erro
// ---------------------------------------------------------------------------

/// Erros possíveis no pipeline de geração de ata.
#[derive(Debug)]
pub enum MinutesError {
    Config(String),
    Io(String),
    Http(String),
    Parse(String),
}

impl fmt::Display for MinutesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(m) => write!(f, "Configuração: {m}"),
            Self::Io(m)     => write!(f, "I/O: {m}"),
            Self::Http(m)   => write!(f, "HTTP: {m}"),
            Self::Parse(m)  => write!(f, "Parse: {m}"),
        }
    }
}

impl std::error::Error for MinutesError {}

// ---------------------------------------------------------------------------
// Função pública principal
// ---------------------------------------------------------------------------

/// Lê o summary JSON, chama o LLM e retorna a ata estruturada.
pub fn generate_minutes(
    summary_path: &Path,
    config:       &MinutesConfig,
) -> Result<MinutesResult, MinutesError> {
    // 1. Carrega e parseia o summary JSON
    let json_str = std::fs::read_to_string(summary_path)
        .map_err(|e| MinutesError::Io(format!("summary: {e}")))?;
    let summary: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| MinutesError::Parse(format!("JSON inválido: {e}")))?;

    // 2. Monta os prompts
    let system = prompt::build_system_prompt();
    let user   = prompt::build_user_prompt(&summary);

    // 3. Chama o LLM
    eprintln!("Gerando ata via {}...", config.deployment);
    let (raw, token_usage) = llm::complete(system, &user, config)?;

    // 4. Parseia a resposta
    let minutes: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
        let trecho = &raw[..raw.len().min(400)];
        MinutesError::Parse(format!("LLM retornou JSON inválido: {e}\n{trecho}"))
    })?;

    // 5. Valida campos obrigatórios
    validate_required_fields(&minutes)?;

    Ok(MinutesResult { minutes, token_usage })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Verifica presença dos campos de primeiro nível obrigatórios.
fn validate_required_fields(v: &serde_json::Value) -> Result<(), MinutesError> {
    let required = [
        "meeting_data", "participants", "context",
        "topics", "decisions", "open_points",
        "materials", "post_meeting_analysis", "action_plan",
    ];
    for field in required {
        if v.get(field).is_none() {
            return Err(MinutesError::Parse(
                format!("Campo obrigatório ausente na resposta do LLM: {field}")
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_error_display_config() {
        let e = MinutesError::Config("chave ausente".into());
        assert_eq!(e.to_string(), "Configuração: chave ausente");
    }

    #[test]
    fn minutes_error_display_io() {
        let e = MinutesError::Io("arquivo não encontrado".into());
        assert_eq!(e.to_string(), "I/O: arquivo não encontrado");
    }

    #[test]
    fn minutes_error_display_http() {
        let e = MinutesError::Http("timeout".into());
        assert_eq!(e.to_string(), "HTTP: timeout");
    }

    #[test]
    fn minutes_error_display_parse() {
        let e = MinutesError::Parse("JSON inválido".into());
        assert_eq!(e.to_string(), "Parse: JSON inválido");
    }

    #[test]
    fn minutes_result_to_json_contem_campos_obrigatorios() {
        let v = serde_json::json!({
            "meeting_data":          { "title": "Reunião Teste" },
            "participants":          [],
            "context":               { "objective": "Alinhar escopo" },
            "topics":                [],
            "decisions":             [],
            "open_points":           [],
            "materials":             { "received": [], "to_send": [] },
            "post_meeting_analysis": { "key_quotes": [] },
            "action_plan":           { "todos": [] }
        });
        let result = MinutesResult {
            minutes: v,
            token_usage: llm::TokenUsage::default(),
        };
        let json   = result.to_json();
        assert!(json.contains("meeting_data"));
        assert!(json.contains("participants"));
        assert!(json.contains("action_plan"));
    }

    #[test]
    fn validate_required_fields_falha_com_campo_ausente() {
        let v = serde_json::json!({ "meeting_data": {} });
        assert!(validate_required_fields(&v).is_err());
    }

    #[test]
    fn validate_required_fields_passa_com_todos_presentes() {
        let v = serde_json::json!({
            "meeting_data": {}, "participants": [], "context": {},
            "topics": [], "decisions": [], "open_points": [],
            "materials": {}, "post_meeting_analysis": {}, "action_plan": {}
        });
        assert!(validate_required_fields(&v).is_ok());
    }
}
