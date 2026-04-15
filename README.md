# rust-stt

Pipeline em Rust para preparar e transcrever áudio de vídeos MP4 usando Azure OpenAI.

## O que faz

1. **Converte** MP4 → WAV (mono, 16 kHz, 16-bit PCM)
2. **Processa** o áudio com 9 etapas de filtragem (noise reduction, EQ, compressão, VAD...)
3. **Transcreve** o WAV via Azure OpenAI (`gpt-4o-transcribe-diarize`) com estimativa de custo

## Requisitos

- Rust (edition 2024)
- `ffmpeg` e `ffprobe` instalados no PATH
- Conta Azure OpenAI com deployment `gpt-4o-transcribe-diarize`

## Configuração

Copie o arquivo de exemplo e preencha com suas credenciais:

```sh
cp .env.example .env
```

## Uso

### Pipeline completo (MP4 → WAV processado)

```sh
cargo run --bin pipeline -- data/video.mp4
# resultado: temp/video.wav
```

### Transcrição

```sh
cargo run --bin transcribe -- temp/video.wav
# resultado: temp/video_transcript.json
```

## Documentação

- [Conversão e Processamento de Áudio](docs/audio-processing.md)
- [Transcrição via Azure OpenAI](docs/transcription.md)

## Testes

```sh
cargo test
```
