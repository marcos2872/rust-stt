# Plano: Gerador de Ata Estruturada (minutes)

**Data:** 2026-04-16
**Autor:** agente-plan
**Status:** rascunho

---

## Objetivo

Criar o módulo `minutes` e o binário `minutes` que recebem um `summary.json` (saída do
`summarize`) e geram uma ata estruturada no formato FLOW — Registro de Reunião de Alinhamento,
salvando o resultado como `<stem>_minutes.json`.

O LLM (Azure OpenAI, mesmo deployment do `summarize`) recebe o conteúdo do summary e produz
um JSON com todos os campos do template FLOW (seções 01–08), que é então validado e persistido.

---

## Escopo

**Dentro do escopo:**
- Novo módulo `src/minutes/` com submódulos `llm.rs` e `prompt.rs`
- Novo binário `src/bin/minutes.rs`
- Atualização de `src/lib.rs` (expor `pub mod minutes`)
- Atualização de `Cargo.toml` (novo `[[bin]]`)
- Testes unitários embutidos nos módulos (sem dependência de rede)

**Fora do escopo:**
- Geração de arquivo `.md` ou `.docx` a partir do JSON
- Suporte a múltiplos templates de ata
- Passagem do arquivo de template como argumento CLI
- Alteração de qualquer módulo existente (`summarizer`, `transcriber`, etc.)

---

## Schema JSON de saída (`*_minutes.json`)

O JSON completo que o LLM deve produzir — e que `MinutesResult` envelopa:

```json
{
  "meeting_data": {
    "title":      "string",
    "date":       "dd/mm/aaaa",
    "time_start": "HH:MM",
    "time_end":   "HH:MM",
    "duration":   "string",
    "modality":   "Remota | Presencial | Híbrida",
    "facilitator":"string",
    "relator":    "string",
    "platform":   "string"
  },
  "participants": [
    { "name": "string", "organization": "string", "role": "string" }
  ],
  "context": {
    "objective":       "string",
    "planned_agenda":  ["string"],
    "agenda_changes":  "string"
  },
  "topics": [
    {
      "title":                  "string",
      "context":                "string",
      "key_points":             "string",
      "questions_and_answers":  "string",
      "attention_point":        "string"
    }
  ],
  "decisions": [
    { "id": "D1", "decision": "string", "justification": "string", "impact": "string" }
  ],
  "open_points": [
    {
      "id":            "P1",
      "description":   "string",
      "what_is_missing":"string",
      "responsible":   "string",
      "deadline":      "string",
      "status":        "🔴 Aberto"
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
      "alignment_level":   "Muito alinhado | Alinhado | Parcialmente alinhado | Com divergências | Com divergências críticas",
      "tone_observations": "string"
    }
  },
  "action_plan": {
    "todos": [
      { "what": "string", "who": "string", "when": "string", "depends_on": "string", "observation": "string" }
    ],
    "next_meeting": {
      "expected_date":    "string",
      "objective":        "string",
      "suggested_agenda": "string",
      "who_schedules":    "string"
    }
  }
}
```

---

## Arquivos Afetados

| Arquivo | Ação | Motivo |
|---|---|---|
| `src/minutes/mod.rs` | criar | módulo principal: config, types, erro, `generate_minutes()` |
| `src/minutes/llm.rs` | criar | cliente HTTP Azure OpenAI adaptado para MinutesError |
| `src/minutes/prompt.rs` | criar | construtores de `system_prompt` e `user_prompt` |
| `src/bin/minutes.rs` | criar | binário CLI: lê summary.json, salva minutes.json |
| `src/lib.rs` | modificar | adicionar `pub mod minutes;` |
| `Cargo.toml` | modificar | adicionar `[[bin]] name = "minutes"` |

---

## Sequência de Execução

### 1. Atualizar `Cargo.toml`
**Arquivo:** `Cargo.toml`
**O que fazer:** Adicionar entrada `[[bin]]` para o novo binário.
```toml
[[bin]]
name = "minutes"
path = "src/bin/minutes.rs"
```
**Dependências:** nenhuma — nenhuma dependência nova de crate é necessária
(reutiliza `reqwest`, `serde`, `serde_json`, `dotenvy`).

