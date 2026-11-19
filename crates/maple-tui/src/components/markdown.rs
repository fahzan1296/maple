//! Markdown renderer: headings, lists, tables, code fences, quotes, rules, and
//! inline styles, wrapped to the viewport width.
//! Reference: `l/src/components/markdown.ts`.
//!
//! Bounded clean-room port of the upstream `Markdown` component: no streaming
//! cache, no syntax highlighter, no HTML block layout. Rendered rows preserve
//! ANSI styling and never exceed the requested width.

use crate::ansi_scan::strip_ansi;
use crate::component::Component;
use crate::latex_block::latex_to_block;
use crate::latex_to_unicode::{is_bare_math_environment, render_math_at};
use crate::style::{Color, Style};
use crate::symbols::SymbolTheme;
use crate::terminal_capabilities::terminal;
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis};

/// Colors and glyphs for a [`Markdown`] renderer.
#[derive(Clone, PartialEq)]
pub struct MarkdownTheme {
    /// Heading color; weight is layered on top by the renderer.
    pub heading: Style,
    /// Body text.
    pub text: Style,
    /// Optional prose foreground for plain paragraph runs (`assistantMessageText`).
    pub prose: Option<Style>,
    /// Link label.
    pub link: Style,
    /// Label color for links whose scheme an extension registered.
    pub extension_link: Style,
    /// Link destination suffix.
    pub link_url: Style,
    /// Inline code.
    pub code: Style,
    /// Code block body.
    pub code_block: Style,
    /// Code fence border lines.
    pub code_block_border: Style,
    /// Blockquote text.
    pub quote: Style,
    /// Blockquote border glyph.
    pub quote_border: Style,
    /// Horizontal rule.
    pub hr: Style,
    /// List bullet marker.
    pub list_bullet: Style,
    /// Bold spans.
    pub bold: Style,
    /// Italic spans.
    pub italic: Style,
    /// Strikethrough spans.
    pub strikethrough: Style,
    /// Underline spans.
    pub underline: Style,
    /// Table header cells.
    pub table_header: Style,
    /// Border and rule glyphs.
    pub symbols: SymbolTheme,
    /// Spaces prepended to each code block body line.
    pub code_block_indent: usize,
}

impl Default for MarkdownTheme {
    fn default() -> Self {
        MarkdownTheme {
            heading: Style::fg(Color::Cyan),
            text: Style::default(),
            prose: None,
            link: Style::fg(Color::LightBlue),
            extension_link: Style::fg(Color::LightBlue),
            link_url: Style::fg(Color::DarkGray),
            code: Style::fg(Color::Green),
            code_block: Style::fg(Color::Gray),
            code_block_border: Style::fg(Color::DarkGray),
            quote: Style::fg(Color::DarkGray),
            quote_border: Style::fg(Color::DarkGray),
            hr: Style::fg(Color::DarkGray),
            list_bullet: Style::fg(Color::Cyan),
            bold: Style::default().bold(),
            italic: Style::default().italic(),
            strikethrough: Style::default().strikethrough(),
            underline: Style::default().underline(),
            table_header: Style::default().bold(),
            symbols: SymbolTheme::unicode(),
            code_block_indent: 2,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Center,
    Right,
}

struct ListItem {
    checked: Option<bool>,
    text: String,
}

enum Block {
    Heading(u8, String),
    Paragraph(String),
    Code {
        lang: String,
        body: String,
    },
    List {
        ordered: bool,
        start: usize,
        items: Vec<ListItem>,
    },
    Quote(Vec<Block>),
    Table {
        header: Vec<String>,
        aligns: Vec<Align>,
        rows: Vec<Vec<String>>,
    },
    Hr,
    /// Display math body (`$$…$$`, `\[…\]`, or a bare math environment).
    Math(String),
}

/// Markdown document renderer.
pub struct Markdown {
    text: String,
    padding_x: u16,
    padding_y: u16,
    theme: MarkdownTheme,
    images_visible: bool,
    table_charts_visible: bool,
    tool_result_images_visible: bool,
    on_link_click: Option<Box<dyn FnMut(&str)>>,
    prose_transform: Option<Box<dyn Fn(&str) -> String>>,
    text_transform: Option<Box<dyn Fn(&str) -> String>>,
    transient_render_cache: bool,
    last_text: String,
    last_rows: Vec<String>,
    last_width: u16,
    fast_hits: usize,
    render_generation: u64,
    last_generation: u64,
    cache: Option<(String, usize, Vec<String>)>,
}

impl Default for Markdown {
    fn default() -> Self {
        Self::new()
    }
}

fn fence_open(line: &str) -> Option<(String, String)> {
    let first = line.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = line.chars().take_while(|c| *c == first).count();
    if count < 3 {
        return None;
    }
    let lang = line[count..].trim().to_string();
    Some((std::iter::repeat(first).take(count).collect(), lang))
}

fn parse_heading(line: &str) -> Option<(u8, &str)> {
    let depth = line.chars().take_while(|c| *c == '#').count();
    if depth == 0 || depth > 6 {
        return None;
    }
    let rest = &line[depth..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    Some((depth as u8, rest.trim_start()))
}

fn is_hr(line: &str) -> bool {
    let chars: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() < 3 {
        return false;
    }
    let first = chars[0];
    if first != '-' && first != '*' && first != '_' {
        return false;
    }
    chars.iter().all(|c| *c == first)
}

fn parse_list_item(line: &str) -> Option<(bool, usize, usize, Option<bool>, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let (ordered, marker_len, start) = if rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
        .is_some()
    {
        (false, 2, 1)
    } else {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }
        let after = &rest[digits.len()..];
        let separator = after.chars().next()?;
        if separator != '.' && separator != ')' {
            return None;
        }
        if !after[1..].starts_with(' ') {
            return None;
        }
        let start = digits.parse().ok()?;
        (true, digits.len() + 2, start)
    };
    let text = &rest[marker_len..];
    let (checked, text) = if let Some(rest) = text.strip_prefix("[ ] ") {
        (Some(false), rest)
    } else if let Some(rest) = text.strip_prefix("[x] ") {
        (Some(true), rest)
    } else if let Some(rest) = text.strip_prefix("[X] ") {
        (Some(true), rest)
    } else {
        (None, text)
    };
    Some((ordered, start, indent, checked, text))
}

fn is_block_start(line: &str) -> bool {
    fence_open(line).is_some()
        || parse_heading(line).is_some()
        || is_hr(line)
        || is_html_block_start(line)
        || line.starts_with('>')
        || parse_list_item(line).is_some()
}

fn split_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(str::trim).map(str::to_string).collect()
}

fn is_table_delimiter(line: &str) -> bool {
    let cells = split_row(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let cell = cell.trim();
            !cell.is_empty() && cell.contains('-') && cell.chars().all(|c| c == '-' || c == ':')
        })
}

/// Body and next line index of a display math block starting at `index`:
/// `$$…$$`, `\[…\]`, or a bare `\begin{<math env>}…\end{…}` environment.
fn math_block_at(lines: &[String], index: usize) -> Option<(String, usize)> {
    let trimmed = lines[index].trim();
    if let Some(rest) = trimmed.strip_prefix("$$") {
        return delimiter_block(lines, index, rest, "$$");
    }
    if let Some(rest) = trimmed.strip_prefix("\\[") {
        return delimiter_block(lines, index, rest, "\\]");
    }
    if let Some(rest) = trimmed.strip_prefix("\\begin{") {
        let (env, _) = rest.split_once('}')?;
        if !is_bare_math_environment(env) {
            return None;
        }
        let end_token = format!("\\end{{{env}}}");
        let mut body = String::new();
        let mut cursor = index;
        while cursor < lines.len() {
            let line = lines[cursor].trim_start();
            if let Some(at) = line.find(&end_token) {
                if cursor > index {
                    body.push('\n');
                }
                body.push_str(&line[..at + end_token.len()]);
                return Some((body, cursor + 1));
            }
            if cursor > index {
                body.push('\n');
            }
            body.push_str(line);
            cursor += 1;
        }
        return None;
    }
    None
}

