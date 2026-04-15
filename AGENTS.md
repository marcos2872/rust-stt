# AGENTS.md

> Arquivo gerado por `/init` com análise automática. Edite manualmente para ajustar convenções.

## Projeto

- **Nome:** rust-stt
- **Descrição:** Pipeline de Speech-to-Text em Rust que converte MP4 → WAV, aplica uma cadeia de 9 filtros de áudio via ffmpeg e transcreve com diarização de falantes usando o Azure OpenAI (`gpt-4o-transcribe-diarize`).

## Stack

- **Linguagem(s):** Rust (edition 2024)
- **Frameworks:** —
- **Dependências principais:** `reqwest 0.12` (HTTP multipart/JSON), `serde 1` + `serde_json 1` (serialização), `dotenvy 0.15` (variáveis de ambiente)
- **Dependências de sistema:** `ffmpeg` e `ffprobe` (devem estar no PATH)

## Gerenciamento de Dependências

- **Instalar tudo:** `cargo build`
- **Adicionar pacote:** `cargo add <pacote>`
- **Remover pacote:** `cargo remove <pacote>`

## Comandos Essenciais

- **Testes:** `cargo test`
- **Build:** `cargo build --release`
- **Rodar pipeline (MP4 → WAV processado):** `cargo run --bin pipeline -- <arquivo.mp4>`
- **Rodar transcrição (WAV → texto):** `cargo run --bin transcribe -- <arquivo.wav>`

## Estrutura de Diretórios

- **Código principal:** `src/`
- **Testes:** embutidos nos módulos via `#[cfg(test)]` (não há diretório `tests/` separado)
- **Dados de exemplo:** `data/`
- **Arquivos temporários de saída:** `temp/`
- **Documentação:** `docs/`

## Módulos

- **`src/main.rs`** — Binário `pipeline`: orquestra o fluxo MP4 → WAV intermediário → WAV processado em 2 etapas
- **`src/bin/transcribe.rs`** — Binário `transcribe`: recebe um WAV, chama a API Azure OpenAI e exibe transcrição com métricas de tokens e custo
- **`src/lib.rs`** — Raiz da crate de biblioteca; reexporta os três módulos públicos
- **`src/audio_processor/`** — Processamento de áudio em 2 passes ffmpeg: 9 filtros configuráveis (bandpass, denoising FFT/NLM, EQ, compressão, noise gate, normalização, limiter, VAD)
- **`src/audio_processor/filters.rs`** — Construtores de strings de filtros ffmpeg individuais (bandpass, adeclick, afftdn, anlmdn, voice_eq, compression, noise_gate, loudness_normalization, limiter, silence_removal)
- **`src/converter/`** — Conversão MP4 → WAV mono 16 kHz 16-bit via ffmpeg; valida entrada e gerencia diretório de saída
- **`src/transcriber/`** — Orquestração da transcrição: divide áudio em chunks de 700 s quando necessário, acumula tokens e faz parse de segmentos por falante
- **`src/transcriber/azure.rs`** — Cliente HTTP blocking para a API de transcrição do Azure OpenAI (multipart/form-data)

## Arquitetura

- **Estilo:** Pipeline modular — dois binários CLI independentes que compartilham uma biblioteca interna
- **Descrição:** `pipeline` usa `converter` + `audio_processor` para preparar o áudio; `transcribe` usa `transcriber` (que delega ao cliente `azure`) para enviar o WAV à API e serializar o resultado em JSON. Os módulos se comunicam apenas por tipos Rust explícitos (`Result`, structs de config) sem estado global.

## Variáveis de Ambiente

> Copie `.env.example` para `.env` e ajuste os valores.

- **Obrigatórias:** `AZURE_OPENAI_API_KEY`, `AZURE_OPENAI_ENDPOINT`, `AZURE_OPENAI_DEPLOYMENT`
- **Opcionais:** `AZURE_OPENAI_API_VERSION` (padrão: `2025-04-01-preview`), `AZURE_OPENAI_LANGUAGE` (padrão: detecção automática, ex.: `pt`)

## Testes

- **Framework:** Rust built-in (`#[cfg(test)]` + `#[test]`)
- **Diretório:** embutidos em cada módulo (`src/audio_processor/mod.rs`, `src/converter/mod.rs`, `src/transcriber/mod.rs`, `src/bin/transcribe.rs`)
- **Executar todos:** `cargo test`
- **Testes de integração:** dependem de `ffmpeg`/`ffprobe` disponíveis no PATH; criam arquivos temporários em `$TMPDIR` e os removem ao final

## Convenções de Código

- **Tamanho máximo de função:** 40 linhas
- **Tamanho máximo de arquivo:** 300 linhas
- **Aninhamento máximo:** 3 níveis
- **Docstrings / comentários:** Português brasileiro
- **Identificadores (variáveis, funções, structs, enums):** Inglês
- **Erros:** tipos `enum` próprios por módulo implementando `std::fmt::Display` e `std::error::Error`; sem uso de `unwrap()` em código de produção
- **Resultados:** sempre `Result<T, ModuleError>` — nunca panic em caminhos normais
- **Rust idiomático:** `|` para padrões, `?` para propagação de erros, sem `clone()` desnecessário

## Commits

Este projeto segue o padrão **Conventional Commits**.
Antes de commitar, carregue a skill de commit:

```
/skill:git-commit-push
```

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