---

### 2. Criar `src/minutes/llm.rs`
**Arquivo:** `src/minutes/llm.rs`
**O que fazer:** Clonar o padrão de `src/summarizer/llm.rs` adaptando os tipos:
- Substituir `SummarizerConfig` → `MinutesConfig`
- Substituir `SummarizerError` → `MinutesError`
- `max_completion_tokens: 8_192` (ata tem mais campos que o summary)
- Timeout: `300s` — transcrição completa aumenta o tempo de processamento
- Tudo mais idêntico ao summarizer/llm.rs

**Dependências:** nenhuma (MinutesConfig e MinutesError definidos no passo 3).

---

### 3. Criar `src/minutes/prompt.rs`
**Arquivo:** `src/minutes/prompt.rs`
**O que fazer:** Dois construtores de string:

**`build_system_prompt() -> String`**
- Define o papel do LLM: especialista em atas corporativas
- Inclui as 7 regras de processamento do template FLOW (não repetir informações entre seções,
  síntese objetiva em tópicos, não inventar decisões/prazos, etc.)
- Declara que a resposta deve ser **apenas JSON válido** com o schema completo da ata

**`build_user_prompt(summary: &serde_json::Value) -> String`**
- Extrai do summary: `summary` (texto), `action_items`, `key_decisions`, `speaker_mapping`
- Serializa o **summary JSON completo** (com toda a `transcript`) e o passa diretamente
  como string JSON — nenhuma informação é descartada
- Transcrição completa garante extração consistente de citações-chave, tópicos, clima
  e análise pós-reunião
- Inclui o JSON schema completo como "formato de saída esperado"
- Instrução explícita: campos sem informação → `"Não mencionado na reunião"` (string)
  ou `[]` (array)
- Fallback de segurança: se o JSON serializado ultrapassar ~120 KB, trunca os segmentos
  intermediários da `transcript` (preserva início e fim) e registra aviso em `stderr`

**Dependências:** nenhuma além de `serde_json`.

---

### 4. Criar `src/minutes/mod.rs`
**Arquivo:** `src/minutes/mod.rs`
**O que fazer:** Definir:

**`MinutesConfig`** — campos idênticos a `SummarizerConfig`:
```rust
pub struct MinutesConfig {
    pub api_key:    String,
    pub endpoint:   String,
    pub deployment: String,
    pub api_version: String,
}
```
`from_env()` usa as mesmas vars de ambiente (`AZURE_OPENAI_*`).

**`MinutesResult`** — envelopa o JSON gerado:
```rust
pub struct MinutesResult {
    pub minutes: serde_json::Value,
}
```
`to_json(&self) -> String` → `serde_json::to_string_pretty(&self.minutes)`.

**`MinutesError`** — enum com variantes `Config`, `Io`, `Http`, `Parse`
(mesmo padrão de `SummarizerError`, implementa `Display` + `Error`).

**`generate_minutes(summary_path, config) -> Result<MinutesResult, MinutesError>`**:
1. Lê e parseia `summary_path` como `serde_json::Value`
2. Chama `prompt::build_system_prompt()` e `prompt::build_user_prompt(&summary_json)`
3. Chama `llm::complete(system, user, config)`
4. Parseia a string retornada como `serde_json::Value`
5. Retorna `MinutesResult { minutes }`

**Testes unitários:**
- `minutes_error_display` — verifica mensagens de erro
- `minutes_result_to_json_e_valido` — instancia um `MinutesResult` com JSON mínimo e verifica
  que `to_json()` contém `"meeting_data"`, `"participants"`, `"action_plan"`

**Dependências:** passos 2 e 3 devem existir.

---

### 5. Atualizar `src/lib.rs`
**Arquivo:** `src/lib.rs`
**O que fazer:** Adicionar `pub mod minutes;` ao arquivo.

**Dependências:** passo 4.

---

