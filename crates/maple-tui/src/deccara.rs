//! DECSACE background-fill optimizer (port of `l/src/deccara.ts`).
//!
//! Full-width rows padded with background-colored spaces are shortened and the
//! padding is repainted with a single rectangular DECCARA sequence per run.

use crate::text_shape::visible_width;

pub const DECSACE_RECT: &str = "\x1b[2*x";
pub const DECSACE_DEFAULT: &str = "\x1b[*x";
pub const SEGMENT_RESET: &str = "\x1b[0m";

const DECSACE_WRAPPER_BYTES: usize = DECSACE_RECT.len() + DECSACE_DEFAULT.len();

/// Encode one rectangle (1-based inclusive rows/columns).
pub fn encode_deccara(top: u16, left: u16, bottom: u16, right: u16, sgr: &str) -> String {
    format!("\x1b[{top};{left};{bottom};{right};{sgr}$r")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BgFillAnalysis {
    /// Byte index where droppable trailing background padding begins.
    pub cut: usize,
    /// 0-based column where the trailing padding begins.
    pub left_col: usize,
    /// SGR parameter list covering the trailing region (`None` = default bg).
    pub bg: Option<String>,
}

/// Fold one SGR parameter list into the active background parameter string.
fn next_background(bg: Option<String>, params: &str) -> Result<Option<String>, ()> {
    if params.is_empty() {
        return Ok(None);
    }
    let tokens: Vec<&str> = params.split(';').collect();
    let mut result = bg;
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        let n: i64 = if token.is_empty() {
            0
        } else {
            token.parse().map_err(|_| ())?
        };
        if n == 0 || n == 49 {
            result = None;
            i += 1;
            continue;
        }
        if (40..=47).contains(&n) || (100..=107).contains(&n) {
            result = Some(token.to_string());
            i += 1;
            continue;
        }
        if n == 48 {
            let mode = tokens.get(i + 1).copied();
            match mode {
                Some("5") => {
                    let idx = tokens.get(i + 2).ok_or(())?;
                    result = Some(format!("48;5;{idx}"));
                    i += 3;
                    continue;
                }
                Some("2") => {
                    let r = tokens.get(i + 2).ok_or(())?;
                    let g = tokens.get(i + 3).ok_or(())?;
                    let b = tokens.get(i + 4).ok_or(())?;
                    result = Some(format!("48;2;{r};{g};{b}"));
                    i += 5;
                    continue;
                }
                _ => return Err(()),
            }
        }
        if n == 38 {
            match tokens.get(i + 1).copied() {
                Some("5") => {
                    i += 3;
                    continue;
                }
                Some("2") => {
                    i += 5;
                    continue;
                }
                _ => return Err(()),
            }
        }
        i += 1;
    }
    Ok(result)
}

/// Decide whether `line` is a full-width background fill whose trailing spaces
/// can be replaced by a DECCARA rectangle.
pub fn analyze_bg_fill_line(line: &str, width: usize) -> Option<BgFillAnalysis> {
    if width == 0 || line.is_empty() {
        return None;
    }
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut col = 0usize;
    let mut bg: Option<String> = None;
    let mut non_space_end_byte = 0usize;
    let mut non_space_end_col = 0usize;
    let mut trail_bg: Option<String> = None;
    let mut trail_started = false;
    let mut trail_consistent = true;

    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if bytes.get(i + 1) != Some(&0x5b) {
                return None;
            }
            let mut j = i + 2;
            while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                j += 1;
            }
            if j >= bytes.len() || bytes[j] != 0x6d {
                return None;
            }
            let params = &line[i + 2..j];
            bg = next_background(bg, params).ok()?;
            i = j + 1;
            continue;
        }
        let mut j = i;
        while j < bytes.len() && bytes[j] != 0x1b {
            j += 1;
        }
        let text = &line[i..j];
        let mut non_space_len = text.len();
        while non_space_len > 0 && text.as_bytes()[non_space_len - 1] == b' ' {
            non_space_len -= 1;
        }
        if non_space_len > 0 {
            let non_space = &text[..non_space_len];
            let non_space_width = visible_width(non_space);
            non_space_end_byte = i + non_space_len;
            non_space_end_col = col + non_space_width;
            if non_space_len < text.len() {
                trail_bg = bg.clone();
                trail_started = true;
            } else {
                trail_bg = None;
                trail_started = false;
            }
            trail_consistent = true;
        } else if !text.is_empty() {
            if !trail_started {
                trail_bg = bg.clone();
                trail_started = true;
            } else if bg != trail_bg {
                trail_consistent = false;
            }
        }
        col += visible_width(text);
        i = j;
    }

    if col != width || non_space_end_col >= width || !trail_started {
        return None;
    }
    let trail_bg = trail_bg?;
    if !trail_consistent {
        return None;
    }
    Some(BgFillAnalysis {
        cut: non_space_end_byte,
        left_col: non_space_end_col,
        bg: Some(trail_bg),
    })
}

