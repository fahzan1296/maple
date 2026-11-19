//! JavaScript/TypeScript display formatting.
//!
//! Ported from `l/src/tools/eval-format/javascript.ts`. The formatter is a
//! prefix-tolerant tokenizer: it never requires valid syntax, never changes
//! literals, and keeps already committed lines stable as more source streams in.

const CONTROL_HEADER_WORDS: &[&str] = &["catch", "for", "if", "switch", "while", "with"];
const REGEX_PREFIX_WORDS: &[&str] = &[
    "await", "case", "delete", "do", "else", "extends", "in", "instanceof", "new", "of", "return",
    "throw", "typeof", "void", "yield",
];
const CLOSE_CONTINUATIONS: &[&str] = &["else", "catch", "finally"];
const BLOCK_BRACE_WORDS: &[&str] = &[
    "catch", "class", "do", "else", "finally", "for", "function", "if", "switch", "try", "while",
    "with",
];
const OBJECT_PREFIX_WORDS: &[&str] = &["const", "default", "let", "return", "throw", "var", "yield"];

const JS_DISPLAY_MAX_LINE_WIDTH: usize = 100;
const JS_DISPLAY_INDENT: &str = "    ";

fn word_in(table: &[&str], word: &str) -> bool {
    table.iter().any(|entry| *entry == word)
}

fn matches_at(source: &[char], start: usize, pattern: &str) -> bool {
    pattern
        .chars()
        .enumerate()
        .all(|(offset, ch)| source.get(start + offset) == Some(&ch))
}

fn is_identifier_start(ch: char) -> bool {
    ch == '$' || ch == '_' || ch.is_ascii_alphabetic() || (ch as u32) >= 128
}

fn is_identifier_part(ch: char) -> bool {
    is_identifier_start(ch) || ch.is_ascii_digit()
}

fn scan_identifier(source: &[char], start: usize) -> usize {
    let mut index = start + 1;
    while index < source.len() && is_identifier_part(source[index]) {
        index += 1;
    }
    index
}

fn scan_number(source: &[char], start: usize) -> usize {
    let mut index = start + 1;
    while index < source.len() {
        let ch = source[index];
        if !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '.') {
            break;
        }
        index += 1;
    }
    index
}

fn scan_quoted(source: &[char], start: usize) -> usize {
    let quote = source[start];
    let mut index = start + 1;
    while index < source.len() {
        if source[index] == '\\' {
            index += if index + 1 < source.len() { 2 } else { 1 };
            continue;
        }
        if source[index] == quote {
            return index + 1;
        }
        index += 1;
    }
    source.len()
}

fn scan_line_comment(source: &[char], start: usize) -> usize {
    let mut index = start + 2;
    while index < source.len() && source[index] != '\n' && source[index] != '\r' {
        index += 1;
    }
    index
}

fn scan_block_comment(source: &[char], start: usize) -> usize {
    let mut index = start + 2;
    while index < source.len() {
        if source[index] == '*' && source.get(index + 1) == Some(&'/') {
            return index + 2;
        }
        index += 1;
    }
    source.len()
}

fn scan_regex(source: &[char], start: usize) -> usize {
    let mut index = start + 1;
    let mut in_character_class = false;
    while index < source.len() {
        let ch = source[index];
        if ch == '\\' {
            index += if index + 1 < source.len() { 2 } else { 1 };
            continue;
        }
        if ch == '[' {
            in_character_class = true;
        } else if ch == ']' {
            in_character_class = false;
        } else if ch == '/' && !in_character_class {
            index += 1;
            while index < source.len() && is_identifier_part(source[index]) {
                index += 1;
            }
            return index;
        }
        index += 1;
    }
    source.len()
}

enum TemplateFrame {
    Text,
    Expression {
        brace_depth: usize,
        regex_allowed: bool,
    },
}

