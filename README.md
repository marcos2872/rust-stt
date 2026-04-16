# rust-stt

Pipeline em Rust para transcrever reuniões em MP4, identificar falantes e gerar atas estruturadas usando Azure AI Speech e Azure OpenAI.

## O que faz

```
MP4 + VTT
  │
  ├─ [pipeline]   MP4 → WAV mono 16 kHz processado (9 filtros ffmpeg)
  ├─ [transcribe] WAV → transcript.json  (Azure AI Speech, diarização)
  ├─ [summarize]  transcript.json + VTT → summary.json  (Azure OpenAI)
  ├─ [minutes]    summary.json → minutes.json  (ata FLOW, Azure OpenAI)
  └─ [app]        interface gráfica nativa — orquestra tudo acima
```

## Requisitos

- Rust (edition 2024)
- `ffmpeg` e `ffprobe` no PATH
- Conta Azure AI Speech
- Conta Azure OpenAI com deployment configurado (ex.: `gpt-5.4-mini`)

## Configuração

```sh
cp .env.example .env   # preencha as credenciais
```

| Variável | Obrigatória | Descrição |
|---|---|---|
| `AZURE_OPENAI_API_KEY` | sim | Chave do recurso Azure OpenAI |
| `AZURE_OPENAI_ENDPOINT` | sim | URL base do recurso |
| `AZURE_OPENAI_DEPLOYMENT` | sim | Nome do deployment do modelo |
| `AZURE_OPENAI_API_VERSION` | não | Padrão: `2025-01-01-preview` |
| `AZURE_SPEECH_KEY` | não* | Chave Azure AI Speech (fallback: `AZURE_OPENAI_API_KEY`) |
| `AZURE_SPEECH_ENDPOINT` | não* | Endpoint Speech (fallback: `AZURE_OPENAI_ENDPOINT`) |
| `AZURE_SPEECH_LANGUAGE` | não | Padrão: `pt-BR` |
| `AZURE_SPEECH_MAX_SPEAKERS` | não | Padrão: `10` |

## Uso via CLI

### 1 — Preparar o áudio

```sh
cargo run --bin pipeline -- reuniao.mp4
# saída: temp/reuniao.wav
```

### 2 — Transcrever

```sh
cargo run --bin transcribe -- temp/reuniao.wav
# saída: temp/reuniao_transcript.json
```

### 3 — Identificar falantes e resumir

Exporte o `.vtt` da gravação do Teams antes deste passo.

```sh
cargo run --bin summarize -- temp/reuniao_transcript.json reuniao.vtt
# saída: temp/reuniao_summary.json
```

### 4 — Gerar ata (formato FLOW)

```sh
cargo run --bin minutes -- temp/reuniao_summary.json
# saída: temp/reuniao_minutes.json
```

## Uso via Interface Gráfica

```sh
cargo run --bin app
```

A janela permite selecionar o MP4 e o VTT, acompanhar o progresso em tempo real, salvar os três artefatos finais e consultar o histórico de uso de tokens e custos.

## Artefatos gerados

| Arquivo | Conteúdo |
|---|---|
| `*_transcript.json` | Transcrição completa com Speaker N, timestamps e confiança |
| `*_summary.json` | Mapeamento de falantes reais, resumo executivo, action items, decisões, uso de tokens |
| `*_minutes.json` | Ata estruturada FLOW (seções 01–08), uso de tokens |

## Estimativa de custo

| Etapa | Serviço | Preço |
|---|---|---|
| Transcrição | Azure AI Speech | $1.00 / hora de áudio |
| Resumo | Azure OpenAI gpt-5.4-mini | $0.750 / 1M tokens entrada · $4.500 / 1M tokens saída |
| Ata | Azure OpenAI gpt-5.4-mini | $0.750 / 1M tokens entrada · $4.500 / 1M tokens saída |

Os valores são exibidos no terminal após cada execução e persistidos em `~/.config/rust-stt/cost_history.json` quando usando o app gráfico.

## Documentação

| Documento | Conteúdo |
|---|---|
| [Conversão e Processamento de Áudio](docs/audio-processing.md) | Filtros ffmpeg, parâmetros WAV |
| [Transcrição](docs/transcription.md) | Azure AI Speech, diarização, chunking, JSON de saída |
| [Summarizer](docs/summarizer.md) | Matching VTT, LLM, mapeamento de falantes, JSON de saída |
| [Minutes](docs/minutes.md) | Template FLOW, schema JSON da ata, prompt |
| [Interface Gráfica](docs/ui.md) | Painéis, configuração, histórico de custos |

## Testes

```sh
cargo test
```

## Build release

```sh
cargo build --release
# binários em target/release/: pipeline, transcribe, summarize, minutes, app
```
