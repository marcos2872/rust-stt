//! Parser de arquivos VTT gerados pelo Microsoft Teams.
//!
//! Formato esperado:
//! ```text
//! WEBVTT
//!
//! c83a7d7c-ef1b-4a98-8cc0-8abe652ded43/11-0
//! 00:00:03.833 --> 00:00:10.338
//! <v Mariana Cardoso Fabre Albino>A coordenadora pedagógica lá da unidade.
//! E era a pergunta dele sobre se uma turma,</v>
//! ```

/// Uma entrada do VTT: falante, timestamps e texto.
#[derive(Debug, Clone)]
pub struct VttEntry {
    /// Nome completo extraído da tag `<v Name>`.
    pub name: String,
    /// Início em millisegundos.
    pub start_ms: u64,
    /// Fim em millisegundos.
    pub end_ms: u64,
    /// Texto transcrito (newlines internos substituídos por espaço).
    pub text: String,
}

/// Parseia o conteúdo de um arquivo VTT e retorna todas as entradas com voz.
/// Entradas sem tag `<v>` são descartadas.
pub fn parse(content: &str) -> Vec<VttEntry> {
    let mut entries = Vec::new();
    let mut lines   = content.lines().peekable();

    while let Some(line) = lines.next() {
        let line = line.trim();

        // Linha de timestamp: "HH:MM:SS.mmm --> HH:MM:SS.mmm"
        if !line.contains("-->") {
            continue;
        }

        let (start_ms, end_ms) = match parse_timestamp_line(line) {
            Some(v) => v,
            None    => continue,
        };

        // Coleta linhas de texto até linha vazia ou fim
        let mut text_lines: Vec<&str> = Vec::new();
        loop {
            match lines.peek() {
                None | Some(&"") => { lines.next(); break; }
                Some(_)          => text_lines.push(lines.next().unwrap()),
            }
        }

        let raw = text_lines.join("\n");
        if let Some((name, text)) = extract_voice_tag(&raw) {
            entries.push(VttEntry { name, start_ms, end_ms, text });
        }
    }

    entries
}

// ---------------------------------------------------------------------------
// Helpers privados
// ---------------------------------------------------------------------------

fn parse_timestamp_line(line: &str) -> Option<(u64, u64)> {
    let mut it = line.splitn(2, "-->");
    let start  = parse_ts(it.next()?.trim())?;
    let end    = parse_ts(it.next()?.trim().split_whitespace().next()?)?;
    Some((start, end))
}

/// `"HH:MM:SS.mmm"` ou `"MM:SS.mmm"` → millisegundos.
fn parse_ts(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        3 => {
            let h: u64 = parts[0].parse().ok()?;
            let m: u64 = parts[1].parse().ok()?;
            let (sec, ms) = split_sec_ms(parts[2])?;
            Some(h * 3_600_000 + m * 60_000 + sec * 1_000 + ms)
        }
        2 => {
            let m: u64 = parts[0].parse().ok()?;
            let (sec, ms) = split_sec_ms(parts[1])?;
            Some(m * 60_000 + sec * 1_000 + ms)
        }
        _ => None,
    }
}

fn split_sec_ms(s: &str) -> Option<(u64, u64)> {
    let mut it  = s.splitn(2, '.');
    let sec: u64 = it.next()?.parse().ok()?;
    let ms_str   = it.next().unwrap_or("0");
    let padded   = format!("{:0<3}", &ms_str[..ms_str.len().min(3)]);
    let ms: u64  = padded.parse().unwrap_or(0);
    Some((sec, ms))
}

/// Extrai `(nome, texto)` de `"<v Name>text\nmore text</v>"`.
/// Também lida com tags internas como `<lang pt-BR>...</lang>`.
fn extract_voice_tag(raw: &str) -> Option<(String, String)> {
    let v_start    = raw.find("<v ")?;
    let name_start = v_start + 3;
    let name_end   = raw[name_start..].find('>')? + name_start;
    let name       = raw[name_start..name_end].trim().to_string();

    if name.is_empty() { return None; }

    let text_start = name_end + 1;
    let text_raw   = if let Some(p) = raw[text_start..].find("</v>") {
        &raw[text_start..text_start + p]
    } else {
        &raw[text_start..]
    };

    let text = strip_tags(text_raw)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if text.is_empty() { return None; }

    Some((name, text))
}

/// Remove tags HTML/XML preservando o texto entre elas.
fn strip_tags(s: &str) -> String {
    let mut out    = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _   => if !in_tag { out.push(ch); }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "WEBVTT\n\nc83a7d7c/11-0\n00:00:03.833 --> 00:00:10.338\n<v Mariana Cardoso Fabre Albino>A coordenadora pedagógica lá da unidade.\nE era a pergunta dele sobre se uma turma,</v>\n\nc83a7d7c/10-0\n00:00:04.153 --> 00:00:05.193\n<v Joana Lopes>Next time.</v>\n\nc83a7d7c/13-0\n00:00:07.233 --> 00:00:11.357\n<v Joana Lopes>Yeah. So we'll go ahead.</v>\n\nc83a7d7c/11-1\n00:00:10.338 --> 00:00:15.353\n<v Mariana Cardoso Fabre Albino>se um professor teria mais que uma turma.</v>\n";

    #[test]
    fn parse_retorna_quatro_entradas() {
        assert_eq!(parse(SAMPLE).len(), 4);
    }

    #[test]
    fn parse_extrai_nomes() {
        let e = parse(SAMPLE);
        assert_eq!(e[0].name, "Mariana Cardoso Fabre Albino");
        assert_eq!(e[1].name, "Joana Lopes");
    }

    #[test]
    fn parse_converte_timestamps() {
        let e = parse(SAMPLE);
        assert_eq!(e[0].start_ms, 3_833);
        assert_eq!(e[0].end_ms,   10_338);
    }

    #[test]
    fn parse_junta_linhas_sem_newline() {
        let e = parse(SAMPLE);
        assert!(e[0].text.contains("coordenadora"));
        assert!(e[0].text.contains("turma"));
        assert!(!e[0].text.contains('\n'));
    }

    #[test]
    fn parse_ts_hhmmss() {
        assert_eq!(parse_ts("00:01:03.500"), Some(63_500));
    }

    #[test]
    fn parse_ts_mmss() {
        assert_eq!(parse_ts("01:03.500"), Some(63_500));
    }

    #[test]
    fn strip_tags_remove_lang() {
        assert_eq!(strip_tags("<lang pt-BR>Olá</lang>"), "Olá");
    }
}