fn scan_template(source: &[char], start: usize) -> usize {
    let mut frames = vec![TemplateFrame::Text];
    let mut index = start + 1;

    while index < source.len() {
        if frames.is_empty() {
            return index;
        }
        let ch = source[index];
        let next = source.get(index + 1).copied();
        let is_text = matches!(frames.last(), Some(TemplateFrame::Text));

        if is_text {
            if ch == '\\' {
                index += if index + 1 < source.len() { 2 } else { 1 };
            } else if ch == '`' {
                frames.pop();
                index += 1;
                if frames.is_empty() {
                    return index;
                }
                if let Some(TemplateFrame::Expression { regex_allowed, .. }) = frames.last_mut() {
                    *regex_allowed = false;
                }
            } else if ch == '$' && next == Some('{') {
                frames.push(TemplateFrame::Expression {
                    brace_depth: 1,
                    regex_allowed: true,
                });
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }

        if ch == '\'' || ch == '"' {
            index = scan_quoted(source, index);
            set_template_regex_allowed(&mut frames, false);
            continue;
        }
        if ch == '`' {
            frames.push(TemplateFrame::Text);
            index += 1;
            continue;
        }
        if ch == '/' && next == Some('/') {
            index = scan_line_comment(source, index);
            continue;
        }
        if ch == '/' && next == Some('*') {
            index = scan_block_comment(source, index);
            continue;
        }
        if ch == '/' && template_regex_allowed(&frames) {
            index = scan_regex(source, index);
            set_template_regex_allowed(&mut frames, false);
            continue;
        }
        if is_identifier_start(ch) {
            let end = scan_identifier(source, index);
            let word: String = source[index..end].iter().collect();
            set_template_regex_allowed(&mut frames, word_in(REGEX_PREFIX_WORDS, &word));
            index = end;
            continue;
        }
        if ch.is_ascii_digit() {
            index = scan_number(source, index);
            set_template_regex_allowed(&mut frames, false);
            continue;
        }
        if ch == '{' {
            if let Some(TemplateFrame::Expression {
                brace_depth,
                regex_allowed,
            }) = frames.last_mut()
            {
                *brace_depth += 1;
                *regex_allowed = true;
            }
            index += 1;
            continue;
        }
        if ch == '}' {
            let mut pop_expression = false;
            if let Some(TemplateFrame::Expression {
                brace_depth,
                regex_allowed,
            }) = frames.last_mut()
            {
                *brace_depth -= 1;
                if *brace_depth == 0 {
                    pop_expression = true;
                } else {
                    *regex_allowed = false;
                }
            }
            if pop_expression {
                frames.pop();
            }
            index += 1;
            continue;
        }
        if (ch == '+' && next == Some('+')) || (ch == '-' && next == Some('-')) {
            set_template_regex_allowed(&mut frames, false);
            index += 2;
            continue;
        }
        if ch == ')' || ch == ']' || ch == '.' {
            set_template_regex_allowed(&mut frames, false);
        } else if !ch.is_whitespace() {
            set_template_regex_allowed(&mut frames, true);
        }
        index += 1;
    }

    source.len()
}

fn template_regex_allowed(frames: &[TemplateFrame]) -> bool {
    matches!(
        frames.last(),
        Some(TemplateFrame::Expression {
            regex_allowed: true,
            ..
        })
    )
}

fn set_template_regex_allowed(frames: &mut [TemplateFrame], allowed: bool) {
    if let Some(TemplateFrame::Expression { regex_allowed, .. }) = frames.last_mut() {
        *regex_allowed = allowed;
    }
}

fn can_join_close_with_word(word: &str, at_source_end: bool) -> bool {
    CLOSE_CONTINUATIONS
        .iter()
        .any(|keyword| *keyword == word || (at_source_end && keyword.starts_with(word)))
}

fn can_attach_to_close(ch: char) -> bool {
    "();,.)]:?+-*/%&|^<>=!".contains(ch)
}

/// Classifies each source `{` as an object literal (`true`) or a block.
fn classify_source_braces(source: &[char]) -> Vec<bool> {
    let mut objects = Vec::new();
    let mut statement_declares_block = false;
    let mut previous_token = String::new();
    let mut previous_word = String::new();
    let mut regex_allowed = true;

    let mut index = 0;
    while index < source.len() {
        let ch = source[index];
        let next = source.get(index + 1).copied();
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        if ch == '/' && next == Some('/') {
            index = scan_line_comment(source, index);
            continue;
        }
        if ch == '/' && next == Some('*') {
            index = scan_block_comment(source, index);
            continue;
        }
        if ch == '\'' || ch == '"' {
            index = scan_quoted(source, index);
            regex_allowed = false;
            continue;
        }
        if ch == '`' {
            index = scan_template(source, index);
            regex_allowed = false;
            continue;
        }
        if ch == '/' && regex_allowed {
            index = scan_regex(source, index);
            regex_allowed = false;
            continue;
        }
        if is_identifier_start(ch) {
            let end = scan_identifier(source, index);
            let word: String = source[index..end].iter().collect();
            if word == "class" || word == "function" || word == "switch" {
                statement_declares_block = true;
            }
            previous_word = word.clone();
            previous_token = word;
            regex_allowed = word_in(REGEX_PREFIX_WORDS, &previous_token);
            index = end;
            continue;
        }
        if ch.is_ascii_digit() {
            index = scan_number(source, index);
            previous_token = "value".to_string();
            previous_word.clear();
            regex_allowed = false;
            continue;
        }
        if ch == '{' {
            let is_block = previous_token == "=>"
                || previous_token == ")"
                || previous_token == "}"
                || word_in(BLOCK_BRACE_WORDS, &previous_word)
                || statement_declares_block;
            let is_object = !is_block
                && (previous_token == "="
                    || previous_token == ":"
                    || previous_token == ","
                    || previous_token == "("
                    || previous_token == "["
                    || previous_token == ";"
                    || previous_token.is_empty()
                    || word_in(OBJECT_PREFIX_WORDS, &previous_word));
            objects.push(is_object);
            if is_block {
                statement_declares_block = false;
            }
            previous_token = "{".to_string();
            previous_word.clear();
            regex_allowed = true;
            index += 1;
            continue;
        }
        if ch == '}' {
            previous_token = "}".to_string();
            previous_word.clear();
            regex_allowed = false;
            index += 1;
            continue;
        }
        if ch == ';' {
            statement_declares_block = false;
            previous_token = ";".to_string();
            previous_word.clear();
            regex_allowed = true;
            index += 1;
            continue;
        }
        if ch == ')' {
            previous_token = ")".to_string();
            previous_word.clear();
            regex_allowed = false;
            index += 1;
            continue;
        }
        if ch == ']' || ch == '.' {
            previous_token = ch.to_string();
            previous_word.clear();
            regex_allowed = false;
            index += 1;
            continue;
        }
        if ch == '=' && next == Some('>') {
            previous_token = "=>".to_string();
            previous_word.clear();
            regex_allowed = true;
            index += 2;
            continue;
        }
        previous_token = ch.to_string();
        previous_word.clear();
        regex_allowed = ch != ')' && ch != ']' && ch != '.';
        index += 1;
    }
    objects
}

#[derive(Clone, Debug)]
struct DisplayBraceNode {
    start: usize,
    end: Option<usize>,
    object: bool,
    children: Vec<usize>,
}

fn collect_display_brace_nodes(
    text: &[char],
    object_flags: &[bool],
) -> (Vec<DisplayBraceNode>, Vec<usize>) {
    let mut nodes: Vec<DisplayBraceNode> = Vec::new();
    let mut roots = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut opening_index = 0;

    let mut index = 0;
    while index < text.len() {
        let ch = text[index];
        let next = text.get(index + 1).copied();
        if ch == '/' && next == Some('/') {
            index = scan_line_comment(text, index);
            continue;
        }
        if ch == '/' && next == Some('*') {
            index = scan_block_comment(text, index);
            continue;
        }
        if ch == '\'' || ch == '"' {
            index = scan_quoted(text, index);
            continue;
        }
        if ch == '`' {
            index = scan_template(text, index);
            continue;
        }
        if ch == '{' {
            let id = nodes.len();
            nodes.push(DisplayBraceNode {
                start: index,
                end: None,
                object: object_flags.get(opening_index).copied().unwrap_or(false),
                children: Vec::new(),
            });
            opening_index += 1;
            match stack.last().copied() {
                Some(parent) => nodes[parent].children.push(id),
                None => roots.push(id),
            }
            stack.push(id);
            index += 1;
            continue;
        }
        if ch == '}' {
            if let Some(id) = stack.pop() {
                nodes[id].end = Some(index);
            }
            index += 1;
            continue;
        }
        index += 1;
    }
    (nodes, roots)
}

fn collapse_display_whitespace(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::new();
    let mut pending_space = false;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        let next = chars.get(index + 1).copied();
        if ch == '\n' || ch == '\r' || ch.is_whitespace() {
            pending_space = true;
            index += 1;
            continue;
        }
        if ch == '/' && next == Some('/') {
            return None;
        }
        if ch == '/' && next == Some('*') {
            let end = scan_block_comment(&chars, index);
            if pending_space && !output.is_empty() {
                output.push(' ');
            }
            output.extend(&chars[index..end]);
            pending_space = false;
            index = end;
            continue;
        }
        if ch == '\'' || ch == '"' || ch == '`' {
            let end = if ch == '`' {
                scan_template(&chars, index)
            } else {
                scan_quoted(&chars, index)
            };
            let literal: String = chars[index..end].iter().collect();
            if literal.contains('\n') || literal.contains('\r') {
                return None;
            }
            if pending_space && !output.is_empty() {
                output.push(' ');
            }
            output.push_str(&literal);
            pending_space = false;
            index = end;
            continue;
        }
        if pending_space && !output.is_empty() {
            output.push(' ');
        }
        pending_space = false;
        output.push(ch);
        index += 1;
    }
    Some(output.trim().to_string())
}

