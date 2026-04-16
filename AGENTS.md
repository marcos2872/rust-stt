# AGENTS.md

> Arquivo gerado por `/init` com análise automática. Edite manualmente para ajustar convenções.

## Projeto

- **Nome:** rust-stt
- **Descrição:** Pipeline em Rust para preparar e transcrever áudio de vídeos MP4 usando Azure. Converte MP4 → WAV, aplica 9 etapas de filtragem de áudio via ffmpeg, transcreve com diarização de falantes via Azure AI Speech (`Fast Transcription API`) e gera resumo executivo identificando falantes reais via Azure OpenAI + VTT do Teams.

## Stack

- **Linguagem(s):** Rust (edition 2024)
- **Frameworks:** —
- **Dependências principais:** `reqwest 0.12` (HTTP multipart/JSON, blocking), `serde 1` + `serde_json 1` (serialização), `dotenvy 0.15` (variáveis de ambiente)
- **Dependências de sistema:** `ffmpeg` e `ffprobe` (devem estar no PATH)

## Gerenciamento de Dependências

- **Instalar tudo:** `cargo build`
- **Adicionar pacote:** `cargo add <pacote>`
- **Remover pacote:** `cargo remove <pacote>`

## Comandos Essenciais

- **Testes:** `cargo test`
- **Build:** `cargo build --release`
- **Pipeline (MP4 → WAV processado):** `cargo run --bin pipeline -- <arquivo.mp4>`
- **Transcrição (WAV → JSON):** `cargo run --bin transcribe -- <arquivo.wav>`
- **Summarizer (transcript.json + VTT → resumo):** `cargo run --bin summarize -- <transcript.json> <meeting.vtt>`

## Estrutura de Diretórios

- **Código principal:** `src/`
- **Testes:** embutidos nos módulos via `#[cfg(test)]` (não há diretório `tests/` separado)
- **Dados de exemplo:** `data/`
- **Arquivos de saída temporários:** `temp/`
- **Documentação:** `docs/`

## Módulos

- **`src/main.rs`** — Binário `pipeline`: orquestra o fluxo MP4 → WAV intermediário (tmpdir do SO) → WAV processado em 2 etapas, removendo o intermediário ao final
- **`src/bin/transcribe.rs`** — Binário `transcribe`: recebe um WAV, chama o módulo `transcriber` e serializa transcrição com diarização em `<stem>_transcript.json`
- **`src/bin/summarize.rs`** — Binário `summarize`: recebe `transcript.json` + `.vtt` do Teams, identifica falantes reais e gera resumo executivo em `<stem>_summary.json`
- **`src/lib.rs`** — Raiz da crate de biblioteca; reexporta os quatro módulos públicos (`audio_processor`, `converter`, `transcriber`, `summarizer`)
- **`src/audio_processor/`** — Processamento de áudio em 2 passes ffmpeg com 9 filtros configuráveis: bandpass HPF/LPF, adeclick, afftdn, anlmdn, voice EQ, compressão, noise gate, dynaudnorm, alimiter, VAD
- **`src/audio_processor/filters.rs`** — Construtores de strings de filtros ffmpeg individuais (cada filtro em função separada)
- **`src/converter/`** — Conversão MP4 → WAV mono 16 kHz 16-bit PCM via ffmpeg; valida entrada, gerencia diretório de saída e define `ConverterError`
- **`src/transcriber/`** — Orquestra a transcrição: arquivos até ~180 MB em requisição única; acima disso divide em chunks de 3 000 s com ajuste de timestamps globais
- **`src/transcriber/azure_speech.rs`** — Cliente HTTP blocking para a Azure AI Speech Fast Transcription API (multipart/form-data, `api-version=2024-11-15`)
- **`src/summarizer/`** — Identificação de falantes reais e geração de resumo executivo; define `SummarizerConfig`, `SummaryResult` e `SummarizerError`
- **`src/summarizer/vtt.rs`** — Parser do formato WebVTT do Teams, extrai entradas `<v Name>`
- **`src/summarizer/matcher.rs`** — Matching por timestamp (tolerância configurável) entre `Speaker N` da transcrição e nomes reais do VTT
- **`src/summarizer/llm.rs`** — Cliente HTTP blocking para Azure OpenAI chat completions; confirma mapeamento e gera resumo/action items/decisões

