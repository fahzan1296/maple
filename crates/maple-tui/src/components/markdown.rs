//! Markdown renderer: headings, lists, tables, code fences, quotes, rules, and
//! inline styles, wrapped to the viewport width.
//! Reference: `l/src/components/markdown.ts`.
//!
//! Bounded clean-room port of the upstream `Markdown` component: no streaming
//! cache, no syntax highlighter, no HTML block layout. Rendered rows preserve
//! ANSI styling and never exceed the requested width.

use crate::ansi_scan::strip_ansi;
use crate::component::Component;
use crate::style::{Color, Style};
use crate::symbols::SymbolTheme;
use crate::text_shape::{pad_to_width, truncate_to_width, visible_width, wrap_text_with_ansi, Ellipsis};

/// Colors and glyphs for a [`Markdown`] renderer.
#[derive(Clone, PartialEq)]
pub struct MarkdownTheme {
    /// Heading color; weight is layered on top by the renderer.
    pub heading: Style,
    /// Body text.
    pub text: Style,
    /// Link label.
    pub link: Style,
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
            link: Style::fg(Color::LightBlue),
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
        let mut paragraph = String::new();
        while index < lines.len() {
            let candidate = lines[index].trim_start();
            if candidate.is_empty() || is_block_start(candidate) {
                break;
            }
            if candidate.contains('|')
                && index + 1 < lines.len()
                && is_table_delimiter(lines[index + 1].trim())
            {
                break;
            }
            if !paragraph.is_empty() {
                paragraph.push(' ');
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
    let mut out = String::new();
    let mut index = 0usize;
    while index < src.len() {
        if src.as_bytes()[index] == 0x1b {
            let len = escape_len(&src[index..]);
            out.push_str(&src[index..index + len]);
            index += len;
            continue;
        }
        let c = match src[index..].chars().next() {
            Some(c) => c,
            None => break,
        };
        if c == '`' {
            if let Some(end) = src[index + 1..].find('`') {
                let inner = &src[index + 1..index + 1 + end];
                out.push_str(&theme.code.paint(inner));
                index = index + 1 + end + 1;
                continue;
            }
        }
        if c == '~' && src[index..].starts_with("~~") {
            if let Some(end) = src[index + 2..].find("~~") {
                let inner = &src[index + 2..index + 2 + end];
                let styled = render_inline(inner, theme, images);
                out.push_str(&theme.strikethrough.paint(&styled));
                index = index + 2 + end + 2;
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
                let inner = &src[index + delim..index + delim + end];
                let styled = render_inline(inner, theme, images);
                let style = if delim == 2 { theme.bold } else { theme.italic };
                out.push_str(&style.paint(&styled));
                index = index + delim + end + delim;
                continue;
            }
        }
        if c == '!' && src[index..].starts_with("![") {
            if let Some((alt, _url, next)) = parse_link(&src[index + 2..]) {
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
                let label = render_inline(&text, theme, images);
                let styled = Style::merge(theme.link, theme.underline).paint(&label);
                out.push_str(&styled);
                if !url.is_empty() && text != url {
                    out.push(' ');
                    out.push_str(&theme.link_url.paint(&format!("({url})")));
                }
                index = index + 1 + next;
                continue;
            }
        }
        out.push(c);
        index += c.len_utf8();
    }
    out
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
        self.invalidate();
    }

    /// Whether inline images are shown.
    pub fn images_visible(&self) -> bool {
        self.images_visible
    }

    /// Show or hide fenced `chart`/`mermaid` blocks.
    pub fn set_table_charts_visible(&mut self, visible: bool) {
        self.table_charts_visible = visible;
        self.invalidate();
    }

    /// Whether fenced chart blocks are shown.
    pub fn table_charts_visible(&self) -> bool {
        self.table_charts_visible
    }

    /// Show or hide image placeholders inside `tool_result` code blocks.
    pub fn set_tool_result_images_visible(&mut self, visible: bool) {
        self.tool_result_images_visible = visible;
        self.invalidate();
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
        self.invalidate();
    }

    /// The active theme.
    pub fn theme(&self) -> &MarkdownTheme {
        &self.theme
    }

    /// Set horizontal and vertical padding.
    pub fn set_padding(&mut self, x: u16, y: u16) {
        self.padding_x = x;
        self.padding_y = y;
        self.invalidate();
    }

    /// Set the code block body indent.
    pub fn set_code_block_indent(&mut self, indent: usize) {
        self.theme.code_block_indent = indent;
        self.invalidate();
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

    fn render_block(&self, block: &Block, width: usize, out: &mut Vec<String>) {
        match block {
            Block::Heading(depth, text) => {
                let inline = render_inline(text, &self.theme, self.images_visible);
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
                let inline = render_inline(text, &self.theme, self.images_visible);
                push_wrapped(out, &inline, width);
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
            let inline = render_inline(&item.text, &self.theme, self.images_visible);
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