### 6. Criar `src/bin/minutes.rs`
**Arquivo:** `src/bin/minutes.rs`
**O que fazer:**

**`main()`** — parse de args, validação, chama `run()`, trata erros com `process::exit(1)`.

**Uso:**
```sh
cargo run --bin minutes -- <summary.json>
```

**`run(summary_path: &Path) -> Result<(), String>`**:
1. Valida que `summary_path` existe
2. Carrega `MinutesConfig::from_env()`
3. Imprime cabeçalho (arquivo, deployment, api_version)
4. Chama `rust_stt::minutes::generate_minutes(summary_path, &config)`
5. Imprime seções principais no terminal (meeting_data.title, participantes,
   quantidade de tópicos, decisões, action_plan.todos)
6. Salva JSON em `<stem>_minutes.json` via `build_output_path()`

**`build_output_path(p: &Path) -> PathBuf`**:
- `temp/call_summary.json` → `temp/call_minutes.json`
- Remove sufixo `_summary` do stem antes de compor o nome

**Testes unitários:**
- `build_output_path_remove_sufixo_summary`
- `build_output_path_sem_sufixo`

**Dependências:** passos 4 e 5.

---

## Detalhes do Prompt

### System prompt (estrutura)

```
Você é um especialista em elaboração de atas corporativas.
Receberá um resumo estruturado de reunião (summary JSON) e deverá preenchê-lo
no formato FLOW — Registro de Reunião de Alinhamento.

Regras:
1. Preserve a estrutura do JSON schema fornecido.
2. Não repita a mesma informação em seções diferentes.
3. Em "topics", sintetize objetivamente; não transcreva a conversa.
4. Em "decisions", registre apenas o que foi acordado.
5. Em "open_points", apenas itens ainda não resolvidos.
6. Se algo não foi mencionado, use "Não mencionado na reunião" (string) ou [] (array).
7. Não invente decisões, prazos, responsáveis ou materiais.

Responda APENAS com JSON válido no schema fornecido no prompt do usuário.
```

### User prompt (estrutura)

```
# Resumo da reunião
{summary_text}

# Participantes identificados
{speaker_mapping}

# Pontos de ação
{action_items}

# Decisões tomadas
{key_decisions}

# Summary JSON completo (inclui transcrição inteira)
{summary_json_completo}

# Schema de saída esperado
{json_schema_completo}

Preencha todos os campos. Para campos sem informação use
"Não mencionado na reunião" ou [].
```

---

## Riscos e Mitigações

| Risco | Probabilidade | Mitigação |
|---|---|---|
| LLM retornar JSON inválido ou truncado | Média | `serde_json::from_str` com `map_err` → `MinutesError::Parse`; mensagem inclui trecho do retorno |
| JSON de entrada exceder limite do modelo | Baixa | `build_user_prompt` verifica tamanho em bytes; se > ~120 KB, trunca segmentos intermediários da `transcript` preservando início e fim, e emite aviso em `stderr` |
| Campos obrigatórios ausentes no JSON retornado | Média | Validação pós-parse: verificar presença de `meeting_data` e `action_plan`; erro descritivo se ausentes |
| Tempo de resposta elevado | Baixa | Timeout configurado em 300s no cliente reqwest; mensagem de erro clara |
| `src/summarizer/mod.rs` já tem 469 linhas (viola regra 300) | Alta (já existe) | Fora do escopo deste plano; não tocar no summarizer |

---

## Critérios de Conclusão

- [ ] `cargo build --release` sem erros ou warnings
- [ ] `cargo test` passa (todos os testes unitários dos novos módulos)
- [ ] Executar `cargo run --bin minutes -- temp/call_summary.json` gera `temp/call_minutes.json`
- [ ] O JSON gerado contém todas as chaves de primeiro nível:
  `meeting_data`, `participants`, `context`, `topics`, `decisions`,
  `open_points`, `materials`, `post_meeting_analysis`, `action_plan`
- [ ] Cada arquivo novo respeita o limite de 300 linhas
- [ ] Cada função respeita o limite de 40 linhas
- [ ] Sem `unwrap()` em código de produção