fn split_display_object_properties(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut properties = Vec::new();
    let mut start = 0;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    for (index, ch) in chars.iter().copied().enumerate() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                properties.push(chars[start..=index].iter().collect());
                start = index + 1;
            }
            _ => {}
        }
    }
    properties.push(chars[start..].iter().collect());
    properties
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ObjectMode {
    Inline,
    Verbatim,
    Explode,
}

fn js_slice(source: &[char], start: usize, end: usize) -> String {
    let start = start.min(source.len());
    let end = end.min(source.len());
    if start >= end {
        String::new()
    } else {
        source[start..end].iter().collect()
    }
}

fn render_display_range(
    nodes: &[DisplayBraceNode],
    formatted: &[char],
    start: usize,
    end: usize,
    children: &[usize],
    child_indent: &str,
) -> String {
    let mut output = String::new();
    let mut cursor = start;
    for &child_id in children {
        let child = &nodes[child_id];
        if child.start < start || child.start >= end {
            continue;
        }
        output.push_str(&js_slice(formatted, cursor, child.start));
        output.push_str(&render_display_node(nodes, formatted, child_id, child_indent));
        cursor = match child.end {
            Some(child_end) => child_end + 1,
            None => end,
        };
    }
    output.push_str(&js_slice(formatted, cursor, end));
    output
}