/// Collect a `$$…$$` / `\[…\]` block whose opener line carries `rest`.
fn delimiter_block(lines: &[String], index: usize, rest: &str, closer: &str) -> Option<(String, usize)> {
    if let Some(at) = rest.find(closer) {
        return Some((rest[..at].trim().to_string(), index + 1));
    }
    let mut body = String::new();
    body.push_str(rest.trim());
    let mut cursor = index + 1;
    while cursor < lines.len() {
        let line = lines[cursor].trim();
        if let Some(at) = line.find(closer) {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(line[..at].trim_end());
            return Some((body, cursor + 1));
        }
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(line);
        cursor += 1;
    }
    None
}

fn parse_blocks(lines: &[String]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        if trimmed.is_empty() {
            index += 1;
            continue;
        }
        if let Some((fence, lang)) = fence_open(trimmed) {
            let mut body = String::new();
            index += 1;
            while index < lines.len() {
                if lines[index].trim_start().starts_with(fence.as_str()) {
                    index += 1;
                    break;
                }
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(lines[index].as_str());
                index += 1;
            }
            blocks.push(Block::Code { lang, body });
            continue;
        }
        if let Some((depth, text)) = parse_heading(trimmed) {
            blocks.push(Block::Heading(depth, text.to_string()));
            index += 1;
            continue;
        }
        if is_hr(trimmed) {
            blocks.push(Block::Hr);
            index += 1;
            continue;
        }
        if is_html_hr(trimmed) {
            blocks.push(Block::Hr);
            index += 1;
            continue;
        }
        if is_blockquote_start(trimmed) {
            let (inner, next) = collect_blockquote(lines, index);
            blocks.push(Block::Quote(parse_blocks(&inner)));
            index = next;
            continue;
        }
        if trimmed.starts_with('>') {
            let mut inner = Vec::new();
            while index < lines.len() {
                let candidate = lines[index].trim_start();
                if let Some(rest) = candidate.strip_prefix('>') {
                    inner.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
                    index += 1;
                } else {
                    break;
                }
            }
            blocks.push(Block::Quote(parse_blocks(&inner)));
            continue;
        }
        if trimmed.contains('|')
            && index + 1 < lines.len()
            && is_table_delimiter(lines[index + 1].trim())
        {
            let header = split_row(trimmed);
            let aligns: Vec<Align> = split_row(lines[index + 1].trim())
                .iter()
                .map(|cell| {
                    let cell = cell.trim();
                    match (cell.starts_with(':'), cell.ends_with(':')) {
                        (true, true) => Align::Center,
                        (false, true) => Align::Right,
                        _ => Align::Left,
                    }
                })
                .collect();
            let mut rows = Vec::new();
            let mut cursor = index + 2;
            while cursor < lines.len() {
                let candidate = lines[cursor].trim_start();
                if candidate.is_empty() || !candidate.contains('|') {
                    break;
                }
                rows.push(split_row(candidate));
                cursor += 1;
            }
            blocks.push(Block::Table {
                header,
                aligns,
                rows,
            });
            index = cursor;
            continue;
        }
        if let Some((ordered, start, base_indent, _, _)) = parse_list_item(trimmed) {
            let mut items: Vec<ListItem> = Vec::new();
            while index < lines.len() {
                let candidate = lines[index].trim_start();
                if candidate.is_empty() {
                    break;
                }
                match parse_list_item(candidate) {
                    Some((item_ordered, _, indent, checked, text)) if item_ordered == ordered => {
                        if indent > base_indent {
                            if let Some(last) = items.last_mut() {
                                last.text.push(' ');
                                last.text.push_str(text);
                                index += 1;
                                continue;
                            }
                        }
                        items.push(ListItem {
                            checked,
                            text: text.to_string(),
                        });
                        index += 1;
                    }
                    _ => break,
                }
            }
            blocks.push(Block::List {
                ordered,
                start,
                items,
            });
            continue;
        }
        if let Some((body, next)) = math_block_at(lines, index) {
            blocks.push(Block::Math(body));
            index = next;
            continue;
        }
        let mut paragraph = String::new();
        while index < lines.len() {
            let candidate = lines[index].trim_start();
            if candidate.is_empty() || is_block_start(candidate) {
                break;
            }
            if math_block_at(lines, index).is_some() {
                break;
            }
            if candidate.contains('|')
                && index + 1 < lines.len()
                && is_table_delimiter(lines[index + 1].trim())
            {
                break;
            }
            if !paragraph.is_empty() {
                paragraph.push('\n');
            }
            paragraph.push_str(candidate);
            index += 1;
        }
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(paragraph));
        }
    }
    blocks
}

fn escape_len(input: &str) -> usize {
    let bytes = input.as_bytes();
    if bytes.len() < 2 || bytes[0] != 0x1b {
        return bytes.len().max(1);
    }
    match bytes[1] {
        b'[' => {
            for (offset, byte) in bytes.iter().enumerate().skip(2) {
                if (0x40..=0x7e).contains(byte) {
                    return offset + 1;
                }
            }
            bytes.len()
        }
        b']' | b'P' | b'X' | b'^' | b'_' => {
            let mut index = 2;
            while index < bytes.len() {
                if bytes[index] == 0x07 {
                    return index + 1;
                }
                if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\' {
                    return index + 2;
                }
                index += 1;
            }
            bytes.len()
        }
        _ => input[1..].chars().next().map(char::len_utf8).unwrap_or(1) + 1,
    }
}

/// A `~~` opener when the run is exactly two tildes and starts real text.
fn strike_open_ok(src: &str, index: usize) -> bool {
    let bytes = src.as_bytes();
    if bytes.len() < index + 2 || bytes[index] != b'~' || bytes[index + 1] != b'~' {
        return false;
    }
    if index > 0 && bytes[index - 1] == b'~' {
        return false;
    }
    match bytes.get(index + 2) {
        Some(byte) => *byte != b'~' && !byte.is_ascii_whitespace(),
        None => false,
    }
}

/// First GFM-valid `~~` closer at or after `from`: exactly two tildes, real
/// text before it, and no third tilde after it.
fn find_strike_closer(src: &str, from: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut index = from;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] == b'~' && bytes[index + 1] == b'~' {
            let before_ok = index > from
                && bytes[index - 1] != b'~'
                && !bytes[index - 1].is_ascii_whitespace();
            let after_ok = bytes.get(index + 2) != Some(&b'~');
            if before_ok && after_ok {
                return Some(index);
            }
            index += 2;
            continue;
        }
        index += 1;
    }
    None
}

/// Split an inline HTML tag: `(name, inner, next, closed)`. `next` is the byte
/// offset just past the tag (or its closing tag). Void tags (`hr`, `br`) stop
/// at the opening tag; an unclosed tag swallows the rest as its content.
fn parse_html_inline(src: &str) -> Option<(String, String, usize, bool)> {
    let rest = src.strip_prefix('<')?;
    let close = rest.find('>')?;
    let raw_name = rest[..close].split_whitespace().next().unwrap_or("");
    let name = raw_name.trim_start_matches('/').to_ascii_lowercase();
    if name.is_empty() {
        return None;
    }
    let after_open = close + 1;
    if name == "hr" || name == "br" {
        return Some((name, String::new(), after_open, false));
    }
    if let Some(start) = find_close_tag(src, after_open, &name) {
        let end = src[start..].find('>').map(|at| start + at + 1).unwrap_or(src.len());
        Some((name, src[after_open..start].to_string(), end, true))
    } else {
        Some((name, src[after_open..].to_string(), src.len(), false))
    }
}

