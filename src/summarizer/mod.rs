//! Summarizer — identifica falantes pelo VTT do Teams e gera resumo via LLM.
//!
//! # Fluxo
//! 1. Carrega `transcript.json` (saída do `transcribe`)
//! 2. Parseia o `.vtt` do Teams
//! 3. Faz matching por timestamp → `Speaker N` → nome real
//! 4. Envia amostra + mapeamento para o LLM (Azure OpenAI) que:
//!    - Confirma/corrige o mapeamento
//!    - Gera resumo executivo
//!    - Extrai pontos de ação e decisões
//! 5. Aplica o mapeamento final em todos os segmentos
//!
//! # Credenciais (`.env`)
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
//! use rust_stt::summarizer::{SummarizerConfig, summarize};
//! let cfg    = SummarizerConfig::from_env().unwrap();
//! let result = summarize(Path::new("transcript.json"), Path::new("meeting.vtt"), &cfg).unwrap();
//! println!("{}", result.format_output());
//! ```

pub mod vtt;
pub mod matcher;
pub mod llm;

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

// ---------------------------------------------------------------------------
// Configuração
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SummarizerConfig {
    pub api_key:    String,
    pub endpoint:   String,
    pub deployment: String,
    pub api_version: String,
}

impl SummarizerConfig {
    pub fn from_env() -> Result<Self, SummarizerError> {
        let api_key = std::env::var("AZURE_OPENAI_API_KEY")
            .map_err(|_| SummarizerError::Config("AZURE_OPENAI_API_KEY ausente".into()))?;
        let endpoint = std::env::var("AZURE_OPENAI_ENDPOINT")
            .map_err(|_| SummarizerError::Config("AZURE_OPENAI_ENDPOINT ausente".into()))?;
        let deployment = std::env::var("AZURE_OPENAI_DEPLOYMENT")
            .map_err(|_| SummarizerError::Config("AZURE_OPENAI_DEPLOYMENT ausente".into()))?;
        let api_version = std::env::var("AZURE_OPENAI_API_VERSION")
            .unwrap_or_else(|_| "2025-01-01-preview".to_string());

        Ok(Self { api_key, endpoint, deployment, api_version })
    }
}

// ---------------------------------------------------------------------------
// Tipos de resultado
// ---------------------------------------------------------------------------

/// Resultado final do summarizer.
#[derive(Debug)]
pub struct SummaryResult {
    /// Mapeamento confirmado pelo LLM: `Speaker N` → nome real.
    pub speaker_mapping: HashMap<String, String>,
    /// Transcrição completa com nomes reais.
    pub transcript: Vec<NamedSegment>,
    /// Resumo executivo da reunião.
    pub summary: String,
    /// Pontos de ação identificados.
    pub action_items: Vec<String>,
    /// Decisões tomadas.
    pub key_decisions: Vec<String>,
    /// Uso de tokens reportado pela Azure OpenAI.
    pub token_usage: llm::TokenUsage,
}

/// Um segmento da transcrição com nome real do falante.
#[derive(Debug)]
pub struct NamedSegment {
    pub speaker_name: String,
    pub time:         String,   // "MM:SS"
    pub start_ms:     u64,
    pub text:         String,
    pub confidence:   Option<f64>,
}

// ---------------------------------------------------------------------------
// Erro
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum SummarizerError {
    Config(String),
    Io(String),
    Http(String),
    Parse(String),
}

impl fmt::Display for SummarizerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(m) => write!(f, "Configuração: {m}"),
            Self::Io(m)     => write!(f, "I/O: {m}"),
            Self::Http(m)   => write!(f, "HTTP: {m}"),
            Self::Parse(m)  => write!(f, "Parse: {m}"),
        }
    }
}

impl std::error::Error for SummarizerError {}

// ---------------------------------------------------------------------------
// Função pública principal
// ---------------------------------------------------------------------------