## Arquitetura

- **Estilo:** Pipeline modular — três binários CLI independentes que compartilham uma biblioteca interna (`rust_stt`)
- **Descrição:** `pipeline` usa `converter` + `audio_processor` para preparar o áudio; `transcribe` usa `transcriber` (que delega ao cliente `azure_speech`) para enviar o WAV à API e gravar JSON; `summarize` usa `summarizer` (que usa `vtt`, `matcher` e `llm`) para identificar falantes via VTT do Teams e gerar resumo via Azure OpenAI. Os módulos se comunicam apenas por tipos Rust explícitos (`Result<T, ModuleError>`, structs de configuração) sem estado global.

## Variáveis de Ambiente

> Copie `.env.example` para `.env` e ajuste os valores.

- **Azure OpenAI (obrigatórias para todos os módulos):** `AZURE_OPENAI_API_KEY`, `AZURE_OPENAI_ENDPOINT`
- **Summarizer (obrigatória):** `AZURE_OPENAI_DEPLOYMENT` (ex.: `gpt-5.4-mini`)
- **Summarizer (opcional):** `AZURE_OPENAI_API_VERSION` (padrão: `2025-01-01-preview`)
- **Transcrição — Speech (opcionais, fallback para `AZURE_OPENAI_*`):** `AZURE_SPEECH_KEY`, `AZURE_SPEECH_ENDPOINT`, `AZURE_SPEECH_LANGUAGE` (padrão: `pt-BR`), `AZURE_SPEECH_MAX_SPEAKERS` (padrão: `10`)

## Testes

- **Framework:** Rust built-in (`#[cfg(test)]` + `#[test]`)
- **Diretório:** embutidos em cada módulo (`src/converter/mod.rs`, `src/transcriber/mod.rs`, `src/summarizer/mod.rs`, `src/bin/summarize.rs`)
- **Executar todos:** `cargo test`
- **Testes de integração:** dependem de `ffmpeg`/`ffprobe` disponíveis no PATH; criam arquivos temporários em `$TMPDIR` e os removem ao final

## Convenções de Código

- **Tamanho máximo de função:** 40 linhas
- **Tamanho máximo de arquivo:** 300 linhas
- **Aninhamento máximo:** 3 níveis
- **Docstrings / comentários:** Português brasileiro
- **Identificadores (variáveis, funções, structs, enums):** Inglês
- **Erros:** `enum` próprio por módulo (`ConverterError`, `TranscriberError`, `SummarizerError`) implementando `std::fmt::Display` e `std::error::Error`; sem `unwrap()` em código de produção
- **Resultados:** sempre `Result<T, ModuleError>` — nunca panic em caminhos normais
- **Rust idiomático:** `?` para propagação de erros, sem `clone()` desnecessário, `map_err` para conversões de erro entre camadas
- **Testes:** cada módulo inclui testes unitários (lógica pura) e de integração (dependem de ffmpeg) separados por comentários `// ---`

## Commits

Este projeto segue o padrão **Conventional Commits**.
Antes de commitar, carregue a skill de commit:

```
/skill:git-commit-push
```

Ou siga diretamente as regras em `.agents/skills/git-commit-push/SKILL.md`.

## Agentes e Skills

| Agente    | Função                                         | Modo                   |
|-----------|------------------------------------------------|------------------------|
| `build`   | Implementa funcionalidades e corrige bugs      | escrita completa       |
| `ask`     | Responde perguntas somente-leitura             | somente-leitura        |
| `plan`    | Cria planos detalhados em `.pi/plans/`         | escrita em .pi/plans/  |
| `quality` | Auditoria de qualidade de código               | bash + leitura         |
| `qa`      | Análise de bugs e edge cases                   | bash + leitura         |
| `test`    | Cria e mantém testes automatizados             | escrita em tests/      |
| `doc`     | Cria documentação técnica em `docs/`           | escrita em docs/       |
