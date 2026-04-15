//! Conversor de MP4 → WAV via `ffmpeg`.
//!
//! Parâmetros fixos de saída: mono, 16 kHz, PCM 16-bit (`pcm_s16le`).
//! Formato ideal para pipelines de Speech-to-Text.
//!
//! # Exemplo
//! ```no_run
//! use std::path::Path;
//! use rust_stt::converter::convert_mp4_to_wav;
//!
//! let output = convert_mp4_to_wav(Path::new("video.mp4"), Path::new("temp")).unwrap();
//! println!("Arquivo gerado: {}", output.display());
//! ```

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Tipo de erro
// ---------------------------------------------------------------------------

/// Erros possíveis durante a conversão.
#[derive(Debug)]
pub enum ConverterError {
    /// Arquivo de entrada não encontrado ou inacessível.
    InputNotFound(PathBuf),
    /// O arquivo de entrada não é um `.mp4`.
    InvalidExtension(PathBuf),
    /// Falha ao criar o diretório de saída.
    OutputDirError(String),
    /// O processo `ffmpeg` falhou.
    FfmpegFailed {
        exit_code: Option<i32>,
        stderr: String,
    },
}

impl fmt::Display for ConverterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputNotFound(p) => {
                write!(f, "Arquivo de entrada não encontrado: {}", p.display())
            }
            Self::InvalidExtension(p) => {
                write!(f, "Extensão inválida (esperado .mp4): {}", p.display())
            }
            Self::OutputDirError(msg) => write!(f, "Erro ao criar diretório de saída: {msg}"),
            Self::FfmpegFailed { exit_code, stderr } => {
                write!(f, "ffmpeg falhou (código {:?}):\n{stderr}", exit_code)
            }
        }
    }
}

impl std::error::Error for ConverterError {}

// ---------------------------------------------------------------------------
// Função pública
// ---------------------------------------------------------------------------

/// Converte um arquivo MP4 em WAV sem perdas.
///
/// Parâmetros de saída fixos:
/// - **Codec**: PCM signed 16-bit little-endian (`pcm_s16le`)
/// - **Canais**: mono (`-ac 1`)
/// - **Taxa de amostragem**: 16 kHz (`-ar 16000`)
///
/// # Parâmetros
/// - `input`      — caminho para o arquivo `.mp4`.
/// - `output_dir` — diretório onde o `.wav` será salvo (criado se não existir).
///
/// # Retorno
/// Caminho completo do arquivo `.wav` gerado em caso de sucesso.
///
/// # Erros
/// Retorna [`ConverterError`] se o arquivo de entrada for inválido,
/// o diretório não puder ser criado ou o `ffmpeg` falhar.
pub fn convert_mp4_to_wav(input: &Path, output_dir: &Path) -> Result<PathBuf, ConverterError> {
    validate_input(input)?;
    let output_path = build_output_path(input, output_dir)?;
    run_ffmpeg(input, &output_path)?;
    Ok(output_path)
}

// ---------------------------------------------------------------------------
// Funções auxiliares privadas
// ---------------------------------------------------------------------------

/// Valida existência e extensão do arquivo de entrada.
fn validate_input(input: &Path) -> Result<(), ConverterError> {
    if !input.exists() {
        return Err(ConverterError::InputNotFound(input.to_path_buf()));
    }

    let ext = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if ext != "mp4" {
        return Err(ConverterError::InvalidExtension(input.to_path_buf()));
    }

    Ok(())
}

/// Constrói o caminho de saída, criando o diretório se necessário.
fn build_output_path(input: &Path, output_dir: &Path) -> Result<PathBuf, ConverterError> {
    std::fs::create_dir_all(output_dir)
        .map_err(|e| ConverterError::OutputDirError(e.to_string()))?;

    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    Ok(output_dir.join(format!("{stem}.wav")))
}