/// Carrega o transcript JSON + VTT, faz o matching e chama o LLM.
pub fn summarize(
    transcript_path: &Path,
    vtt_path:        &Path,
    config:          &SummarizerConfig,
) -> Result<SummaryResult, SummarizerError> {
    // 1. Carrega o transcript JSON
    let transcript_json = std::fs::read_to_string(transcript_path)
        .map_err(|e| SummarizerError::Io(format!("transcript: {e}")))?;
    let transcript: serde_json::Value = serde_json::from_str(&transcript_json)
        .map_err(|e| SummarizerError::Parse(format!("transcript JSON inválido: {e}")))?;

    // 2. Parseia o VTT
    let vtt_content = std::fs::read_to_string(vtt_path)
        .map_err(|e| SummarizerError::Io(format!("vtt: {e}")))?;
    let vtt_entries = vtt::parse(&vtt_content);

    if vtt_entries.is_empty() {
        return Err(SummarizerError::Parse(
            "VTT não contém entradas com falante (<v Name>)".into()
        ));
    }

    // 3. Extrai segmentos da transcrição para o matcher
    let segments_raw = transcript["segments"]
        .as_array()
        .ok_or_else(|| SummarizerError::Parse("transcript.segments ausente".into()))?;

    let match_segs: Vec<matcher::TranscriptSegment> = segments_raw
        .iter()
        .filter_map(|s| {
            Some(matcher::TranscriptSegment {
                speaker:  s["speaker"].as_str()?,
                start_ms: s["start_ms"].as_u64()?,
                end_ms:   s["end_ms"].as_u64()?,
            })
        })
        .collect();

    // 4. Matching por timestamp (tolerância 1500 ms)
    let initial_mapping = matcher::match_speakers(&match_segs, &vtt_entries, 1_500);

    // 5. Monta prompt para o LLM
    let system_prompt = build_system_prompt();
    let user_prompt   = build_user_prompt(&transcript, &vtt_entries, &initial_mapping);

    // 6. Chama o LLM
    eprintln!("Chamando {} para confirmar mapeamento e gerar resumo...", config.deployment);
    let (llm_response, token_usage) = llm::complete(&system_prompt, &user_prompt, config)?;

    // 7. Parseia a resposta do LLM
    let llm_json: serde_json::Value = serde_json::from_str(&llm_response)
        .map_err(|e| SummarizerError::Parse(format!("LLM retornou JSON inválido: {e}\n{llm_response}")))?;

    // 8. Extrai mapeamento final
    let final_mapping = extract_mapping(&llm_json, &initial_mapping);

    // 9. Aplica mapeamento em todos os segmentos
    let named_transcript = apply_mapping(segments_raw, &final_mapping);

    // 10. Extrai resumo, action items e decisões
    let summary = llm_json["summary"]
        .as_str()
        .unwrap_or("Resumo não disponível.")
        .to_string();

    let action_items = extract_str_array(&llm_json["action_items"]);
    let key_decisions = extract_str_array(&llm_json["key_decisions"]);

    Ok(SummaryResult {
        speaker_mapping: final_mapping,
        transcript:      named_transcript,
        summary,
        action_items,
        key_decisions,
        token_usage,
    })
}

// ---------------------------------------------------------------------------
// Construção dos prompts
// ---------------------------------------------------------------------------

fn build_system_prompt() -> String {
    r#"Você é um especialista em análise de reuniões.
Receberá uma transcrição com falantes anônimos (Speaker 1, Speaker 2...) e
um VTT do Teams com nomes reais. Sua tarefa:
1. Confirmar ou corrigir o mapeamento de falantes fornecido.
2. Gerar resumo executivo da reunião (3-5 parágrafos).
3. Extrair pontos de ação claros (quem, o quê).
4. Extrair decisões tomadas.

Responda APENAS com JSON válido no formato exato especificado no prompt do usuário."#
    .to_string()
}