/// Decode `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&#NN;` and `&#xNN;`.
fn decode_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        match decode_entity(tail) {
            Some((ch, len)) => {
                out.push(ch);
                rest = &tail[len..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_entity(tail: &str) -> Option<(char, usize)> {
    let rest = tail.strip_prefix('&')?;
    for (name, ch) in [
        ("amp;", '&'),
        ("lt;", '<'),
        ("gt;", '>'),
        ("quot;", '"'),
        ("apos;", '\''),
    ] {
        if rest.strip_prefix(name).is_some() {
            return Some((ch, 1 + name.len()));
        }
    }
    let digits = rest.strip_prefix('#')?;
    let (hex, digits) = match digits.strip_prefix('x').or_else(|| digits.strip_prefix('X')) {
        Some(rest) => (true, rest),
        None => (false, digits),
    };
    let end = digits.find(';')?;
    let code = if hex {
        u32::from_str_radix(&digits[..end], 16).ok()?
    } else {
        digits[..end].parse::<u32>().ok()?
    };
    let ch = char::from_u32(code)?;
    Some((ch, 2 + usize::from(hex) + end + 1))
}

/// Byte offset of `</name` at or after `from`, ASCII case-insensitive.
fn find_close_tag(src: &str, from: usize, name: &str) -> Option<usize> {
    let bytes = src.as_bytes();
    let needle = name.as_bytes();
    let mut index = from;
    while index + needle.len() + 1 < bytes.len() {
        if bytes[index] == b'<' && bytes[index + 1] == b'/' {
            let mut matches = true;
            for (offset, byte) in needle.iter().enumerate() {
                if bytes[index + 2 + offset].to_ascii_lowercase() != *byte {
                    matches = false;
                    break;
                }
            }
            if matches {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn is_html_hr(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    lower == "<hr>" || lower == "<hr/>"
}

fn is_blockquote_start(line: &str) -> bool {
    line.trim_start().to_ascii_lowercase().starts_with("<blockquote")
}

fn is_html_block_start(line: &str) -> bool {
    is_html_hr(line) || is_blockquote_start(line)
}

/// Collect a `<blockquote>…</blockquote>` block, dropping the wrapper tags.
fn collect_blockquote(lines: &[String], index: usize) -> (Vec<String>, usize) {
    let mut inner = Vec::new();
    let mut cursor = index;
    let mut opened = false;
    while cursor < lines.len() {
        let mut line = lines[cursor].trim_start().to_string();
        if !opened {
            if let Some(at) = line.find('>') {
                line = line[at + 1..].to_string();
                opened = true;
            }
        }
        let lower = line.to_ascii_lowercase();
        if let Some(at) = lower.find("</blockquote") {
            let end = line[at..].find('>').map(|offset| at + offset + 1).unwrap_or(line.len());
            let before = line[..at].trim_end().to_string();
            if !before.is_empty() {
                inner.push(before);
            }
            return (inner, cursor + 1);
        }
        inner.push(line);
        cursor += 1;
    }
    (inner, cursor)
}

fn parse_link(rest: &str) -> Option<(String, String, usize)> {
    let close = rest.find(']')?;
    if !rest[close + 1..].starts_with('(') {
        return None;
    }
    let text = rest[..close].to_string();
    let after = &rest[close + 2..];
    let end = after.find(')')?;
    let url = after[..end].to_string();
    Some((text, url, close + 2 + end + 1))
}

fn render_inline(src: &str, theme: &MarkdownTheme, images: bool) -> String {
    render_inline_impl(src, theme, images, false, None, None)
}

/// Render inline markdown. Plain runs take `theme.prose` when `prose_enabled`
/// (paragraph context); md sub-elements and `prose_enabled == false` contexts
/// keep their own tokens. `text_transform` paints every plain run, including
/// md-owned spans, and wins over the paragraph `prose_transform`.
fn render_inline_impl(
    src: &str,
    theme: &MarkdownTheme,
    images: bool,
    prose_enabled: bool,
    text_transform: Option<&dyn Fn(&str) -> String>,
    prose_transform: Option<&dyn Fn(&str) -> String>,
) -> String {
    let mut out = String::new();
    let mut plain = String::new();
    fn flush(
        out: &mut String,
        plain: &mut String,
        theme: &MarkdownTheme,
        prose_enabled: bool,
        text_transform: Option<&dyn Fn(&str) -> String>,
        prose_transform: Option<&dyn Fn(&str) -> String>,
    ) {
        if plain.is_empty() {
            return;
        }
        let text = std::mem::take(plain);
        let text = decode_entities(&text);
        if let Some(transform) = text_transform {
            out.push_str(&transform(&text));
        } else if prose_enabled {
            if let Some(transform) = prose_transform {
                out.push_str(&transform(&text));
            } else {
                match theme.prose.as_ref() {
                    Some(style) => out.push_str(&style.paint(&text)),
                    None => out.push_str(&text),
                }
            }
        } else {
            out.push_str(&text);
        }
    }
    let mut index = 0usize;
    while index < src.len() {
        if src.as_bytes()[index] == 0x1b {
            flush(&mut out, &mut plain, theme, prose_enabled, text_transform, prose_transform);
            let len = escape_len(&src[index..]);
            out.push_str(&src[index..index + len]);
            index += len;
            continue;
        }
        let c = match src[index..].chars().next() {
            Some(c) => c,
            None => break,
        };
        if c == '\\' && src[index..].starts_with("\\\n") {
            plain.push('\n');
            index += 2;
            continue;
        }
        if c == '\\' && src[index..].starts_with("\\~") {
            plain.push('~');
            index += 2;
            continue;
        }
        if c == '<' {
            if let Some((name, inner, next, _closed)) = parse_html_inline(&src[index..]) {
                flush(&mut out, &mut plain, theme, prose_enabled, text_transform, prose_transform);
                match name.as_str() {
                    "code" => out.push_str(&theme.code.paint(&decode_entities(&inner))),
                    "b" | "strong" => {
                        let styled = render_inline_impl(
                            &decode_entities(&inner),
                            theme,
                            images,
                            false,
                            text_transform,
                            prose_transform,
                        );
                        out.push_str(&theme.bold.paint(&styled));
                    }
                    "i" | "em" => {
                        let styled = render_inline_impl(
                            &decode_entities(&inner),
                            theme,
                            images,
                            false,
                            text_transform,
                            prose_transform,
                        );
                        out.push_str(&theme.italic.paint(&styled));
                    }
                    "hr" | "br" => out.push('\n'),
                    _ => out.push_str(&render_inline_impl(
                        &decode_entities(&inner),
                        theme,
                        images,
                        false,
                        text_transform,
                        prose_transform,
                    )),
                }
                index += next;
                continue;
            }
        }
        if c == '`' {
            if let Some(end) = src[index + 1..].find('`') {
                flush(&mut out, &mut plain, theme, prose_enabled, text_transform, prose_transform);
                let inner = &src[index + 1..index + 1 + end];
                out.push_str(&theme.code.paint(inner));
                index = index + 1 + end + 1;
                continue;
            }
        }
        if c == '\\' && src[index..].starts_with("\\$") {
            plain.push('$');
            index += 2;
            continue;
        }
        if c == '\\' && src[index..].starts_with("\\\\") {
            plain.push('\\');
            index += 2;
            continue;
        }
        if c == '$' || c == '\\' {
            if let Some((rendered, end)) = render_math_at(src, index) {
                flush(&mut out, &mut plain, theme, prose_enabled, text_transform, prose_transform);
                out.push_str(&rendered);
                index = end;
                continue;
            }
        }
        if c == '~' && src[index..].starts_with("~~") && strike_open_ok(src, index) {
            if let Some(end) = find_strike_closer(src, index + 2) {
                flush(&mut out, &mut plain, theme, prose_enabled, text_transform, prose_transform);
                let inner = &src[index + 2..end];
                let styled =
                    render_inline_impl(inner, theme, images, false, text_transform, prose_transform);
                out.push_str(&theme.strikethrough.paint(&styled));
                index = end + 2;
                continue;
            }
        }
        if c == '*' || c == '_' {
            let double = if c == '*' {
                src[index..].starts_with("**")
            } else {
                src[index..].starts_with("__")
            };
            let delim = if double { 2 } else { 1 };
            let marker: String = std::iter::repeat(c).take(delim).collect();
            if let Some(end) = src[index + delim..].find(marker.as_str()) {
                flush(&mut out, &mut plain, theme, prose_enabled, text_transform, prose_transform);
                let inner = &src[index + delim..index + delim + end];
                let styled =
                    render_inline_impl(inner, theme, images, false, text_transform, prose_transform);
                let style = if delim == 2 { theme.bold } else { theme.italic };
                out.push_str(&style.paint(&styled));
                index = index + delim + end + delim;
                continue;
            }
        }
        if c == '!' && src[index..].starts_with("![") {
            if let Some((alt, _url, next)) = parse_link(&src[index + 2..]) {
                flush(&mut out, &mut plain, theme, prose_enabled, text_transform, prose_transform);
                if images {
                    let label = if alt.is_empty() { "image" } else { alt.as_str() };
                    out.push_str(&theme.code.paint(&format!("[{label}]")));
                }
                index = index + 2 + next;
                continue;
            }
        }
        if c == '[' {
            if let Some((text, url, next)) = parse_link(&src[index + 1..]) {
                flush(&mut out, &mut plain, theme, prose_enabled, text_transform, prose_transform);
                let label =
                    render_inline_impl(&text, theme, images, false, text_transform, prose_transform);
                let extension = !url.is_empty()
                    && crate::render::extension_links::extension_link_scheme(&url).is_some();
                let style = if extension {
                    Style::merge(theme.extension_link, theme.underline)
                } else {
                    Style::merge(theme.link, theme.underline)
                };
                out.push_str(&style.paint(&label));
                if !url.is_empty() && text != url && !extension {
                    out.push(' ');
                    out.push_str(&theme.link_url.paint(&format!("({url})")));
                }
                index = index + 1 + next;
                continue;
            }
        }
        plain.push(c);
        index += c.len_utf8();
    }
    flush(&mut out, &mut plain, theme, prose_enabled, text_transform, prose_transform);
    out
}

/// Render inline markdown (styles, links, math) without block layout.
pub fn render_inline_markdown(src: &str, theme: &MarkdownTheme) -> String {
    render_inline(src, theme, true)
}

/// Whether a grown line's start is a block marker, HR, or reference
/// definition — shapes whose cold lex differs from a paragraph splice.
#[must_use]
pub fn fast_line_start_hazard(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    match first {
        '#' | '>' | '|' => true,
        '-' | '+' | '*' => {
            trimmed.starts_with("- ")
                || trimmed.starts_with("+ ")
                || trimmed.starts_with("* ")
                || trimmed.chars().all(|ch| ch == first)
        }
        '=' | '═' | '─' => trimmed.chars().all(|ch| ch == first),
        '[' => trimmed.contains("]: "),
        _ => {
            if !first.is_ascii_digit() {
                return false;
            }
            let rest = trimmed.trim_start_matches(|ch: char| ch.is_ascii_digit());
            rest.starts_with(". ") || rest.starts_with(") ")
        }
    }
}

/// Whether `text` is inert prose: no ANSI, no markdown marker that could pair
/// or complete across a splice seam.
fn inert_prose(text: &str) -> bool {
    !text.contains('\u{1b}')
        && text.chars().all(|ch| {
            ch.is_alphanumeric()
                || ch.is_whitespace()
                || matches!(
                    ch,
                    '.' | ',' | ';' | ':' | '\'' | '"' | '!' | '?' | '(' | ')' | '-'
                )
        })
}

fn scan_links(src: &str, out: &mut Vec<String>) {
    let mut index = 0usize;
    while index < src.len() {
        if src.as_bytes()[index] == 0x1b {
            index += escape_len(&src[index..]);
            continue;
        }
        if src[index..].starts_with("![") {
            if let Some((_alt, _url, next)) = parse_link(&src[index + 2..]) {
                index = index + 2 + next;
                continue;
            }
        }
        if src.as_bytes()[index] == b'[' {
            if let Some((_text, url, next)) = parse_link(&src[index + 1..]) {
                if !url.is_empty() {
                    out.push(url);
                }
                index = index + 1 + next;
                continue;
            }
        }
        let c = match src[index..].chars().next() {
            Some(c) => c,
            None => break,
        };
        index += c.len_utf8();
    }
}

fn collect_links(blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Heading(_, text) | Block::Paragraph(text) => scan_links(text, out),
            Block::Code { .. } => {}
            Block::List { items, .. } => {
                for item in items {
                    scan_links(&item.text, out);
                }
            }
            Block::Quote(inner) => collect_links(inner, out),
            Block::Table { header, rows, .. } => {
                for cell in header {
                    scan_links(cell, out);
                }
                for row in rows {
                    for cell in row {
                        scan_links(cell, out);
                    }
                }
            }
            Block::Hr => {}
            Block::Math(_) => {}
        }
    }
}

fn strip_images(line: &str) -> String {
    let mut out = String::new();
    let mut index = 0usize;
    while index < line.len() {
        if line[index..].starts_with("![") {
            if let Some((_alt, _url, next)) = parse_link(&line[index + 2..]) {
                index = index + 2 + next;
                continue;
            }
        }
        let c = match line[index..].chars().next() {
            Some(c) => c,
            None => break,
        };
        out.push(c);
        index += c.len_utf8();
    }
    out
}

fn longest_word(text: &str) -> usize {
    strip_ansi(text)
        .split_whitespace()
        .map(visible_width)
        .max()
        .unwrap_or(0)
        .min(30)
}

fn align_cell(text: &str, width: usize, align: Align) -> String {
    let used = visible_width(text);
    let pad = width.saturating_sub(used);
    match align {
        Align::Left => format!("{text}{}", " ".repeat(pad)),
        Align::Right => format!("{}{text}", " ".repeat(pad)),
        Align::Center => {
            let left = pad / 2;
            let right = pad - left;
            format!("{}{text}{}", " ".repeat(left), " ".repeat(right))
        }
    }
}

fn push_wrapped(out: &mut Vec<String>, text: &str, width: usize) {
    for line in wrap_text_with_ansi(text, width) {
        out.push(line);
    }
}

// ---------------------------------------------------------------------------
// Tree-guide hanging wrap (port of l/src/components/markdown.ts:836-1008).
// ---------------------------------------------------------------------------

const MIN_TREE_CONTENT_WIDTH: usize = 8;

/// Continuation glyph for a tree-guide character.
fn tree_guide_continuation(ch: char) -> Option<char> {
    match ch {
        '│' | '┃' | '║' => Some(ch),
        '├' => Some('│'),
        '┣' => Some('┃'),
        '╠' => Some('║'),
        '└' | '┗' | '╚' | '╰' => Some(' '),
        '─' | '━' | '═' | ' ' => Some(' '),
        _ => None,
    }
}

fn is_tree_guide_anchor(ch: char) -> bool {
    matches!(ch, '│' | '┃' | '║' | '├' | '┣' | '╠' | '└' | '┗' | '╚' | '╰')
}

fn is_tree_branch(ch: char) -> bool {
    matches!(ch, '├' | '┣' | '╠' | '└' | '┗' | '╚' | '╰')
}

fn is_tree_connector(ch: char) -> bool {
    matches!(ch, '─' | '━' | '═')
}

struct TreeGuidePrefix {
    /// Byte offset just past the guide prefix.
    end: usize,
    /// SGR sequences seen before/between the guides.
    codes: String,
    /// Closer-only SGR sequences left pending after the prefix.
    closers: String,
    /// The visible guide characters.
    guides: String,
}

/// Length of the SGR sequence at the start of `input`, if any.
fn sgr_len(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    if bytes.first() != Some(&0x1b) || bytes.get(1) != Some(&b'[') {
        return None;
    }
    for (offset, byte) in bytes.iter().enumerate().skip(2) {
        if (0x40..=0x7e).contains(byte) {
            return Some(offset + 1);
        }
    }
    None
}

fn is_sgr_closer(seq: &str) -> bool {
    let Some(params) = seq.strip_prefix("\x1b[").and_then(|rest| rest.strip_suffix('m')) else {
        return false;
    };
    params.is_empty()
        || params.split(';').all(|part| {
            matches!(part, "" | "0" | "22" | "23" | "24" | "25" | "26" | "27" | "28" | "29" | "39" | "49" | "59")
        })
}

/// Match `^(?:\x1b\[[0-9;:]*m)*[` + guide chars + `]+` and require a branch/connector pair.
fn match_tree_guide_prefix(line: &str) -> Option<TreeGuidePrefix> {
    let mut codes = String::new();
    let mut pending = String::new();
    let mut guides = String::new();
    let mut end = 0usize;
    let mut index = 0usize;
    while index < line.len() {
        if line.as_bytes()[index] == 0x1b {
            match sgr_len(&line[index..]) {
                Some(len) => {
                    pending.push_str(&line[index..index + len]);
                    index += len;
                }
                None => break,
            }
            continue;
        }
        let Some(ch) = line[index..].chars().next() else {
            break;
        };
        if tree_guide_continuation(ch).is_none() {
            break;
        }
        guides.push(ch);
        codes.push_str(&pending);
        pending.clear();
        index += ch.len_utf8();
        end = index;
    }
    if index >= line.len() {
        return None;
    }
    let mut branch_connector = false;
    let mut previous: Option<char> = None;
    for ch in guides.chars() {
        if let Some(prev) = previous {
            if is_tree_branch(prev) && is_tree_connector(ch) {
                branch_connector = true;
            }
        }
        previous = Some(ch);
    }
    if !branch_connector {
        return None;
    }
    let mut closers = String::new();
    let mut rest = pending.as_str();
    while !rest.is_empty() {
        if rest.starts_with("\x1b[") {
            if let Some(len) = sgr_len(rest) {
                let seq = &rest[..len];
                if is_sgr_closer(seq) {
                    closers.push_str(seq);
                }
                rest = &rest[len..];
                continue;
            }
        }
        rest = &rest[1..];
    }
    Some(TreeGuidePrefix {
        end,
        codes,
        closers,
        guides,
    })
}

/// Drop every SGR except what follows the last bare reset (`\x1b[m`/`\x1b[0m`).
fn compact_sgr_carry(carry: &str) -> String {
    let short = carry.rfind("\x1b[m").map(|at| at + 3);
    let long = carry.rfind("\x1b[0m").map(|at| at + 4);
    match short.max(long) {
        Some(cut) => carry[cut..].to_string(),
        None => carry.to_string(),
    }
}

/// Concatenate every SGR sequence in `text`.
fn sgr_sequences(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("\x1b[") {
        let tail = &rest[at..];
        match sgr_len(tail) {
            Some(len) => {
                out.push_str(&tail[..len]);
                rest = &tail[len..];
            }
            None => rest = &tail[1..],
        }
    }
    out
}

/// Wrap `text` under its tree-guide prefix; returns `None` when no physical
/// line needs hanging and the caller should use the plain wrapper.
fn hang_wrap_tree_guide_lines(text: &str, width: usize) -> Option<Vec<String>> {
    if width < MIN_TREE_CONTENT_WIDTH || !text.chars().any(is_tree_guide_anchor) {
        return None;
    }
    let source_lines: Vec<&str> = text.split('\n').collect();
    let hangs = |line: &str| -> Option<TreeGuidePrefix> {
        if visible_width(line) <= width {
            return None;
        }
        let prefix = match_tree_guide_prefix(line)?;
        if width.saturating_sub(visible_width(&prefix.guides)) < MIN_TREE_CONTENT_WIDTH {
            return None;
        }
        Some(prefix)
    };
    if !source_lines.iter().any(|line| hangs(line).is_some()) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut carry = String::new();
    for line in source_lines {
        let prefix = match hangs(line) {
            Some(prefix) => prefix,
            None => {
                if carry.is_empty() {
                    out.push(line.to_string());
                } else {
                    out.push(format!("{carry}{line}"));
                }
                carry = compact_sgr_carry(&format!("{carry}{}", sgr_sequences(line)));
                continue;
            }
        };
        let active_codes = format!("{carry}{}", prefix.codes);
        let node = format!("{active_codes}{}", &line[prefix.end..]);
        let rows = wrap_text_with_ansi(&node, width - visible_width(&prefix.guides));
        let mut hang = String::new();
        for guide in prefix.guides.chars() {
            hang.push(tree_guide_continuation(guide).unwrap_or(' '));
        }
        let shortfall = visible_width(&prefix.guides).saturating_sub(visible_width(&hang));
        hang.push_str(&" ".repeat(shortfall));
        let first_rest = rows[0].strip_prefix(&active_codes).unwrap_or(&rows[0]);
        out.push(format!("{carry}{}{first_rest}", &line[..prefix.end]));
        let rail = format!("{active_codes}{hang}{}", prefix.closers);
        for row in rows.iter().skip(1) {
            out.push(format!("{rail}{row}"));
        }
        carry = compact_sgr_carry(&format!("{carry}{}", sgr_sequences(line)));
    }
    Some(out)
}

impl Markdown {
    /// Create an empty renderer with the default theme and no padding.
    pub fn new() -> Self {
        Markdown {
            text: String::new(),
            padding_x: 0,
            padding_y: 0,
            theme: MarkdownTheme::default(),
            images_visible: true,
            table_charts_visible: true,
            tool_result_images_visible: true,
            on_link_click: None,
            prose_transform: None,
            text_transform: None,
            transient_render_cache: false,
            last_text: String::new(),
            last_rows: Vec::new(),
            last_width: 0,
            fast_hits: 0,
            render_generation: 0,
            last_generation: 0,
            cache: None,
        }
    }

    /// Replace the source text; re-renders on the next [`Component::render`].
    pub fn set_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text != self.text {
            self.text = text;
            self.invalidate();
        }
    }

    /// The unrendered source text.
    pub fn get_text(&self) -> &str {
        &self.text
    }

    /// Show or hide inline `![alt](url)` images.
    pub fn set_images_visible(&mut self, visible: bool) {
        self.images_visible = visible;
        self.bump_generation();
    }

    /// Whether inline images are shown.
    pub fn images_visible(&self) -> bool {
        self.images_visible
    }

    /// Show or hide fenced `chart`/`mermaid` blocks.
    pub fn set_table_charts_visible(&mut self, visible: bool) {
        self.table_charts_visible = visible;
        self.bump_generation();
    }

    /// Whether fenced chart blocks are shown.
    pub fn table_charts_visible(&self) -> bool {
        self.table_charts_visible
    }

    /// Show or hide image placeholders inside `tool_result` code blocks.
    pub fn set_tool_result_images_visible(&mut self, visible: bool) {
        self.tool_result_images_visible = visible;
        self.bump_generation();
    }

    /// Whether `tool_result` image placeholders are shown.
    pub fn tool_result_images_visible(&self) -> bool {
        self.tool_result_images_visible
    }

    /// Fire `callback` with a link destination when the host activates it.
    pub fn set_on_link_click(&mut self, callback: impl FnMut(&str) + 'static) {
        self.on_link_click = Some(Box::new(callback));
    }

    /// Replace the theme.
    pub fn set_theme(&mut self, theme: MarkdownTheme) {
        self.theme = theme;
        self.bump_generation();
    }

    /// Paint every plain run through `transform`, including md-owned spans
    /// (live-command output contract). Wins over the paragraph prose paint.
    pub fn set_text_transform(&mut self, transform: Option<Box<dyn Fn(&str) -> String>>) {
        self.text_transform = transform;
        self.bump_generation();
    }

    /// Paint plain paragraph runs through `transform` (`proseFg` token
    /// contract): headings and links keep their own `md*` tokens.
    pub fn set_prose_transform(&mut self, transform: Option<Box<dyn Fn(&str) -> String>>) {
        self.prose_transform = transform;
        self.bump_generation();
    }

    /// Enable the transient (streaming) render cache. While enabled, an
    /// append-only prose delta re-wraps only the grown tail row; any hazard
    /// (marker, style, newline, width change) falls back to a cold re-lex.
    pub fn set_transient_render_cache(&mut self, enabled: bool) {
        if self.transient_render_cache == enabled {
            return;
        }
        self.transient_render_cache = enabled;
        self.last_text.clear();
        self.last_rows.clear();
        self.invalidate();
    }

    /// How many renders took the fast tail splice.
    #[must_use]
    pub fn fast_path_hits(&self) -> usize {
        self.fast_hits
    }

    /// The active theme.
    pub fn theme(&self) -> &MarkdownTheme {
        &self.theme
    }

    /// Set horizontal and vertical padding.
    pub fn set_padding(&mut self, x: u16, y: u16) {
        self.padding_x = x;
        self.padding_y = y;
        self.bump_generation();
    }

    /// Set the code block body indent.
    pub fn set_code_block_indent(&mut self, indent: usize) {
        self.theme.code_block_indent = indent;
        self.bump_generation();
    }

    /// Every link destination in the document, in reading order.
    pub fn links(&self) -> Vec<String> {
        let lines: Vec<String> = self.text.split('\n').map(str::to_string).collect();
        let blocks = parse_blocks(&lines);
        let mut urls = Vec::new();
        collect_links(&blocks, &mut urls);
        urls
    }

    /// Activate a link destination through the registered callback.
    pub fn activate_link(&mut self, url: &str) {
        if let Some(callback) = self.on_link_click.as_mut() {
            callback(url);
        }
    }

    /// Drop the render cache; the next render rebuilds from state.
    pub fn invalidate(&mut self) {
        self.cache = None;
    }

    /// Invalidate the cache and mark the configuration changed, so a transient
    /// tail splice cannot reuse rows rendered under the previous inputs.
    fn bump_generation(&mut self) {
        self.render_generation = self.render_generation.wrapping_add(1);
        self.invalidate();
    }

    fn render_blocks(&self, blocks: &[Block], width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let total = blocks.len();
        for (index, block) in blocks.iter().enumerate() {
            self.render_block(block, width, &mut lines);
            if index + 1 < total && !matches!(block, Block::List { .. }) {
                lines.push(String::new());
            }
        }
        lines
    }

    fn inline(&self, src: &str, images: bool, prose_enabled: bool) -> String {
        render_inline_impl(
            src,
            &self.theme,
            images,
            prose_enabled,
            self.text_transform.as_deref(),
            self.prose_transform.as_deref(),
        )
    }

    fn render_block(&self, block: &Block, width: usize, out: &mut Vec<String>) {
        match block {
            Block::Heading(depth, text) => {
                let inline = self.inline(text, self.images_visible, false);
                // A sized H1 draws its plain text, so inline styling (math color)
                // does not change the emitted bytes.
                if *depth == 1 && terminal().text_sizing {
                    let plain = strip_ansi(&inline);
                    let plain_width = visible_width(&plain);
                    if plain_width > 0 && plain_width * 2 <= width {
                        let style = Style::merge(
                            Style::merge(self.theme.heading, self.theme.bold),
                            self.theme.underline,
                        );
                        out.push(style.paint(&plain));
                        return;
                    }
                }
                let style = match depth {
                    1 => Style::merge(
                        Style::merge(self.theme.heading, self.theme.bold),
                        self.theme.underline,
                    ),
                    _ => Style::merge(self.theme.heading, self.theme.bold),
                };
                let painted = if *depth >= 3 {
                    style.paint(&format!("{} {}", "#".repeat(*depth as usize), inline))
                } else {
                    style.paint(&inline)
                };
                push_wrapped(out, &painted, width);
            }
            Block::Paragraph(text) => {
                let inline = self.inline(text, self.images_visible, true);
                match hang_wrap_tree_guide_lines(&inline, width) {
                    Some(rows) => out.extend(rows),
                    None => push_wrapped(out, &inline, width),
                }
            }
            Block::Math(body) => {
                for line in latex_to_block(body) {
                    out.push(truncate_to_width(
                        &self.theme.text.paint(&line),
                        width,
                        Ellipsis::Omit,
                    ));
                }
            }
            Block::Code { lang, body } => {
                if (lang == "chart" || lang == "mermaid") && !self.table_charts_visible {
                    return;
                }
                out.push(truncate_to_width(
                    &self.theme.code_block_border.paint(&format!("```{lang}")),
                    width,
                    Ellipsis::Omit,
                ));
                let indent = " ".repeat(self.theme.code_block_indent.min(width));
                for line in body.split('\n') {
                    let line = if lang == "tool_result" && !self.tool_result_images_visible {
                        strip_images(line)
                    } else {
                        line.to_string()
                    };
                    let styled = self.theme.code_block.paint(&line);
                    push_wrapped(out, &format!("{indent}{styled}"), width);
                }
                out.push(truncate_to_width(
                    &self.theme.code_block_border.paint("```"),
                    width,
                    Ellipsis::Omit,
                ));
            }
            Block::List {
                ordered,
                start,
                items,
            } => {
                self.render_list(*ordered, *start, items, width, out);
            }
            Block::Quote(inner) => {
                let inner_width = width.saturating_sub(2).max(1);
                for line in self.render_blocks(inner, inner_width) {
                    let styled = self.theme.quote.paint(&line);
                    out.push(format!(
                        "{} {}",
                        self.theme.quote_border.paint(&self.theme.symbols.quote_border),
                        styled
                    ));
                }
            }
            Block::Table {
                header,
                aligns,
                rows,
            } => {
                self.render_table(header, aligns, rows, width, out);
            }
            Block::Hr => {
                let fill: String = std::iter::repeat(self.theme.symbols.hr_char)
                    .take(width.min(80))
                    .collect();
                out.push(truncate_to_width(&self.theme.hr.paint(&fill), width, Ellipsis::Omit));
            }
        }
    }

    fn render_list(
        &self,
        ordered: bool,
        start: usize,
        items: &[ListItem],
        width: usize,
        out: &mut Vec<String>,
    ) {
        for (index, item) in items.iter().enumerate() {
            let marker = if ordered {
                format!("{}. ", start + index)
            } else {
                "- ".to_string()
            };
            let task = match item.checked {
                Some(true) => "[x] ",
                Some(false) => "[ ] ",
                None => "",
            };
            let bullet = format!("{marker}{task}");
            let mut first_prefix = self.theme.list_bullet.paint(&marker);
            if !task.is_empty() {
                first_prefix.push_str(&self.theme.text.paint(task));
            }
            let continuation = " ".repeat(visible_width(&bullet));
            let inline = self.inline(&item.text, self.images_visible, true);
            let prefix_width = visible_width(&first_prefix);
            let body_width = width.saturating_sub(prefix_width).max(1);
            let wrapped = wrap_text_with_ansi(&inline, body_width);
            if wrapped.is_empty() {
                out.push(first_prefix);
                continue;
            }
            out.push(format!("{}{}", first_prefix, wrapped[0]));
            for line in &wrapped[1..] {
                out.push(format!("{}{}", continuation, line));
            }
        }
    }

    fn render_table(
        &self,
        header: &[String],
        aligns: &[Align],
        rows: &[Vec<String>],
        width: usize,
        out: &mut Vec<String>,
    ) {
        let columns = header.len();
        if columns == 0 {
            return;
        }
        let overhead = 3 * columns + 1;
        let available = width.saturating_sub(overhead);
        if available < columns {
            let mut fallback = vec![header.join(" | ")];
            for row in rows {
                fallback.push(row.join(" | "));
            }
            for line in fallback {
                push_wrapped(out, &line, width);
            }
            return;
        }

        let mut natural = vec![0usize; columns];
        let mut min_word = vec![1usize; columns];
        for (index, cell) in header.iter().enumerate() {
            let styled = render_inline(cell, &self.theme, self.images_visible);
            natural[index] = visible_width(&styled).max(natural[index]);
            min_word[index] = longest_word(&styled).max(1);
        }
        for row in rows {
            for (index, cell) in row.iter().enumerate().take(columns) {
                let styled = render_inline(cell, &self.theme, self.images_visible);
                natural[index] = visible_width(&styled).max(natural[index]);
                min_word[index] = longest_word(&styled).max(1);
            }
        }

        let min_total: usize = min_word.iter().sum();
        let widths: Vec<usize> = if min_total > available {
            let remaining = available - columns;
            let total_weight: usize = min_word.iter().map(|w| w.saturating_sub(1)).sum();
            let mut widths = vec![1usize; columns];
            let mut allocated = 0usize;
            if total_weight > 0 {
                for index in 0..columns {
                    let weight = min_word[index].saturating_sub(1);
                    let grow = weight * remaining / total_weight;
                    widths[index] += grow;
                    allocated += grow;
                }
            }
            let mut leftover = remaining - allocated;
            let mut index = 0usize;
            while leftover > 0 {
                widths[index % columns] += 1;
                leftover -= 1;
                index += 1;
            }
            widths
        } else if natural.iter().sum::<usize>() <= available {
            (0..columns)
                .map(|index| natural[index].max(min_word[index]))
                .collect()
        } else {
            let extra = available - min_total;
            let potential: usize = (0..columns)
                .map(|index| natural[index].saturating_sub(min_word[index]))
                .sum();
            let mut widths = vec![0usize; columns];
            let mut allocated = 0usize;
            for index in 0..columns {
                let delta = natural[index].saturating_sub(min_word[index]);
                let grow = if potential > 0 {
                    delta * extra / potential
                } else {
                    0
                };
                widths[index] = min_word[index] + grow;
                allocated += widths[index];
            }
            let mut remaining = available - allocated;
            while remaining > 0 {
                let mut grew = false;
                for index in 0..columns {
                    if remaining == 0 {
                        break;
                    }
                    if widths[index] < natural[index] {
                        widths[index] += 1;
                        remaining -= 1;
                        grew = true;
                    }
                }
                if !grew {
                    break;
                }
            }
            widths
        };

        let symbols = &self.theme.symbols.table;
        let horizontal = &symbols.horizontal;
        let vertical = &symbols.vertical;
        let repeats: Vec<String> = widths.iter().map(|w| horizontal.repeat(*w)).collect();
        out.push(format!(
            "{}{}{}{}{}",
            symbols.top_left,
            horizontal,
            repeats.join(&format!("{horizontal}{}{horizontal}", symbols.tee_down)),
            horizontal,
            symbols.top_right
        ));

        let mut header_cells: Vec<Vec<String>> = Vec::new();
        for (index, cell) in header.iter().enumerate() {
            let styled = render_inline(cell, &self.theme, self.images_visible);
            header_cells.push(wrap_text_with_ansi(&styled, widths[index]));
        }
        let header_height = header_cells.iter().map(Vec::len).max().unwrap_or(0).max(1);
        for line_index in 0..header_height {
            let mut parts = Vec::new();
            for column in 0..columns {
                let text = header_cells[column]
                    .get(line_index)
                    .cloned()
                    .unwrap_or_default();
                let align = aligns.get(column).copied().unwrap_or(Align::Left);
                let padded = align_cell(&text, widths[column], align);
                parts.push(self.theme.table_header.paint(&padded));
            }
            out.push(format!("{vertical} {} {vertical}", parts.join(&format!(" {vertical} "))));
        }

        let separator = format!(
            "{}{}{}{}{}",
            symbols.tee_right,
            horizontal,
            repeats.join(&format!("{horizontal}{}{horizontal}", symbols.cross)),
            horizontal,
            symbols.tee_left
        );
        out.push(separator.clone());

        for (row_index, row) in rows.iter().enumerate() {
            let mut cells: Vec<Vec<String>> = Vec::new();
            for column in 0..columns {
                let text = row.get(column).cloned().unwrap_or_default();
                let styled = render_inline(&text, &self.theme, self.images_visible);
                cells.push(wrap_text_with_ansi(&styled, widths[column]));
            }
            let height = cells.iter().map(Vec::len).max().unwrap_or(0).max(1);
            for line_index in 0..height {
                let mut parts = Vec::new();
                for column in 0..columns {
                    let text = cells[column].get(line_index).cloned().unwrap_or_default();
                    let align = aligns.get(column).copied().unwrap_or(Align::Left);
                    parts.push(align_cell(&text, widths[column], align));
                }
                out.push(format!(
                    "{vertical} {} {vertical}",
                    parts.join(&format!(" {vertical} "))
                ));
            }
            if row_index + 1 < rows.len() {
                out.push(separator.clone());
            }
        }

        out.push(format!(
            "{}{}{}{}{}",
            symbols.bottom_left,
            horizontal,
            repeats.join(&format!("{horizontal}{}{horizontal}", symbols.tee_up)),
            horizontal,
            symbols.bottom_right
        ));
    }

    /// Re-wrap only the grown tail row for an append-only inert prose delta.
    /// Returns `None` for every hazard (style, marker, newline, width change),
    /// so the caller falls back to a cold re-lex.
    fn fast_tail_rows(&mut self, width: usize) -> Option<Vec<String>> {
        if !self.transient_render_cache
            || self.padding_x > 0
            || self.padding_y > 0
            || self.text_transform.is_some()
            || self.prose_transform.is_some()
            || self.theme.prose.is_some()
            || self.last_rows.is_empty()
            || self.last_width as usize != width
            || !self.text.starts_with(&self.last_text)
            || self.render_generation != self.last_generation
        {
            return None;
        }
        let delta = &self.text[self.last_text.len()..];
        if delta.is_empty() || delta.contains('\n') || delta.contains('\r') {
            return None;
        }
        if !inert_prose(&self.last_text) || !inert_prose(delta) {
            return None;
        }
        let last_row = self.last_rows.last()?;
        let last_chars = last_row.trim_end_matches(' ').chars().count();
        let prev_chars = self.last_text.chars().count();
        let src_start = prev_chars.checked_sub(last_chars)?;
        let src_byte = self
            .last_text
            .char_indices()
            .nth(src_start)
            .map(|(index, _)| index)
            .unwrap_or(self.last_text.len());
        let tail = self.text.get(src_byte..)?;
        let at_line_start =
            src_byte == 0 || self.text.as_bytes().get(src_byte - 1) == Some(&b'\n');
        if at_line_start {
            let first_line = tail.split('\n').next().unwrap_or(tail);
            if fast_line_start_hazard(first_line) {
                return None;
            }
        }
        let mut rows = self.last_rows.clone();
        rows.pop();
        for line in wrap_text_with_ansi(tail, width) {
            rows.push(pad_to_width(&truncate_to_width(&line, width, Ellipsis::Omit), width));
        }
        self.fast_hits += 1;
        self.last_text = self.text.clone();
        self.last_rows = rows.clone();
        self.last_generation = self.render_generation;
        Some(rows)
    }
}

impl Component for Markdown {
    fn render(&mut self, width: u16) -> Vec<String> {
        let width = width.max(1) as usize;
        if let Some((cached_text, cached_width, rows)) = &self.cache {
            if *cached_text == self.text && *cached_width == width {
                return rows.clone();
            }
        }
        if self.text.trim().is_empty() {
            self.cache = Some((self.text.clone(), width, Vec::new()));
            return Vec::new();
        }
        if let Some(rows) = self.fast_tail_rows(width) {
            self.cache = Some((self.text.clone(), width, rows.clone()));
            return rows;
        }
        let padding_x = (self.padding_x as usize).min(width / 2);
        let content_width = width.saturating_sub(2 * padding_x).max(1);
        let lines: Vec<String> = self.text.split('\n').map(str::to_string).collect();
        let blocks = parse_blocks(&lines);
        let content = self.render_blocks(&blocks, content_width);

        let mut rows: Vec<String> = Vec::new();
        for _ in 0..self.padding_y {
            rows.push(pad_to_width("", width));
        }
        for line in content {
            let line = truncate_to_width(&line, content_width, Ellipsis::Omit);
            let mut row = String::new();
            row.push_str(&" ".repeat(padding_x));
            row.push_str(&line);
            row.push_str(&" ".repeat(padding_x));
            rows.push(pad_to_width(&row, width));
        }
        for _ in 0..self.padding_y {
            rows.push(pad_to_width("", width));
        }
        self.last_text = self.text.clone();
        self.last_rows = rows.clone();
        self.last_width = width as u16;
        self.last_generation = self.render_generation;
        self.cache = Some((self.text.clone(), width, rows.clone()));
        rows
    }

    fn invalidate(&mut self) {
        self.cache = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn renders_headings_lists_and_inline_styles() {
        let mut markdown = Markdown::new();
        markdown.set_text(
            "# Title\n\nSome **bold** and *italic* and ~~gone~~ and `code`.\n\n- one\n- two\n\n1. first\n2. second\n\n- [x] done\n- [ ] todo",
        );
        let rows = markdown.render(40);
        let joined = rows.join("\n");
        assert!(joined.contains("Title"));
        assert!(joined.contains("bold"));
        assert!(joined.contains("italic"));
        assert!(joined.contains("gone"));
        assert!(joined.contains("code"));
        assert!(joined.contains("one"));
        assert!(joined.contains("two"));
        assert!(joined.contains("first"));
        assert!(joined.contains("second"));
        assert!(joined.contains("[x] done"));
        assert!(joined.contains("[ ] todo"));
        assert!(rows.iter().all(|row| visible_width(row) <= 40));
    }

    #[test]
    fn renders_fenced_code_with_language_label() {
        let mut markdown = Markdown::new();
        markdown.set_text("```rust\nfn main() {}\n```");
        let joined = markdown.render(30).join("\n");
        assert!(joined.contains("```rust"));
        assert!(joined.contains("fn main() {}"));
        assert!(joined.contains("```"));
    }

    #[test]
    fn renders_table_rows_with_borders() {
        let mut markdown = Markdown::new();
        markdown.set_text("| a | b |\n|---|---|\n| 1 | 2 |");
        let rows = markdown.render(30);
        let joined = rows.join("\n");
        assert!(joined.contains('┌'));
        assert!(joined.contains('┼'));
        assert!(joined.contains('└'));
        assert!(joined.contains("a"));
        assert!(joined.contains("b"));
        assert!(joined.contains("1"));
        assert!(joined.contains("2"));
        assert!(rows.iter().all(|row| visible_width(row) <= 30));
    }

    #[test]
    fn renders_quotes_rules_and_links() {
        let mut markdown = Markdown::new();
        markdown.set_text("> quoted\n\n---\n\n[site](https://example.com)");
        let joined = markdown.render(30).join("\n");
        assert!(joined.contains("quoted"));
        assert!(joined.contains('─'));
        assert!(joined.contains("site"));
        assert!(joined.contains("https://example.com"));
    }

    #[test]
    fn empty_text_renders_no_rows() {
        let mut markdown = Markdown::new();
        assert!(markdown.render(20).is_empty());
        markdown.set_text("   \n  ");
        assert!(markdown.render(20).is_empty());
    }

    #[test]
    fn narrow_widths_never_exceed_or_panic() {
        let mut markdown = Markdown::new();
        markdown.set_text("hello world");
        for width in [0u16, 1, 2, 3, 5, 10] {
            let rows = markdown.render(width);
            assert!(!rows.is_empty());
            assert!(rows.iter().all(|row| visible_width(row) <= width.max(1).into()));
        }
    }

    #[test]
    fn preserves_ansi_while_wrapping() {
        let mut markdown = Markdown::new();
        markdown.set_text("\x1b[31mred\x1b[0m plain");
        let rows = markdown.render(20);
        let joined = rows.join("\n");
        assert!(joined.contains("red"));
        assert!(joined.contains("\x1b[31m"));
        assert!(joined.contains("plain"));
    }

    #[test]
    fn images_toggle_and_cache_invalidation() {
        let mut markdown = Markdown::new();
        markdown.set_text("![alt](https://example.com/x.png)");
        assert!(markdown.render(40).join("\n").contains("alt"));
        markdown.set_images_visible(false);
        assert!(!markdown.render(40).join("\n").contains("alt"));

        markdown.set_images_visible(true);
        markdown.set_text("one");
        assert!(markdown.render(20).join("\n").contains("one"));
        markdown.set_text("two");
        let rows = markdown.render(20);
        assert!(rows.join("\n").contains("two"));
        assert!(!rows.join("\n").contains("one"));
    }

    #[test]
    fn collects_and_activates_links() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        let mut markdown = Markdown::new();
        markdown.set_on_link_click(move |url| sink.borrow_mut().push(url.to_string()));
        markdown.set_text("[a](https://a.example) and [b](https://b.example)");
        assert_eq!(
            markdown.links(),
            vec![
                "https://a.example".to_string(),
                "https://b.example".to_string()
            ]
        );
        markdown.activate_link("https://a.example");
        assert_eq!(seen.borrow().as_slice(), &["https://a.example".to_string()][..]);
    }

    #[test]
    fn chart_blocks_follow_visibility_flag() {
        let mut markdown = Markdown::new();
        markdown.set_text("```mermaid\ngraph TD\n```");
        assert!(markdown.render(30).join("\n").contains("graph TD"));
        markdown.set_table_charts_visible(false);
        assert!(!markdown.render(30).join("\n").contains("graph TD"));
    }

    #[test]
    fn tool_result_images_follow_visibility_flag() {
        let mut markdown = Markdown::new();
        markdown.set_text("```tool_result\n![shot](https://x)\n```");
        assert!(markdown.render(40).join("\n").contains("shot"));
        markdown.set_tool_result_images_visible(false);
        assert!(!markdown.render(40).join("\n").contains("shot"));
    }
}
