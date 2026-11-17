//! Source-level handling of ```svg fences in assistant Markdown: splitting
//! prose around them ([`split_svg_fences`]), turning a fence that is still
//! streaming into a document an SVG parser accepts ([`close_partial_svg`]), and
//! resolving the theme tokens figures color themselves with ([`prepare_svg`]).
//!
//! Reference: `l/src/chat/svg-source.ts`.
//!
//! Deviations: Rust's regex has no lookahead, so the root-tag pattern spells
//! the post-`svg` delimiter out; the palette is a `HashMap` instead of a plain
//! record.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

/// One run of an assistant text block: prose, or the body of a ```svg fence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FigureSegment {
    /// Prose between figures.
    Markdown {
        /// The prose source.
        text: String,
    },
    /// A fence body; `closed` once the fence's closing line arrived.
    Svg {
        /// The fence body.
        source: String,
        /// Whether the closing fence line arrived.
        closed: bool,
    },
}

/// A top-level (≤3-space indent) ```svg / ~~~svg fence opener, info string
/// compared ASCII-case-insensitively.
static SVG_FENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^ {0,3}(?:`{3,}|~{3,})[ \t]*svg(?:[ \t]|$)").expect("valid svg fence")
});

/// A fence opener, capturing the marker run and the rest of the line.
static FENCE_OPEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ {0,3}(`{3,}|~{3,})(.*)$").expect("valid fence open")
});

/// `var(--name)` / `var(--name, fallback)`; the fallback may hold one level of
/// parentheses (`rgb(…)`).
static VAR_REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"var\(\s*--([\w-]+)\s*(?:,\s*((?:[^()]|\([^()]*\))*))?\)").expect("valid var")
});

/// The first `<svg …>` start tag (namespace prefix allowed).
static ROOT_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<(?:[\w.-]+:)?svg(?:[\s/>][^>]*)?>").expect("valid root tag")
});

/// The root element's name.
static TAG_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^<([^\s/>]+)").expect("valid tag name"));

/// Whether `markdown` holds a ```svg fence [`split_svg_fences`] would lift.
pub fn has_svg_fence(markdown: &str) -> bool {
    markdown.contains("svg") && SVG_FENCE.is_match(markdown)
}

/// Split `markdown` into prose and ```svg fence bodies, in order.
///
/// Only top-level fences lift; an svg fence nested in another fence, a list's
/// indented block or a blockquote stays prose. Blank prose runs are dropped. An
/// svg fence still open at the end is the last segment, `closed: false`.
pub fn split_svg_fences(markdown: &str) -> Vec<FigureSegment> {
    let bytes = markdown.as_bytes();
    let mut segments = Vec::new();
    /// Open fence: its marker run, and where an svg fence's body starts
    /// (`None` for other fences).
    let mut fence: Option<(String, Option<usize>)> = None;
    let mut prose_start = 0;
    let mut line_start = 0;
    while line_start <= markdown.len() {
        let mut line_end = line_start;
        while line_end < markdown.len() && bytes[line_end] != b'\n' && bytes[line_end] != b'\r' {
            line_end += 1;
        }
        let next = if line_end == markdown.len() {
            markdown.len() + 1
        } else if bytes[line_end] == b'\r' && bytes.get(line_end + 1) == Some(&b'\n') {
            line_end + 2
        } else {
            line_end + 1
        };
        let line = &markdown[line_start..line_end];
        if let Some((marker, body)) = &fence {
            if closes_fence(line, marker) {
                if let Some(body) = *body {
                    segments.push(FigureSegment::Svg {
                        source: markdown[body..line_start].to_string(),
                        closed: true,
                    });
                    prose_start = next.min(markdown.len());
                }
                fence = None;
            }
        } else if let Some(caps) = FENCE_OPEN.captures(line) {
            let run = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            let info = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
            // A backtick fence's info string cannot hold backticks (CommonMark).
            if !(run.starts_with('`') && info.contains('`')) {
                let is_svg = info
                    .trim()
                    .split([' ', '\t'])
                    .next()
                    .unwrap_or_default()
                    .to_lowercase()
                    == "svg";
                if is_svg {
                    let prose = &markdown[prose_start..line_start];
                    if !prose.trim().is_empty() {
                        segments.push(FigureSegment::Markdown {
                            text: prose.to_string(),
                        });
                    }
                    fence = Some((run.to_string(), Some(next.min(markdown.len()))));
                } else {
                    fence = Some((run.to_string(), None));
                }
            }
        }
        line_start = next;
    }
    if let Some((_, Some(body))) = fence {
        segments.push(FigureSegment::Svg {
            source: markdown[body..].to_string(),
            closed: false,
        });
    } else if !markdown[prose_start..].trim().is_empty() {
        segments.push(FigureSegment::Markdown {
            text: markdown[prose_start..].to_string(),
        });
    }
    segments
}