fn consume_piece(
    mode: &mut ObjectMode,
    width: &mut usize,
    pieces: &mut Vec<String>,
    text: String,
    block_like: bool,
) {
    let empty = text.is_empty();
    let has_newline = text.contains('\n') || text.contains('\r');
    let length = text.chars().count();
    pieces.push(text);
    if *mode != ObjectMode::Inline || empty {
        return;
    }
    if has_newline {
        *mode = if block_like {
            ObjectMode::Verbatim
        } else {
            ObjectMode::Explode
        };
        return;
    }
    *width += length;
    if *width > JS_DISPLAY_MAX_LINE_WIDTH {
        *mode = ObjectMode::Explode;
    }
}

fn render_display_node(
    nodes: &[DisplayBraceNode],
    formatted: &[char],
    id: usize,
    parent_indent: &str,
) -> String {
    let node = &nodes[id];
    let line_start = formatted[..node.start.min(formatted.len())]
        .iter()
        .rposition(|ch| *ch == '\n')
        .map_or(0, |position| position + 1);
    let line_prefix: String = formatted[line_start..node.start.min(formatted.len())]
        .iter()
        .collect();
    let own_indent = if line_prefix.chars().all(|ch| ch == ' ' || ch == '\t') {
        line_prefix
    } else {
        parent_indent.to_string()
    };
    let property_indent = format!("{own_indent}{JS_DISPLAY_INDENT}");
    let closed = node.end.is_some();
    let end = node.end.unwrap_or(formatted.len());

    if !node.object {
        let body = render_display_range(
            nodes,
            formatted,
            node.start + 1,
            end,
            &node.children,
            &property_indent,
        );
        return format!("{{{body}{}", if closed { "}" } else { "" });
    }

    let mut mode = ObjectMode::Inline;
    let mut width = node.start.saturating_sub(line_start) + 4;
    let mut pieces: Vec<String> = Vec::new();
    let mut cursor = node.start + 1;
    for &child_id in &node.children {
        let child = &nodes[child_id];
        if child.start < node.start + 1 || child.start >= end {
            continue;
        }
        let prefix = js_slice(formatted, cursor, child.start);
        consume_piece(&mut mode, &mut width, &mut pieces, prefix, true);
        let rendered = render_display_node(nodes, formatted, child_id, &property_indent);
        consume_piece(&mut mode, &mut width, &mut pieces, rendered, !child.object);
        cursor = match child.end {
            Some(child_end) => child_end + 1,
            None => end,
        };
    }
    let tail = js_slice(formatted, cursor.min(end), end);
    consume_piece(&mut mode, &mut width, &mut pieces, tail, true);
    let body = pieces.concat();

    if mode == ObjectMode::Inline {
        match collapse_display_whitespace(&body) {
            None => mode = ObjectMode::Verbatim,
            Some(flat) => {
                if !closed {
                    return if flat.is_empty() {
                        "{".to_string()
                    } else {
                        format!("{{ {flat}")
                    };
                }
                return if flat.is_empty() {
                    "{}".to_string()
                } else {
                    format!("{{ {flat} }}")
                };
            }
        }
    }
    if mode == ObjectMode::Verbatim {
        return format!("{{{body}{}", if closed { "}" } else { "" });
    }

    let properties: Vec<String> = split_display_object_properties(&body)
        .into_iter()
        .map(|property| property.trim().to_string())
        .filter(|property| !property.is_empty())
        .collect();
    if properties.is_empty() {
        return if closed { "{}" } else { "{" }.to_string();
    }
    let lines: Vec<String> = properties
        .iter()
        .map(|property| format!("{property_indent}{property}"))
        .collect();
    let closer = if closed {
        format!("\n{own_indent}}}")
    } else {
        String::new()
    };
    format!("{{\n{}{closer}", lines.join("\n"))
}

