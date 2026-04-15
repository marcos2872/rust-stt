//! Etapas individuais de filtragem de áudio como strings de filtro do ffmpeg.
//!
//! Cada função retorna um fragmento do filtro `lavfi` que pode ser encadeado
//! com vírgula. A ordem recomendada está documentada em [`super::build_filter_chain`].

// ---------------------------------------------------------------------------
// Etapas originais
// ---------------------------------------------------------------------------

/// **Etapa 1 — Bandpass**: HPF + LPF para isolar a faixa da voz.
///
/// Remove frequências abaixo de `hpf_hz` (ruído de baixa frequência, vibração)
/// e acima de `lpf_hz` (chiado de alta frequência desnecessário para fala).
///
/// > **Atenção**: `lpf_hz` deve ser ≤ Nyquist do sample rate de saída.
/// > Para saída em 16 kHz o limite é **8 000 Hz** — valores maiores serão
/// > cortados pelo resampler antes do filtro ter efeito.
pub fn bandpass(hpf_hz: u32, lpf_hz: u32) -> String {
    format!("highpass=f={hpf_hz},lowpass=f={lpf_hz}")
}

/// **Etapa 2a — Click Removal**: Remove cliques, estouros e ruídos impulsivos.
///
/// Usa `adeclick` para detectar e interpolar amostras com descontinuidades
/// abruptas. Deve vir antes dos filtros espectrais pois cliques corrompem
/// a análise de FFT.
///
/// - `window_ms`: tamanho da janela de análise em ms (padrão: 55)
/// - `overlap`: sobreposição entre janelas em % — range [50–95] (padrão: 75)
pub fn click_removal(window_ms: f32, overlap: u32) -> String {
    format!("adeclick=w={window_ms}:o={overlap}")
}

/// **Etapa 2b — Noise Reduction (FFT)**: Redução de ruído estacionário via FFT.
///
/// `afftdn` atenua componentes espectrais abaixo do piso de ruído estimado.
/// `noise_floor_db` define o limiar (ex.: `-25`). Menos agressivo que nlmeans,
/// processa mais rápido.
pub fn noise_reduction_fft(noise_floor_db: i32) -> String {
    format!("afftdn=nf={noise_floor_db}")
}

/// **Etapa 2c — Noise Reduction (Non-Local Means)**: Redução de ruído de fundo
/// via análise estatística não-local (`anlmdn`).
///
/// Complementa `afftdn`: enquanto o FFT remove ruído estacionário (chiado
/// constante), o NLMDN lida melhor com vozes de fundo e ruídos não-correlatos.
///
/// - `strength`: força da denoising (padrão: 7.0, range: 1–100)
/// - `patch_radius_s`: raio do patch em segundos (padrão: 0.002)
/// - `research_radius_s`: raio de busca em segundos (padrão: 0.002)
/// - `max_gain`: ganho máximo de correção (padrão: 15.0)
pub fn noise_reduction_nlmeans(
    strength: f32,
    patch_radius_s: f32,
    research_radius_s: f32,
    max_gain: f32,
) -> String {
    format!("anlmdn=s={strength}:p={patch_radius_s}:r={research_radius_s}:m={max_gain}")
}

/// **Etapa 3 — Voice EQ**: Realça frequências da fala (2–4 kHz).
///
/// Usa um filtro paramétrico de dois polos (`equalizer`) centrado em
/// `freq_hz` com ganho `gain_db` e largura de banda `bandwidth_hz`
/// em Hz (`t=h`).
///
/// > **Atenção**: use `t=h` (bandwidth em Hz). `t=o` interpreta `w` em
/// > oitavas e valores altos (ex.: 2000) anulam todo o sinal.
pub fn voice_eq(freq_hz: u32, gain_db: f32, bandwidth_hz: u32) -> String {
    format!("equalizer=f={freq_hz}:t=h:width={bandwidth_hz}:g={gain_db}")
}

/// **Etapa 4a — Compression**: Compressão leve para equalizar picos de fala.
///
/// Suaviza transientes sem distorcer. `makeup_db` compensa a perda de ganho
/// introduzida pela compressão.
pub fn compression(threshold_db: i32, ratio: f32, makeup_db: f32) -> String {
    format!(
        "acompressor=threshold={threshold_db}dB:ratio={ratio}:attack=5:release=50:makeup={makeup_db}dB"
    )
}

/// **Etapa 4b — Noise Gate**: Suprime fundo entre segmentos de fala.
///
/// Fecha o gate em momentos abaixo do limiar, silenciando vozes de fundo
/// fracas e ruído ambiente entre palavras. O `attack` lento (5ms) e `knee`
/// suave evitam cortar plosivas e consoantes iniciais.
///
/// - `threshold`: limiar de abertura linear (0.0–1.0, padrão: 0.01 ≈ -40 dBFS)
/// - `ratio`: taxa de supressão abaixo do limiar (padrão: 10.0)
/// - `knee`: largura da curva de transição em dB (padrão: 2.828)
pub fn noise_gate(threshold: f32, ratio: f32, knee: f32) -> String {
    format!(
        "agate=threshold={threshold}:attack=5:release=100:ratio={ratio}:knee={knee}"
    )
}

/// **Etapa 5a — Loudness Normalization**: Normalização dinâmica de loudness.
///
/// Usa `dynaudnorm` (Dynamic Audio Normalizer), que opera em single-pass e é
/// compatível com ffmpeg 7.x. `peak` define o nível de pico alvo (0.0–1.0) e
/// `max_gain` limita o ganho máximo aplicado.
///
/// > Nota: `loudnorm` (EBU R128 two-pass) não é usado aqui pois requer dois
/// > passes completos e pode travar em single-pass com arquivos longos.
pub fn loudness_normalization(peak: f32, max_gain: f32) -> String {
    format!("dynaudnorm=p={peak}:m=100:r=0.5:maxgain={max_gain}")
}