/// Invoca o `ffmpeg` para realizar a extração de áudio.
fn run_ffmpeg(input: &Path, output: &Path) -> Result<(), ConverterError> {
    let result = Command::new("ffmpeg")
        .args([
            "-y",                          // sobrescreve sem perguntar
            "-i", &input.to_string_lossy(),
            "-vn",                         // remove vídeo
            "-acodec", "pcm_s16le",        // PCM 16-bit sem perdas
            "-ac", "1",                    // mono
            "-ar", "16000",                // 16 kHz
            &output.to_string_lossy(),
        ])
        .output()
        .map_err(|e| ConverterError::FfmpegFailed {
            exit_code: None,
            stderr: format!("Não foi possível executar ffmpeg: {e}"),
        })?;

    if !result.status.success() {
        return Err(ConverterError::FfmpegFailed {
            exit_code: result.status.code(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ------------------------------------------------------------------
    // Helpers de teste
    // ------------------------------------------------------------------

    /// Cria um MP4 mínimo (1 segundo, áudio silencioso) via ffmpeg.
    fn create_temp_mp4(path: &Path) {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=320x240:r=25",
                "-t",
                "1",
                "-shortest",
                &path.to_string_lossy(),
            ])
            .output()
            .expect("ffmpeg deve estar disponível no ambiente de teste");

        assert!(status.status.success(), "Falha ao criar MP4 de teste");
    }

    // ------------------------------------------------------------------
    // Testes unitários — lógica pura
    // ------------------------------------------------------------------

    #[test]
    fn build_output_path_usa_stem_do_input() {
        let dir = std::env::temp_dir().join("rust_stt_test_path");
        let input = Path::new("/qualquer/caminho/meu_video.mp4");
        let output = build_output_path(input, &dir).unwrap();
        assert_eq!(output.file_name().unwrap(), "meu_video.wav");
        assert_eq!(output.parent().unwrap(), dir);
        // limpeza
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_output_path_cria_diretorio_se_nao_existir() {
        let dir = std::env::temp_dir().join("rust_stt_test_mkdir");
        let _ = std::fs::remove_dir_all(&dir); // garantir ausência
        let input = Path::new("video.mp4");
        build_output_path(input, &dir).unwrap();
        assert!(dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_input_rejeita_arquivo_inexistente() {
        let err = validate_input(Path::new("/nao/existe/video.mp4")).unwrap_err();
        assert!(matches!(err, ConverterError::InputNotFound(_)));
    }

    #[test]
    fn validate_input_rejeita_extensao_invalida() {
        // Cria arquivo temporário com extensão errada
        let tmp = std::env::temp_dir().join("rust_stt_test_ext.avi");
        std::fs::write(&tmp, b"dummy").unwrap();
        let err = validate_input(&tmp).unwrap_err();
        assert!(matches!(err, ConverterError::InvalidExtension(_)));
        let _ = std::fs::remove_file(&tmp);
    }

    // ------------------------------------------------------------------
    // Testes de integração — conversão real
    // ------------------------------------------------------------------

    #[test]
    fn convert_mp4_to_wav_gera_arquivo_wav() {
        let tmp_dir = std::env::temp_dir().join("rust_stt_integration");
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let input_path = tmp_dir.join("test_input.mp4");
        create_temp_mp4(&input_path);

        let output_dir = tmp_dir.join("out");
        let result = convert_mp4_to_wav(&input_path, &output_dir);

        assert!(result.is_ok(), "Conversão falhou: {:?}", result.err());
        let output_path = result.unwrap();
        assert!(output_path.exists(), "Arquivo WAV não foi criado");
        assert_eq!(output_path.extension().unwrap(), "wav");
        assert!(
            output_path.metadata().unwrap().len() > 0,
            "Arquivo WAV está vazio"
        );

        // limpeza
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn convert_mp4_to_wav_verifica_parametros_de_audio() {
        let tmp_dir = std::env::temp_dir().join("rust_stt_audio_params");
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let input_path = tmp_dir.join("test_input.mp4");
        create_temp_mp4(&input_path);

        let output_dir = tmp_dir.join("out");
        let output_path = convert_mp4_to_wav(&input_path, &output_dir).unwrap();

        // Inspeciona os metadados via ffprobe
        let probe = Command::new("ffprobe")
            .args([
                "-v", "error",
                "-select_streams", "a:0",
                "-show_entries", "stream=sample_rate,channels,codec_name",
                "-of", "default=noprint_wrappers=1",
                &output_path.to_string_lossy(),
            ])
            .output()
            .expect("ffprobe deve estar disponível");

        let info = String::from_utf8_lossy(&probe.stdout);
        assert!(info.contains("sample_rate=16000"), "Taxa de amostragem deve ser 16000 Hz, got: {info}");
        assert!(info.contains("channels=1"), "Deve ser mono (1 canal), got: {info}");
        assert!(info.contains("codec_name=pcm_s16le"), "Codec deve ser pcm_s16le, got: {info}");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn convert_mp4_to_wav_falha_com_entrada_inexistente() {
        let err = convert_mp4_to_wav(Path::new("/nao/existe/video.mp4"), Path::new("/tmp/out"))
            .unwrap_err();
        assert!(matches!(err, ConverterError::InputNotFound(_)));
    }

    #[test]
    fn convert_mp4_to_wav_sobrescreve_arquivo_existente() {
        let tmp_dir = std::env::temp_dir().join("rust_stt_overwrite");
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let input_path = tmp_dir.join("test_input.mp4");
        create_temp_mp4(&input_path);

        let output_dir = tmp_dir.join("out");

        // Primeira conversão
        convert_mp4_to_wav(&input_path, &output_dir).unwrap();
        // Segunda — deve sobrescrever sem erro
        let result = convert_mp4_to_wav(&input_path, &output_dir);
        assert!(
            result.is_ok(),
            "Segunda conversão falhou: {:?}",
            result.err()
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