fn format_display_object_literals(formatted: &str, source: &str) -> String {
    let formatted_chars: Vec<char> = formatted.chars().collect();
    let source_chars: Vec<char> = source.chars().collect();
    let (nodes, roots) = collect_display_brace_nodes(
        &formatted_chars,
        &classify_source_braces(&source_chars),
    );
    render_display_range(
        &nodes,
        &formatted_chars,
        0,
        formatted_chars.len(),
        &roots,
        "",
    )
}

/// Finds the next operator token eligible for spacing normalization.
fn scan_display_operator(source: &[char], start: usize) -> Option<String> {
    for candidate in ["===", "!==", "**=", "&&=", "||=", "??="] {
        if matches_at(source, start, candidate) {
            return Some(candidate.to_string());
        }
    }
    for candidate in [
        "=>", "==", "!=", "&&", "||", "??", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=",
        "^=", "**",
    ] {
        if matches_at(source, start, candidate) {
            return Some(candidate.to_string());
        }
    }
    let ch = *source.get(start)?;
    if "=+-/%&|^!?:,".contains(ch) {
        Some(ch.to_string())
    } else {
        None
    }
}

fn operator_spacing(token: &str, unary: bool, ternary_pending: bool) -> (bool, bool) {
    match token {
        "," => (false, true),
        ":" => (ternary_pending, true),
        "?" => (true, true),
        "!" => (false, false),
        "++" | "--" => (false, false),
        _ if unary => (false, false),
        _ => (true, true),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingBreak {
    Brace,
    Statement,
    Close,
}

#[derive(Clone, Copy)]
struct ParenFrame {
    for_header: bool,
    control_header: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Word,
    Punctuation,
    Value,
}

struct JsFormatter<'a> {
    source: &'a [char],
    output: String,
    indent: usize,
    at_line_start: bool,
    last_char: char,
    pending_whitespace: String,
    pending_break: Option<PendingBreak>,
    pending_operator_space: bool,
    ternary_pending: bool,
    after_for_semicolon: bool,
    regex_allowed: bool,
    pending_for: bool,
    last_word: String,
    last_token_was_word: bool,
    parens: Vec<ParenFrame>,
    brace_kinds: Vec<bool>,
    source_object_braces: Vec<bool>,
    source_brace_index: usize,
}

impl<'a> JsFormatter<'a> {
    fn new(source: &'a [char]) -> Self {
        JsFormatter {
            source,
            output: String::new(),
            indent: 0,
            at_line_start: true,
            last_char: '\0',
            pending_whitespace: String::new(),
            pending_break: None,
            pending_operator_space: false,
            ternary_pending: false,
            after_for_semicolon: false,
            regex_allowed: true,
            pending_for: false,
            last_word: String::new(),
            last_token_was_word: false,
            parens: Vec::new(),
            brace_kinds: Vec::new(),
            source_object_braces: classify_source_braces(source),
            source_brace_index: 0,
        }
    }

    fn append(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.output.push_str(text);
        self.last_char = text.chars().last().unwrap_or('\0');
        self.at_line_start = text.ends_with('\n') || text.ends_with('\r');
    }

    fn newline(&mut self) {
        self.output.push('\n');
        self.last_char = '\n';
        self.at_line_start = true;
    }

    fn trim_trailing_horizontal_whitespace(&mut self) {
        while self.output.ends_with(' ') || self.output.ends_with('\t') {
            self.output.pop();
        }
        self.last_char = self.output.chars().last().unwrap_or('\0');
    }

    fn whitespace_width(text: &str) -> usize {
        let mut width = 0;
        for ch in text.chars() {
            width += if ch == '\t' { 4 - (width % 4) } else { 1 };
        }
        width
    }

    fn flush_whitespace(&mut self) {
        if self.at_line_start {
            let width = (self.indent * 4).max(Self::whitespace_width(&self.pending_whitespace));
            if width > 0 {
                self.append(&" ".repeat(width));
            }
        } else if !self.pending_whitespace.is_empty() {
            self.append(" ");
        }
        self.pending_whitespace.clear();
    }

    fn flush_operator_space(&mut self, next: Option<char>) {
        if !self.pending_operator_space {
            return;
        }
        let first = next.unwrap_or('\0');
        if !self.at_line_start && self.last_char != ' ' && !")]},.;".contains(first) {
            self.append(" ");
        }
        self.pending_whitespace.clear();
        self.pending_operator_space = false;
    }

    fn append_operator(&mut self, token: &str, before: bool, after: bool) {
        self.trim_trailing_horizontal_whitespace();
        if before && !self.at_line_start && self.last_char != ' ' && self.last_char != '\n' {
            self.append(" ");
        }
        self.append(token);
        self.pending_operator_space = after;
    }

    fn force_break(&mut self) {
        self.pending_whitespace.clear();
        self.pending_operator_space = false;
        if !self.at_line_start {
            self.newline();
        }
        self.pending_break = None;
    }

    fn prepare_token(&mut self, kind: TokenKind, text: &str, end: usize) {
        self.flush_operator_space(text.chars().next());
        match self.pending_break {
            Some(PendingBreak::Close) => {
                if kind == TokenKind::Word
                    && can_join_close_with_word(text, end == self.source.len())
                {
                    self.pending_whitespace.clear();
                    if !self.at_line_start && self.last_char != ' ' {
                        self.append(" ");
                    }
                    self.pending_break = None;
                } else if kind == TokenKind::Punctuation
                    && text.chars().next().is_some_and(can_attach_to_close)
                {
                    self.pending_whitespace.clear();
                    self.pending_break = None;
                } else {
                    self.force_break();
                }
            }
            Some(_) => self.force_break(),
            None => {}
        }

        if self.after_for_semicolon {
            if text != ";" && text != ")" && !self.at_line_start && self.pending_whitespace.is_empty()
            {
                self.append(" ");
            }
            self.after_for_semicolon = false;
        }
    }

    fn append_comment(&mut self, start: usize, end: usize) {
        let comment: String = self.source[start..end].iter().collect();
        if self.pending_break.is_some() {
            if !self.pending_whitespace.is_empty() {
                let pending = std::mem::take(&mut self.pending_whitespace);
                self.append(&pending);
            } else if !self.at_line_start {
                self.append(" ");
            }
            self.pending_whitespace.clear();
        } else {
            self.flush_whitespace();
        }
        self.append(&comment);
        if comment.contains('\n') || comment.contains('\r') {
            self.pending_break = None;
        }
        if self.after_for_semicolon {
            self.after_for_semicolon = false;
        }
    }

    fn format(mut self) -> String {
        let mut index = 0usize;
        while index < self.source.len() {
            let ch = self.source[index];
            let next = self.source.get(index + 1).copied();

            if ch == '\n' || ch == '\r' {
                self.pending_whitespace.clear();
                self.pending_operator_space = false;
                self.pending_break = None;
                self.after_for_semicolon = false;
                self.newline();
                index += if ch == '\r' && next == Some('\n') { 2 } else { 1 };
                continue;
            }
            if ch.is_whitespace() {
                let start = index;
                while index < self.source.len() && self.source[index].is_whitespace() && self.source[index] != '\n' && self.source[index] != '\r' {
                    index += 1;
                }
                self.pending_whitespace.extend(&self.source[start..index]);
                continue;
            }
            if ch == '/' && next == Some('/') {
                let end = scan_line_comment(self.source, index);
                self.append_comment(index, end);
                index = end;
                continue;
            }
            if ch == '/' && next == Some('*') {
                let end = scan_block_comment(self.source, index);
                self.append_comment(index, end);
                index = end;
                continue;
            }
            if ch == '\'' || ch == '"' {
                let end = scan_quoted(self.source, index);
                let literal: String = self.source[index..end].iter().collect();
                self.prepare_token(TokenKind::Value, &literal, end);
                self.flush_whitespace();
                self.append(&literal);
                index = end;
                self.regex_allowed = false;
                self.pending_for = false;
                self.last_token_was_word = false;
                continue;
            }
            if ch == '`' {
                let end = scan_template(self.source, index);
                let literal: String = self.source[index..end].iter().collect();
                self.prepare_token(TokenKind::Value, &literal, end);
                self.flush_whitespace();
                self.append(&literal);
                index = end;
                self.regex_allowed = false;
                self.pending_for = false;
                self.last_token_was_word = false;
                continue;
            }
            if ch == '/' && self.regex_allowed {
                let end = scan_regex(self.source, index);
                let literal: String = self.source[index..end].iter().collect();
                self.prepare_token(TokenKind::Value, &literal, end);
                self.flush_whitespace();
                self.append(&literal);
                index = end;
                self.regex_allowed = false;
                self.pending_for = false;
                self.last_token_was_word = false;
                continue;
            }
            if is_identifier_start(ch) {
                let end = scan_identifier(self.source, index);
                let word: String = self.source[index..end].iter().collect();
                self.prepare_token(TokenKind::Word, &word, end);
                self.flush_whitespace();
                self.append(&word);
                if word == "for" {
                    self.pending_for = true;
                } else if !(self.pending_for && word == "await") {
                    self.pending_for = false;
                }
                self.regex_allowed = word_in(REGEX_PREFIX_WORDS, &word);
                self.last_word = word;
                self.last_token_was_word = true;
                index = end;
                continue;
            }
            if ch.is_ascii_digit() {
                let end = scan_number(self.source, index);
                let number: String = self.source[index..end].iter().collect();
                self.prepare_token(TokenKind::Value, &number, end);
                self.flush_whitespace();
                self.append(&number);
                index = end;
                self.regex_allowed = false;
                self.pending_for = false;
                self.last_token_was_word = false;
                continue;
            }
            if ch == '{' {
                let object_brace = self
                    .source_object_braces
                    .get(self.source_brace_index)
                    .copied()
                    .unwrap_or(false);
                self.source_brace_index += 1;
                self.brace_kinds.push(object_brace);
                self.prepare_token(TokenKind::Punctuation, "{", index + 1);
                let had_whitespace = !self.pending_whitespace.is_empty();
                self.flush_whitespace();
                if !self.at_line_start && !had_whitespace && !" ([{".contains(self.last_char) {
                    self.append(" ");
                }
                self.append("{");
                if !object_brace {
                    self.indent += 1;
                    self.pending_break = Some(PendingBreak::Brace);
                }
                self.regex_allowed = true;
                self.pending_for = false;
                self.last_token_was_word = false;
                index += 1;
                continue;
            }
            if ch == '}' {
                let object_brace = self.brace_kinds.pop().unwrap_or(false);
                self.prepare_token(TokenKind::Punctuation, "}", index + 1);
                if object_brace {
                    if self.at_line_start {
                        self.flush_whitespace();
                    } else {
                        self.pending_whitespace.clear();
                        self.trim_trailing_horizontal_whitespace();
                    }
                    self.append("}");
                    self.pending_break = None;
                } else {
                    self.pending_whitespace.clear();
                    if !self.at_line_start {
                        self.newline();
                    }
                    self.indent = self.indent.saturating_sub(1);
                    self.flush_whitespace();
                    self.append("}");
                    self.pending_break = Some(PendingBreak::Close);
                }
                self.regex_allowed = false;
                self.pending_for = false;
                self.last_token_was_word = false;
                index += 1;
                continue;
            }
            if ch == ';' {
                self.prepare_token(TokenKind::Punctuation, ";", index + 1);
                self.flush_whitespace();
                self.append(";");
                let for_header = self.parens.last().is_some_and(|frame| frame.for_header);
                if for_header {
                    self.after_for_semicolon = true;
                } else {
                    self.pending_break = Some(PendingBreak::Statement);
                }
                self.regex_allowed = true;
                self.pending_for = false;
                self.last_token_was_word = false;
                index += 1;
                continue;
            }
            if ch == '(' {
                let for_header = self.pending_for;
                let control_header = for_header
                    || (self.last_token_was_word && word_in(CONTROL_HEADER_WORDS, &self.last_word));
                self.prepare_token(TokenKind::Punctuation, "(", index + 1);
                let needs_space = control_header
                    && self.pending_whitespace.is_empty()
                    && !self.at_line_start;
                self.flush_whitespace();
                if needs_space {
                    self.append(" ");
                }
                self.append("(");
                self.parens.push(ParenFrame {
                    for_header,
                    control_header,
                });
                self.regex_allowed = true;
                self.pending_for = false;
                self.last_token_was_word = false;
                index += 1;
                continue;
            }
            if ch == ')' {
                self.prepare_token(TokenKind::Punctuation, ")", index + 1);
                self.flush_whitespace();
                self.append(")");
                let frame = self.parens.pop();
                self.regex_allowed = frame.is_some_and(|frame| frame.control_header);
                self.pending_for = false;
                self.last_token_was_word = false;
                index += 1;
                continue;
            }

            if let Some(operator) = scan_display_operator(self.source, index) {
                let skip_question = operator == "?" && (next == Some('.') || next == Some(':'));
                if !skip_question {
                    let unary =
                        (operator == "+" || operator == "-") && self.regex_allowed;
                    self.prepare_token(
                        TokenKind::Punctuation,
                        &operator,
                        index + operator.chars().count(),
                    );
                    self.flush_whitespace();
                    let (before, after) =
                        operator_spacing(&operator, unary, self.ternary_pending);
                    self.append_operator(&operator, before, after);
                    if operator == "?" {
                        self.ternary_pending = true;
                    } else if operator == ":" {
                        self.ternary_pending = false;
                    }
                    self.regex_allowed = operator != "++" && operator != "--";
                    self.pending_for = false;
                    self.last_token_was_word = false;
                    index += operator.chars().count();
                    continue;
                }
            }

            let doubled_postfix =
                (ch == '+' && next == Some('+')) || (ch == '-' && next == Some('-'));
            let token = if doubled_postfix {
                self.source[index..index + 2].iter().collect::<String>()
            } else {
                ch.to_string()
            };
            self.prepare_token(
                TokenKind::Punctuation,
                &token,
                index + token.chars().count(),
            );
            self.flush_whitespace();
            self.append(&token);
            if doubled_postfix || ch == ']' || ch == '.' {
                self.regex_allowed = false;
            } else {
                self.regex_allowed = true;
            }
            self.pending_for = false;
            self.last_token_was_word = false;
            index += token.chars().count();
        }

        let source: String = self.source.iter().collect();
        format_display_object_literals(&self.output, &source)
    }
}

/// Formats JavaScript/TypeScript-like eval source for safe, stable display
/// without requiring valid syntax.
pub fn format_javascript_for_display(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    JsFormatter::new(&chars).format()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_control_flow_and_keeps_short_objects_inline() {
        assert_eq!(
            format_javascript_for_display(
                "if (ready){const item = { value: 1 };use(item);}else{fallback();}"
            ),
            [
                "if (ready) {",
                "    const item = { value: 1 };",
                "    use(item);",
                "} else {",
                "    fallback();",
                "}",
            ]
            .join("\n")
        );
        assert_eq!(
            format_javascript_for_display("const rows = [{ value: 1 },{ value: 2 }];"),
            "const rows = [{ value: 1 }, { value: 2 }];"
        );
    }

    #[test]
    fn normalizes_operators_and_for_headers() {
        assert_eq!(
            format_javascript_for_display("for(;;){tick();}for(let i=0;i<2;i++){work(i);}"),
            [
                "for (;;) {",
                "    tick();",
                "}",
                "for (let i = 0; i<2; i++) {",
                "    work(i);",
                "}",
            ]
            .join("\n")
        );
        assert_eq!(
            format_javascript_for_display("const seen=new Map<string,number>();"),
            "const seen = new Map<string, number>();"
        );
    }

    #[test]
    fn unfinished_input_stays_safe() {
        assert_eq!(format_javascript_for_display(""), "");
        assert_eq!(format_javascript_for_display("`a ${b"), "`a ${b");
        assert_eq!(format_javascript_for_display("if (x) {"), "if (x) {");
    }
}