/// **Etapa 5b — Limiter**: Limitador lookahead para controlar picos residuais.
///
/// Aplicado após a normalização como proteção final. Limita hard acima de
/// `limit` (0.0–1.0) sem distorcer a fala normal, pois usa lookahead de 5ms.
///
/// > Use `level=disabled` para não reprocessar o nível de saída.
pub fn limiter(limit: f32, attack_ms: f32, release_ms: f32) -> String {
    format!("alimiter=limit={limit}:attack={attack_ms}:release={release_ms}:level=disabled")
}

/// **Etapa 6 — Silence Removal (VAD)**: Remove silêncios excessivos.
///
/// Remove silêncios ao longo de todo o áudio (início, meio e fim).
/// `threshold_db` define o limiar de silêncio; `min_duration_s` é a duração
/// mínima de silêncio para ser removida.
///
/// > Nota: Em ffmpeg 7.x, `start_periods=N` requer que o áudio comece com
/// > silêncio. Usamos apenas `stop_periods=-1` para remover silêncios
/// > consistentemente em qualquer conteúdo.
pub fn silence_removal(threshold_db: i32, min_duration_s: f32) -> String {
    format!(
        "silenceremove=\
        stop_periods=-1:\
        stop_silence={min_duration_s}:\
        stop_threshold={threshold_db}dB"
    )
}

// ---------------------------------------------------------------------------
// Testes unitários das strings de filtro
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bandpass_gera_highpass_e_lowpass() {
        let f = bandpass(100, 16000);
        assert_eq!(f, "highpass=f=100,lowpass=f=16000");
    }

    #[test]
    fn click_removal_inclui_window_e_overlap() {
        let f = click_removal(55.0, 75);
        assert!(f.contains("adeclick"), "deve usar adeclick: {f}");
        assert!(f.contains("w=55"), "window ausente: {f}");
        assert!(f.contains("o=75"), "overlap ausente: {f}");
    }

    #[test]
    fn noise_reduction_fft_usa_nf_correto() {
        let f = noise_reduction_fft(-25);
        assert_eq!(f, "afftdn=nf=-25");
    }

    #[test]
    fn noise_reduction_nlmeans_inclui_todos_parametros() {
        let f = noise_reduction_nlmeans(7.0, 0.002, 0.002, 15.0);
        assert!(f.contains("anlmdn"), "deve usar anlmdn: {f}");
        assert!(f.contains("s=7"), "strength ausente: {f}");
        assert!(f.contains("p=0.002"), "patch_radius ausente: {f}");
        assert!(f.contains("r=0.002"), "research_radius ausente: {f}");
        assert!(f.contains("m=15"), "max_gain ausente: {f}");
    }

    #[test]
    fn voice_eq_gera_equalizer_correto() {
        let f = voice_eq(3000, 3.0, 2000);
        assert_eq!(f, "equalizer=f=3000:t=h:width=2000:g=3");
    }

    #[test]
    fn compression_inclui_threshold_e_ratio() {
        let f = compression(-18, 3.0, 2.0);
        assert!(f.contains("threshold=-18dB"), "got: {f}");
        assert!(f.contains("ratio=3"), "got: {f}");
        assert!(f.contains("makeup=2dB"), "got: {f}");
    }

    #[test]
    fn noise_gate_inclui_threshold_ratio_knee() {
        let f = noise_gate(0.01, 10.0, 2.828);
        assert!(f.contains("agate"), "deve usar agate: {f}");
        assert!(f.contains("threshold=0.01"), "threshold ausente: {f}");
        assert!(f.contains("ratio=10"), "ratio ausente: {f}");
        assert!(f.contains("knee=2.828"), "knee ausente: {f}");
    }

    #[test]
    fn loudness_normalization_usa_dynaudnorm() {
        let f = loudness_normalization(0.9, 15.0);
        assert!(f.contains("dynaudnorm"), "deve usar dynaudnorm: {f}");
        assert!(f.contains("p=0.9"),      "peak incorreto: {f}");
        assert!(f.contains("maxgain=15"), "maxgain incorreto: {f}");
        assert!(f.contains("r=0.5"),      "crest factor target incorreto: {f}");
    }

    #[test]
    fn limiter_inclui_limit_attack_release() {
        let f = limiter(0.9, 5.0, 50.0);
        assert!(f.contains("alimiter"), "deve usar alimiter: {f}");
        assert!(f.contains("limit=0.9"), "limit ausente: {f}");
        assert!(f.contains("attack=5"), "attack ausente: {f}");
        assert!(f.contains("release=50"), "release ausente: {f}");
        assert!(f.contains("level=disabled"), "level=disabled ausente: {f}");
    }

    #[test]
    fn silence_removal_usa_stop_periods_menos1() {
        let f = silence_removal(-50, 0.3);
        assert!(f.contains("stop_periods=-1"), "deve usar stop_periods=-1: {f}");
        assert!(f.contains("stop_threshold=-50dB"), "threshold incorreto: {f}");
        assert!(f.contains("stop_silence=0.3"), "duração incorreta: {f}");
        // Garante que NAO usa start_periods (causa audio vazio em ffmpeg 7.x quando
        // o audio nao começa com silencio)
        assert!(!f.contains("start_periods"), "nao deve usar start_periods: {f}");
    }
}