/// Whether `line` closes a fence opened by `marker`: same character, at least
/// as long, nothing after.
fn closes_fence(line: &str, marker: &str) -> bool {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < 3 && bytes.get(index) == Some(&b' ') {
        index += 1;
    }
    let first = marker.as_bytes().first().copied().unwrap_or_default();
    let mut run = 0;
    while bytes.get(index + run) == Some(&first) {
        run += 1;
    }
    run >= marker.len() && line[index + run..].trim().is_empty()
}

/// Turn a possibly truncated SVG source into a well-formed document: cut the
/// trailing construct still being written (a tag without its `>`, an
/// unterminated comment, CDATA section, processing instruction or declaration,
/// a dangling `&entity`) and close every element left open, innermost first.
///
/// A complete document passes through unchanged. Returns `None` until the root
/// `<svg …>` start tag is complete — before that there is nothing to draw.
pub fn close_partial_svg(source: &str) -> Option<String> {
    let mut open: Vec<String> = Vec::new();
    let mut root_seen = false;
    let mut end = 0;
    let mut index = 0;
    loop {
        let Some(lt) = source[index..].find('<').map(|at| index + at) else {
            // Trailing text: an entity reference still being written would not parse.
            let amp = source.rfind('&').filter(|amp| *amp >= index);
            end = match amp {
                Some(amp) if !source[amp..].contains(';') => amp,
                _ => source.len(),
            };
            break;
        };
        end = lt;
        let Some(close) = construct_end(source, lt) else {
            break;
        };
        if source[lt..].starts_with("</") {
            let name = source[lt + 2..close - 1].trim();
            if let Some(at) = open.iter().rposition(|entry| entry == name) {
                open.truncate(at);
            }
        } else if !matches!(source.as_bytes().get(lt + 1), Some(b'!') | Some(b'?')) {
            let name = TAG_NAME
                .captures(&source[lt..close])
                .and_then(|caps| caps.get(1))
                .map(|m| m.as_str().to_string());
            if let Some(name) = name {
                if source.as_bytes().get(close - 2) != Some(&b'/') {
                    open.push(name.clone());
                }
                if !root_seen && (name == "svg" || name.ends_with(":svg")) {
                    root_seen = true;
                }
            }
        }
        index = close;
        end = close;
    }
    if !root_seen {
        return None;
    }
    let mut document = source[..end].to_string();
    for name in open.iter().rev() {
        document.push_str("</");
        document.push_str(name);
        document.push('>');
    }
    Some(document)
}

