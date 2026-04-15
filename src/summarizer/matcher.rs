//! Matching por timestamp entre segmentos do Azure Speech e entradas do VTT.
//!
//! Para cada `Speaker N` na transcrição, encontra o nome real no VTT
//! verificando sobreposição de janelas de tempo (tolerância de ±1500 ms).

use std::collections::HashMap;
use crate::summarizer::vtt::VttEntry;

/// Resultado do matching de um falante.
#[derive(Debug, Clone)]
pub struct SpeakerMatch {
    /// Rótulo original, ex.: `"Speaker 1"`.
    pub speaker_label: String,
    /// Nome real identificado pelo matching, ex.: `"Mariana Cardoso Fabre Albino"`.
    pub name: String,
    /// Número de segmentos que contribuíram para a decisão.
    pub match_count: usize,
    /// Confiança: proporção de segmentos com match (0.0–1.0).
    pub confidence: f32,
}

/// Um segmento mínimo da transcrição necessário para o matching.
#[derive(Debug)]
pub struct TranscriptSegment<'a> {
    pub speaker: &'a str,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Executa o matching e devolve um mapa `Speaker N` → [`SpeakerMatch`].
///
/// Algoritmo:
/// 1. Para cada segmento da transcrição, busca entradas do VTT cujo intervalo
///    de tempo se sobreponha (com tolerância de `tolerance_ms`).
/// 2. Conta votos por nome para cada rótulo de falante.
/// 3. O nome com mais votos vence.
pub fn match_speakers<'a>(
    segments:     &[TranscriptSegment<'a>],
    vtt:          &[VttEntry],
    tolerance_ms: u64,
) -> HashMap<String, SpeakerMatch> {
    // votes[speaker_label][name] = count
    let mut votes: HashMap<&str, HashMap<String, usize>> = HashMap::new();

    for seg in segments {
        let overlapping: Vec<&str> = vtt
            .iter()
            .filter(|e| overlaps(seg.start_ms, seg.end_ms, e.start_ms, e.end_ms, tolerance_ms))
            .map(|e| e.name.as_str())
            .collect();

        if !overlapping.is_empty() {
            let entry = votes.entry(seg.speaker).or_default();
            // Voto ponderado: cada sobreposição conta uma vez
            for name in overlapping {
                *entry.entry(name.to_string()).or_insert(0) += 1;
            }
        }
    }

    let mut result: HashMap<String, SpeakerMatch> = HashMap::new();

    for (speaker, name_votes) in votes {
        let total_segs = segments.iter().filter(|s| s.speaker == speaker).count();
        let (best_name, best_count) = name_votes
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .unwrap();

        result.insert(speaker.to_string(), SpeakerMatch {
            speaker_label: speaker.to_string(),
            name:          best_name,
            match_count:   best_count,
            confidence:    best_count as f32 / total_segs.max(1) as f32,
        });
    }

    result
}

/// `true` se os dois intervalos se sobrepõem, considerando uma tolerância.
fn overlaps(a_start: u64, a_end: u64, b_start: u64, b_end: u64, tol: u64) -> bool {
    let a_s = a_start.saturating_sub(tol);
    let a_e = a_end + tol;
    let b_s = b_start.saturating_sub(tol);
    let b_e = b_end + tol;
    a_s < b_e && b_s < a_e
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vtt(name: &str, start_ms: u64, end_ms: u64) -> VttEntry {
        VttEntry { name: name.to_string(), start_ms, end_ms, text: String::new() }
    }

    #[test]
    fn overlaps_detecta_sobreposicao_direta() {
        assert!(overlaps(1000, 3000, 2000, 4000, 0));
    }

    #[test]
    fn overlaps_detecta_sobreposicao_com_tolerancia() {
        // Sem tolerância não sobrepõe; com 500ms sobrepõe
        assert!(!overlaps(0, 1000, 1500, 3000, 0));
        assert!(overlaps(0, 1000, 1500, 3000, 600));
    }

    #[test]
    fn overlaps_nao_sobrepoe_distantes() {
        assert!(!overlaps(0, 1000, 5000, 8000, 1000));
    }

    #[test]
    fn match_speakers_identifica_nome_correto() {
        let vtt = vec![
            make_vtt("Mariana Cardoso", 0,     5_000),
            make_vtt("Mariana Cardoso", 6_000, 10_000),
            make_vtt("Joana Lopes",     11_000, 15_000),
            make_vtt("Joana Lopes",     16_000, 20_000),
        ];

        let segments = vec![
            TranscriptSegment { speaker: "Speaker 1", start_ms: 500,    end_ms: 4_500 },
            TranscriptSegment { speaker: "Speaker 1", start_ms: 6_500,  end_ms: 9_500 },
            TranscriptSegment { speaker: "Speaker 2", start_ms: 11_500, end_ms: 14_500 },
            TranscriptSegment { speaker: "Speaker 2", start_ms: 16_500, end_ms: 19_500 },
        ];

        let result = match_speakers(&segments, &vtt, 1_500);

        assert_eq!(result["Speaker 1"].name, "Mariana Cardoso");
        assert_eq!(result["Speaker 2"].name, "Joana Lopes");
        assert!(result["Speaker 1"].confidence > 0.9);
    }

    #[test]
    fn match_speakers_com_vtt_vazio_retorna_vazio() {
        let segments = vec![
            TranscriptSegment { speaker: "Speaker 1", start_ms: 0, end_ms: 1000 },
        ];
        let result = match_speakers(&segments, &[], 1_500);
        assert!(result.is_empty());
    }
}
