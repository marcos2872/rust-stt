//! Construtores de prompts para o gerador de atas.

// Limite em bytes para o JSON de entrada antes de truncar a transcript.
const MAX_INPUT_BYTES: usize = 120_000;

// ---------------------------------------------------------------------------
// Schema de saída esperado (FLOW — Registro de Reunião de Alinhamento)
// ---------------------------------------------------------------------------

const OUTPUT_SCHEMA: &str = r#"{
  "meeting_data": {
    "title": "string",
    "date": "dd/mm/aaaa",
    "time_start": "HH:MM",
    "time_end": "HH:MM",
    "duration": "string",
    "modality": "Remota | Presencial | Híbrida",
    "facilitator": "string",
    "relator": "string",
    "platform": "string"
  },
  "participants": [
    { "name": "string", "organization": "string", "role": "string" }
  ],
  "context": {
    "objective": "string",
    "planned_agenda": ["string"],
    "agenda_changes": "string"
  },
  "topics": [
    {
      "title": "string",
      "context": "string",
      "key_points": "string",
      "questions_and_answers": "string",
      "attention_point": "string"
    }
  ],
  "decisions": [
    { "id": "D1", "decision": "string", "justification": "string", "impact": "string" }
  ],
  "open_points": [
    {
      "id": "P1",
      "description": "string",
      "what_is_missing": "string",
      "responsible": "string",
      "deadline": "string",
      "status": "🔴 Aberto"
    }
  ],
  "materials": {
    "received": [
      { "material": "string", "format": "string", "provided_by": "string", "location": "string" }
    ],
    "to_send": [
      { "material": "string", "responsible": "string", "recipient": "string", "deadline": "string" }
    ]
  },
  "post_meeting_analysis": {
    "key_quotes": ["string"],
    "hypotheses_alignments": [
      { "hypothesis": "string", "status": "✅ Confirmado | ❌ Refutado | ❓ Inconclusivo | 🆕 Novo", "evidence": "string" }
    ],
    "product_impacts": [
      { "area": "string", "observation": "string", "priority": "Alta | Média | Baixa", "recommended_action": "string" }
    ],
    "risks": [
      { "risk": "string", "probability": "Alta | Média | Baixa", "impact": "Alto | Médio | Baixo", "mitigation": "string" }
    ],
    "climate": {
      "alignment_level": "Muito alinhado | Alinhado | Parcialmente alinhado | Com divergências | Com divergências críticas",
      "tone_observations": "string"
    }
  },
  "action_plan": {
    "todos": [
      { "what": "string", "who": "string", "when": "string", "depends_on": "string", "observation": "string" }
    ],
    "next_meeting": {
      "expected_date": "string",
      "objective": "string",
      "suggested_agenda": "string",
      "who_schedules": "string"
    }
  }
}"#;

// ---------------------------------------------------------------------------
// Funções públicas
// ---------------------------------------------------------------------------

/// Retorna o system prompt para geração de atas no formato FLOW.
pub fn build_system_prompt() -> &'static str {
    r#"Você é um especialista em elaboração de atas corporativas.
Receberá um summary JSON de reunião (com transcrição completa) e deverá
preencher o formato FLOW — Registro de Reunião de Alinhamento.

Regras obrigatórias:
1. Preserve exatamente a estrutura do JSON schema fornecido.
2. Não repita a mesma informação em seções diferentes.
3. Em "topics", faça síntese objetiva de cada tópico; não transcreva a conversa.
4. Em "decisions", registre apenas o que foi de fato acordado entre as partes.
5. Em "open_points", apenas itens ainda sem resolução ao final da reunião.
6. Se algo não foi mencionado, use "Não mencionado na reunião" (string) ou [] (array).
7. Não invente decisões, prazos, responsáveis ou materiais que não apareçam na fonte.

Responda APENAS com JSON válido no schema fornecido no prompt do usuário."#
}

/// Monta o user prompt com todo o conteúdo do summary JSON.
/// Se o JSON serializado ultrapassar MAX_INPUT_BYTES, trunca a transcript.
pub fn build_user_prompt(summary: &serde_json::Value) -> String {
    let summary_text  = summary["summary"].as_str().unwrap_or("");
    let mapping_str   = format_mapping(&summary["speaker_mapping"]);
    let actions_str   = format_array(&summary["action_items"]);
    let decisions_str = format_array(&summary["key_decisions"]);
    let summary_json  = truncate_if_needed(summary);

    format!(
        "# Resumo da reunião\n{summary_text}\n\n\
         # Participantes identificados\n{mapping_str}\n\n\
         # Pontos de ação\n{actions_str}\n\n\
         # Decisões tomadas\n{decisions_str}\n\n\
         # Summary JSON completo (inclui transcrição inteira)\n{summary_json}\n\n\
         # Schema de saída esperado\n{OUTPUT_SCHEMA}\n\n\
         Preencha todos os campos. Para campos sem informação use \
         \"Não mencionado na reunião\" ou []."
    )
}

// ---------------------------------------------------------------------------
// Helpers privados
// ---------------------------------------------------------------------------

/// Formata um array JSON de strings como lista com marcadores.
fn format_array(val: &serde_json::Value) -> String {
    val.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| format!("  • {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| "  Não disponível".to_string())
}

/// Formata o objeto speaker_mapping como lista legível.
fn format_mapping(val: &serde_json::Value) -> String {
    val.as_object()
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| format!("  {k} → {}", v.as_str().unwrap_or("?")))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| "  Não disponível".to_string())
}

/// Serializa o summary para JSON; trunca a transcript se > MAX_INPUT_BYTES,
/// preservando os primeiros e últimos 25 segmentos.
fn truncate_if_needed(summary: &serde_json::Value) -> String {
    let full = serde_json::to_string(summary).unwrap_or_default();
    if full.len() <= MAX_INPUT_BYTES {
        return full;
    }

    let segs = match summary["transcript"].as_array() {
        Some(arr) if arr.len() > 50 => arr.clone(),
        _ => return full,
    };

    let n    = segs.len();
    let half = 25_usize;
    let mut trimmed = segs[..half].to_vec();
    trimmed.push(serde_json::json!({
        "_note": format!("... {} segmentos omitidos ...", n - half * 2)
    }));
    trimmed.extend_from_slice(&segs[n - half..]);

    eprintln!(
        "⚠  JSON > {} KB — transcript truncado: {} de {} segmentos mantidos",
        MAX_INPUT_BYTES / 1_000,
        half * 2,
        n
    );

    let mut modified = summary.clone();
    modified["transcript"] = serde_json::Value::Array(trimmed);
    serde_json::to_string(&modified).unwrap_or_default()
}