/// Make a figure's source what an SVG rasterizer draws as written, in the
/// reader's theme.
///
/// Rasterizers resolve neither CSS custom properties nor an inherited text
/// color, and reject a root without the SVG namespace, so:
/// - `var(--name)` / `var(--name, fallback)` become `palette[name]` (an unknown
///   name takes its fallback, else `palette.fg`);
/// - a root `<svg>` lacking them gets `color` = `palette.fg` (so
///   `currentColor` follows the theme), a sans-serif `font-family` (instead of
///   the rasterizer's Times), `xmlns`, and `xmlns:xlink` when the source uses
///   `xlink:` attributes.
pub fn prepare_svg(svg: &str, palette: &HashMap<String, String>) -> String {
    let fg = palette.get("fg").cloned().unwrap_or_else(|| "currentColor".to_string());
    let resolved = VAR_REFERENCE.replace_all(svg, |caps: &regex::Captures<'_>| {
        let name = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let fallback = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
        palette
            .get(name)
            .cloned()
            .unwrap_or_else(|| {
                let fallback = fallback.trim();
                if fallback.is_empty() {
                    fg.clone()
                } else {
                    fallback.to_string()
                }
            })
    });
    let uses_xlink = resolved.contains("xlink:");
    ROOT_TAG
        .replacen(&resolved, 1, |caps: &regex::Captures<'_>| {
            let tag = caps.get(0).map(|m| m.as_str()).unwrap_or_default();
            let has_attr = |name: &str| -> bool {
                Regex::new(&format!(r"\s{}\s*=", regex::escape(name)))
                    .map(|re| re.is_match(tag))
                    .unwrap_or(false)
            };
            let mut added = String::new();
            if !has_attr("color") {
                added.push_str(&format!(" color=\"{fg}\""));
            }
            if !has_attr("font-family") {
                added.push_str(" font-family=\"sans-serif\"");
            }
            if !has_attr("xmlns") {
                added.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
            }
            if uses_xlink && !has_attr("xmlns:xlink") {
                added.push_str(" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
            }
            if added.is_empty() {
                return tag.to_string();
            }
            match tag.find(|ch: char| ch.is_whitespace() || ch == '/' || ch == '>') {
                Some(at) => format!("{}{}{}", &tag[..at], added, &tag[at..]),
                None => tag.to_string(),
            }
        })
        .into_owned()
}