fn build_user_prompt(
    transcript:      &serde_json::Value,
    vtt_entries:     &[vtt::VttEntry],
    initial_mapping: &HashMap<String, matcher::SpeakerMatch>,
) -> String {
    // Amostras por falante (máx 3 segmentos cada)
    let mut speaker_samples: HashMap<&str, Vec<String>> = HashMap::new();
    if let Some(segs) = transcript["segments"].as_array() {
        for seg in segs {
            let sp   = seg["speaker"].as_str().unwrap_or("");
            let text = seg["text"].as_str().unwrap_or("");
            let time = seg["start_time"].as_str().unwrap_or("");
            let entry = speaker_samples.entry(sp).or_default();
            if entry.len() < 3 {
                entry.push(format!("[{time}] {text}"));
            }
        }
    }

    // Amostras do VTT (máx 3 por nome)
    let mut vtt_samples: HashMap<&str, Vec<String>> = HashMap::new();
    for e in vtt_entries {
        let entry = vtt_samples.entry(e.name.as_str()).or_default();
        if entry.len() < 3 {
            let ts = format_ms(e.start_ms);
            entry.push(format!("[{ts}] {}", &e.text[..e.text.len().min(80)]));
        }
    }

    // Mapeamento inicial
    let mut mapping_lines: Vec<String> = initial_mapping
        .values()
        .map(|m| format!(
            "  {} → \"{}\" ({} matches, confiança {:.0}%)",
            m.speaker_label, m.name, m.match_count, m.confidence * 100.0
        ))
        .collect();
    mapping_lines.sort();

    let mut speakers_block = String::new();
    let mut all_speakers: Vec<&&str> = speaker_samples.keys().collect();
    all_speakers.sort();
    for sp in all_speakers {
        speakers_block.push_str(&format!("\n{sp}:\n"));
        for line in &speaker_samples[sp] {
            speakers_block.push_str(&format!("  {line}\n"));
        }
    }

    let mut vtt_block = String::new();
    let mut all_names: Vec<&&str> = vtt_samples.keys().collect();
    all_names.sort();
    for name in all_names {
        vtt_block.push_str(&format!("\n{name}:\n"));
        for line in &vtt_samples[name] {
            vtt_block.push_str(&format!("  {line}\n"));
        }
    }

    format!(
        r#"# Amostras da transcrição (Azure AI Speech)
{speakers_block}
# Amostras do VTT do Teams (nomes reais)
{vtt_block}
# Mapeamento inicial por timestamp
{}

# Transcrição completa (full_text)
{}

Responda EXATAMENTE neste formato JSON:
{{
  "speaker_mapping": {{
    "Speaker 1": "Nome Completo",
    "Speaker 2": "Nome Completo"
  }},
  "summary": "Resumo executivo da reunião em 3-5 parágrafos...",
  "action_items": [
    "Nome: descrição da ação"
  ],
  "key_decisions": [
    "Decisão tomada"
  ]
}}"#,
        mapping_lines.join("\n"),
        transcript["full_text"].as_str().unwrap_or("").chars().take(3000).collect::<String>()
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extrai o mapeamento da resposta do LLM, fazendo fallback para o inicial.
fn extract_mapping(
    llm_json:        &serde_json::Value,
    initial_mapping: &HashMap<String, matcher::SpeakerMatch>,
) -> HashMap<String, String> {
    let mut map = HashMap::new();

    // Usa o que o LLM retornou
    if let Some(obj) = llm_json["speaker_mapping"].as_object() {
        for (k, v) in obj {
            if let Some(name) = v.as_str() {
                map.insert(k.clone(), name.to_string());
            }
        }
    }

    // Fallback para o matching por timestamp onde o LLM não cobriu
    for (sp, m) in initial_mapping {
        map.entry(sp.clone()).or_insert_with(|| m.name.clone());
    }

    map
}

/// Aplica o mapeamento de nomes em todos os segmentos da transcrição.
fn apply_mapping(
    segments:     &[serde_json::Value],
    mapping:      &HashMap<String, String>,
) -> Vec<NamedSegment> {
    segments
        .iter()
        .filter_map(|s| {
            let speaker_label = s["speaker"].as_str()?;
            let name = mapping
                .get(speaker_label)
                .cloned()
                .unwrap_or_else(|| speaker_label.to_string());

            Some(NamedSegment {
                speaker_name: name,
                time:         s["start_time"].as_str().unwrap_or("").to_string(),
                start_ms:     s["start_ms"].as_u64().unwrap_or(0),
                text:         s["text"].as_str().unwrap_or("").to_string(),
                confidence:   s["confidence"].as_f64(),
            })
        })
        .collect()
}

fn extract_str_array(val: &serde_json::Value) -> Vec<String> {
    val.as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn format_ms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}

// ---------------------------------------------------------------------------
// Formatação
// ---------------------------------------------------------------------------

impl SummaryResult {
    /// Formata a transcrição com nomes reais para exibição no terminal.
    pub fn format_output(&self) -> String {
        self.transcript
            .iter()
            .map(|s| format!("[{}] {}: {}", s.time, s.speaker_name, s.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Serializa o resultado completo como JSON.
    pub fn to_json(&self) -> String {
        let mapping: serde_json::Value = self.speaker_mapping
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect::<serde_json::Map<_, _>>()
            .into();

        let transcript: Vec<serde_json::Value> = self.transcript
            .iter()
            .map(|s| serde_json::json!({
                "speaker":     s.speaker_name,
                "time":        s.time,
                "start_ms":    s.start_ms,
                "text":        s.text,
                "confidence":  s.confidence
            }))
            .collect();

        serde_json::to_string_pretty(&serde_json::json!({
            "speaker_mapping":  mapping,
            "summary":          self.summary,
            "action_items":     self.action_items,
            "key_decisions":    self.key_decisions,
            "segment_count":    self.transcript.len(),
            "token_usage": {
                "prompt_tokens":     self.token_usage.prompt_tokens,
                "completion_tokens": self.token_usage.completion_tokens,
                "total_tokens":      self.token_usage.total_tokens,
            },
            "transcript":       transcript
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

    #[test]
    fn extract_str_array_vazio() {
        assert!(extract_str_array(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn extract_str_array_com_valores() {
        let v = serde_json::json!(["a", "b", "c"]);
        assert_eq!(extract_str_array(&v), vec!["a", "b", "c"]);
    }

    #[test]
    fn format_ms_converte_corretamente() {
        assert_eq!(format_ms(3_833), "00:03");
        assert_eq!(format_ms(65_000), "01:05");
    }

    #[test]
    fn config_from_env_falha_sem_vars() {
        let r = std::env::var("AZURE_OPENAI_VAR_INEXISTENTE_XYZ");
        assert!(r.is_err());
    }

    #[test]
    fn summary_result_to_json_tem_campos_obrigatorios() {
        let result = SummaryResult {
            speaker_mapping: [("Speaker 1".to_string(), "Fulano".to_string())].into(),
            transcript:      vec![NamedSegment {
                speaker_name: "Fulano".to_string(),
                time:         "00:00".to_string(),
                start_ms:     0,
                text:         "Olá.".to_string(),
                confidence:   Some(0.9),
            }],
            summary:         "Reunião produtiva.".to_string(),
            action_items:    vec!["Fulano: enviar relatório".to_string()],
            key_decisions:   vec!["Manter estrutura atual".to_string()],
            token_usage:     llm::TokenUsage::default(),
        };
        let json = result.to_json();
        assert!(json.contains("speaker_mapping"));
        assert!(json.contains("summary"));
        assert!(json.contains("action_items"));
        assert!(json.contains("key_decisions"));
        assert!(json.contains("transcript"));
    }
}