#[derive(Clone, Debug)]
pub struct DeccaraPlan {
    pub texts: Vec<String>,
    pub sequence: String,
}

/// Plan the shortened rows and DECCARA batch for one frame.
pub fn plan_deccara_fills(lines: &[String], width: usize, first_screen_row: u16) -> DeccaraPlan {
    let n = lines.len();
    let mut texts: Vec<String> = Vec::with_capacity(n);
    let mut candidates: Vec<Option<FillCandidate>> = Vec::with_capacity(n);
    for line in lines {
        texts.push(line.clone());
        let analysis = analyze_bg_fill_line(line, width);
        match analysis {
            None => candidates.push(None),
            Some(analysis) => {
                let short = if analysis.cut == 0 {
                    String::new()
                } else {
                    format!("{}{SEGMENT_RESET}", &line[..analysis.cut])
                };
                candidates.push(Some(FillCandidate {
                    left: analysis.left_col + 1,
                    right: width,
                    bg: analysis.bg.unwrap_or_default(),
                    short,
                    orig_len: line.len(),
                }));
            }
        }
    }

    struct Group {
        start: usize,
        end: usize,
        rect: String,
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut removed_total = 0usize;
    let mut rect_bytes_total = 0usize;
    let mut k = 0;
    while k < n {
        let Some(head) = candidates[k].as_ref() else {
            k += 1;
            continue;
        };
        let mut end = k;
        while end + 1 < n {
            let Some(next) = candidates[end + 1].as_ref() else {
                break;
            };
            if next.left != head.left || next.right != head.right || next.bg != head.bg {
                break;
            }
            end += 1;
        }
        let rect = encode_deccara(
            first_screen_row + k as u16 + 1,
            head.left as u16,
            first_screen_row + end as u16 + 1,
            head.right as u16,
            &head.bg,
        );
        let mut removed = 0usize;
        for candidate in candidates.iter().take(end + 1).skip(k).flatten() {
            removed += candidate.orig_len - candidate.short.len();
        }
        if removed > rect.len() {
            let rect_len = rect.len();
            groups.push(Group {
                start: k,
                end,
                rect,
            });
            removed_total += removed;
            rect_bytes_total += rect_len;
        }
        k = end + 1;
    }

    if groups.is_empty() || removed_total.saturating_sub(rect_bytes_total) <= DECSACE_WRAPPER_BYTES {
        return DeccaraPlan {
            texts,
            sequence: String::new(),
        };
    }
    let mut sequence = String::from(DECSACE_RECT);
    for group in &groups {
        for (offset, candidate) in candidates
            .iter_mut()
            .take(group.end + 1)
            .skip(group.start)
            .enumerate()
        {
            if let Some(candidate) = candidate {
                texts[group.start + offset] = candidate.short.clone();
            }
        }
        sequence.push_str(&group.rect);
    }
    sequence.push_str(DECSACE_DEFAULT);
    DeccaraPlan { texts, sequence }
}

#[derive(Clone)]
struct FillCandidate {
    left: usize,
    right: usize,
    bg: String,
    short: String,
    orig_len: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_basic_and_extended_backgrounds() {
        assert_eq!(next_background(None, "44").unwrap(), Some("44".into()));
        assert_eq!(
            next_background(None, "48;5;196").unwrap(),
            Some("48;5;196".into())
        );
        assert_eq!(
            next_background(None, "48;2;1;2;3").unwrap(),
            Some("48;2;1;2;3".into())
        );
        assert_eq!(next_background(Some("44".into()), "0").unwrap(), None);
        assert_eq!(next_background(None, "38;5;9").unwrap(), None);
        assert!(next_background(None, "48:2:1:2:3").is_err());
    }

    #[test]
    fn detects_trailing_fill() {
        let line = "\x1b[44mhello   \x1b[0m";
        let analysis = analyze_bg_fill_line(line, 8).unwrap();
        assert_eq!(analysis.left_col, 5);
        assert_eq!(analysis.bg.as_deref(), Some("44"));
        assert!(analysis.cut > 0);
    }

    #[test]
    fn refuses_partial_and_default_fills() {
        assert!(analyze_bg_fill_line("\x1b[44mhi\x1b[0m", 8).is_none());
        assert!(analyze_bg_fill_line("hello   ", 8).is_none());
        assert!(analyze_bg_fill_line("\x1b]8;;url\x1b\\hi\x1b[44m   \x1b[0m", 8).is_none());
    }

    #[test]
    fn plans_only_when_worthwhile() {
        let line = |word: &str| format!("\x1b[44m{word}{}\x1b[0m", " ".repeat(24));
        let lines = vec![line("hello"), line("world")];
        let plan = plan_deccara_fills(&lines, 29, 0);
        assert!(plan.sequence.starts_with(DECSACE_RECT));
        assert!(plan.sequence.ends_with(DECSACE_DEFAULT));
        assert_eq!(plan.texts[0], "\x1b[44mhello\x1b[0m");
    }
}