/// Index just past the markup construct starting at `<` (`lt`), or `None` when
/// it is not complete yet.
fn construct_end(source: &str, lt: usize) -> Option<usize> {
    let after = |terminator: &str, from: usize| -> Option<usize> {
        source[from.min(source.len())..]
            .find(terminator)
            .map(|at| from.min(source.len()) + at + terminator.len())
    };
    if source[lt..].starts_with("<!--") {
        return after("-->", lt + 4);
    }
    if source[lt..].starts_with("<![CDATA[") {
        return after("]]>", lt + 9);
    }
    if source[lt..].starts_with("<?") {
        return after("?>", lt + 2);
    }
    // Tags and declarations end at the first `>` outside a quoted value; a
    // doctype's internal subset (`[…]`) may hold `>` of its own.
    let bytes = source.as_bytes();
    let mut quote: Option<u8> = None;
    let mut subset = false;
    for (offset, &ch) in bytes.iter().enumerate().skip(lt + 1) {
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            }
        } else if ch == b'"' || ch == b'\'' {
            quote = Some(ch);
        } else if ch == b'[' && bytes.get(lt + 1) == Some(&b'!') {
            subset = true;
        } else if ch == b']' {
            subset = false;
        } else if ch == b'>' && !subset {
            return Some(offset + 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_svg_fences() {
        assert!(has_svg_fence("```svg\n<svg/>\n```"));
        assert!(has_svg_fence("~~~SVG\nx\n~~~"));
        assert!(!has_svg_fence("```rust\nlet x = 1;\n```"));
        assert!(!has_svg_fence("plain text"));
    }

    #[test]
    fn splits_prose_around_closed_fences() {
        let segments = split_svg_fences("before\n```svg\n<svg/>\n```\nafter");
        let expected = vec![
            FigureSegment::Markdown {
                text: "before\n".into(),
            },
            FigureSegment::Svg {
                source: "<svg/>\n".into(),
                closed: true,
            },
            FigureSegment::Markdown {
                text: "after".into(),
            },
        ];
        assert_eq!(segments, expected);
    }

    #[test]
    fn open_fence_ends_the_segments() {
        let segments = split_svg_fences("```svg\n<svg>");
        assert_eq!(
            segments,
            vec![FigureSegment::Svg {
                source: "<svg>".into(),
                closed: false,
            }]
        );
    }

    #[test]
    fn nested_and_indented_fences_stay_prose() {
        // Inside another fence nothing lifts.
        let segments = split_svg_fences("```md\n```svg\n<svg/>\n```\n```");
        assert_eq!(
            segments,
            vec![FigureSegment::Markdown {
                text: "```md\n```svg\n<svg/>\n```\n```".into(),
            }]
        );
        // Four spaces is not a top-level fence.
        let segments = split_svg_fences("    ```svg\n    <svg/>\n    ```");
        assert_eq!(segments.len(), 1);
        assert!(matches!(segments[0], FigureSegment::Markdown { .. }));
    }

    #[test]
    fn closes_open_elements_innermost_first() {
        assert_eq!(
            close_partial_svg("<svg><g><rect/>"),
            Some("<svg><g><rect/></g></svg>".to_string())
        );
        assert_eq!(
            close_partial_svg("<svg><g></g></svg>"),
            Some("<svg><g></g></svg>".to_string())
        );
        // No complete root tag yet.
        assert_eq!(close_partial_svg("<sv"), None);
        assert_eq!(close_partial_svg(""), None);
    }

    #[test]
    fn cuts_a_trailing_incomplete_construct() {
        assert_eq!(
            close_partial_svg("<svg><circle r=\"1\"/><"),
            Some("<svg><circle r=\"1\"/></svg>".to_string())
        );
        // Unterminated comment.
        assert_eq!(close_partial_svg("<svg><!-- hi"), Some("<svg></svg>".into()));
        // Dangling entity reference.
        assert_eq!(close_partial_svg("<svg><t>a &am"), Some("<svg><t>a </t></svg>".into()));
        // Complete entity is kept.
        assert_eq!(
            close_partial_svg("<svg><t>a &amp;</t></svg>"),
            Some("<svg><t>a &amp;</t></svg>".into())
        );
    }

    #[test]
    fn resolves_theme_variables() {
        let mut palette = HashMap::new();
        palette.insert("fg".to_string(), "#eee".to_string());
        palette.insert("accent".to_string(), "#f0f".to_string());
        assert_eq!(
            prepare_svg("<svg><text fill=\"var(--accent)\"/></svg>", &palette),
            "<svg color=\"#eee\" font-family=\"sans-serif\" xmlns=\"http://www.w3.org/2000/svg\"><text fill=\"#f0f\"/></svg>"
        );
        // Unknown name takes its fallback, else fg.
        assert_eq!(
            prepare_svg("<svg fill=\"var(--nope, rgb(1, 2, 3))\"/>", &palette),
            "<svg color=\"#eee\" font-family=\"sans-serif\" xmlns=\"http://www.w3.org/2000/svg\" fill=\"rgb(1, 2, 3)\"/>"
        );
        assert_eq!(
            prepare_svg("<svg fill=\"var(--nope)\"/>", &palette),
            "<svg color=\"#eee\" font-family=\"sans-serif\" xmlns=\"http://www.w3.org/2000/svg\" fill=\"#eee\"/>"
        );
    }

    #[test]
    fn adds_xlink_namespace_only_when_used() {
        let mut palette = HashMap::new();
        palette.insert("fg".to_string(), "#eee".to_string());
        let prepared = prepare_svg("<svg><use xlink:href=\"#a\"/></svg>", &palette);
        assert!(prepared.contains("xmlns:xlink=\"http://www.w3.org/1999/xlink\""));
        let plain = prepare_svg("<svg><use href=\"#a\"/></svg>", &palette);
        assert!(!plain.contains("xmlns:xlink"));
        // Existing attributes are not duplicated.
        let kept = prepare_svg("<svg xmlns=\"http://www.w3.org/2000/svg\" color=\"red\"/>", &palette);
        assert!(!kept.contains("color=\"#eee\""));
        assert_eq!(kept.matches("xmlns=").count(), 1);
    }
}
