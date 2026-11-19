//! Python display formatting.
//!
//! Ported from `l/src/tools/eval-format/python.ts`. Re-indents a source prefix
//! by logical suite structure, tolerating incomplete input.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeaderKind {
    Def,
    Class,
    If,
    Elif,
    Else,
    For,
    While,
    Try,
    Except,
    Finally,
    With,
    Match,
    Case,
    AsyncDef,
    AsyncFor,
    AsyncWith,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChainKind {
    If,
    Loop,
    Try,
    Match,
}

impl ChainKind {
    fn index(self) -> usize {
        match self {
            ChainKind::If => 0,
            ChainKind::Loop => 1,
            ChainKind::Try => 2,
            ChainKind::Match => 3,
        }
    }
}

#[derive(Clone, Copy)]
struct Word {
    text: &'static str,
    end: usize,
}

#[derive(Clone, Copy)]
struct Header {
    kind: HeaderKind,
    end: usize,
}

#[derive(Clone, Copy)]
struct BlockFrame {
    kind: HeaderKind,
    chain: Option<ChainKind>,
    header_indent: usize,
    source_indent: usize,
    previous_chain: i32,
}

#[derive(Clone, Copy)]
struct DelimiterFrame {
    opener: char,
    output_indent: usize,
}

struct PendingSuiteColon {
    kind: HeaderKind,
    chain: Option<ChainKind>,
    header_indent: usize,
    source_indent: usize,
    after_colon: Vec<char>,
}

#[derive(Clone, Copy)]
struct StringState {
    quote: char,
    triple: bool,
    escaped: bool,
}

#[derive(Clone, Copy)]
struct ClauseAlignment {
    indent: usize,
    chain: Option<ChainKind>,
}

fn is_word_start(ch: Option<char>) -> bool {
    ch.is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
}

fn is_word_part(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn read_word(source: &[char], start: usize) -> Word {
    let mut end = start;
    while end < source.len() && is_word_part(source[end]) {
        end += 1;
    }
    let text: String = source[start..end].iter().collect();
    let text = match text.as_str() {
        "def" => "def",
        "class" => "class",
        "if" => "if",
        "elif" => "elif",
        "else" => "else",
        "for" => "for",
        "while" => "while",
        "try" => "try",
        "except" => "except",
        "finally" => "finally",
        "with" => "with",
        "match" => "match",
        "case" => "case",
        "async" => "async",
        _ => "",
    };
    Word { text, end }
}

fn simple_header(word: Word) -> Option<Header> {
    let kind = match word.text {
        "def" => HeaderKind::Def,
        "class" => HeaderKind::Class,
        "if" => HeaderKind::If,
        "elif" => HeaderKind::Elif,
        "else" => HeaderKind::Else,
        "for" => HeaderKind::For,
        "while" => HeaderKind::While,
        "try" => HeaderKind::Try,
        "except" => HeaderKind::Except,
        "finally" => HeaderKind::Finally,
        "with" => HeaderKind::With,
        "match" => HeaderKind::Match,
        "case" => HeaderKind::Case,
        _ => return None,
    };
    Some(Header { kind, end: word.end })
}

fn read_header(source: &[char], start: usize) -> Option<Header> {
    if !is_word_start(source.get(start).copied()) {
        return None;
    }
    let first = read_word(source, start);
    if first.text != "async" {
        return simple_header(first);
    }

    let mut next = first.end;
    while source.get(next) == Some(&' ') || source.get(next) == Some(&'\t') {
        next += 1;
    }
    if !is_word_start(source.get(next).copied()) {
        return None;
    }
    let second = read_word(source, next);
    match second.text {
        "def" => Some(Header {
            kind: HeaderKind::AsyncDef,
            end: second.end,
        }),
        "for" => Some(Header {
            kind: HeaderKind::AsyncFor,
            end: second.end,
        }),
        "with" => Some(Header {
            kind: HeaderKind::AsyncWith,
            end: second.end,
        }),
        _ => None,
    }
}

fn default_chain(kind: HeaderKind) -> Option<ChainKind> {
    match kind {
        HeaderKind::If | HeaderKind::Elif | HeaderKind::Else => Some(ChainKind::If),
        HeaderKind::For | HeaderKind::While | HeaderKind::AsyncFor => Some(ChainKind::Loop),
        HeaderKind::Try | HeaderKind::Except | HeaderKind::Finally => Some(ChainKind::Try),
        HeaderKind::Match | HeaderKind::Case => Some(ChainKind::Match),
        _ => None,
    }
}

fn can_open_suite(kind: HeaderKind, has_payload: bool) -> bool {
    match kind {
        HeaderKind::Else | HeaderKind::Try | HeaderKind::Finally => !has_payload,
        HeaderKind::Except => true,
        _ => has_payload,
    }
}

fn matching_closer(opener: char, closer: char) -> bool {
    (opener == '(' && closer == ')')
        || (opener == '[' && closer == ']')
        || (opener == '{' && closer == '}')
}

struct PyFormatter<'a> {
    source: &'a [char],
    chunks: String,
    blocks: Vec<BlockFrame>,
    delimiters: Vec<DelimiterFrame>,
    chain_tops: [i32; 4],
    pending_horizontal: String,
    output_line_start: bool,
    line_indent: Option<usize>,
    current_output_indent: usize,
    source_line_start: bool,
    source_indent: usize,
    current_source_indent: usize,
    skip_generated_newline: bool,
    statement_prepared: bool,
    statement_kind: Option<HeaderKind>,
    statement_header_end: i32,
    statement_has_payload: bool,
    statement_indent: usize,
    statement_source_indent: usize,
    statement_chain: Option<ChainKind>,
    pending_colon: Option<PendingSuiteColon>,
    string_state: Option<StringState>,
    in_comment: bool,
}

impl<'a> PyFormatter<'a> {
    fn new(source: &'a [char]) -> Self {
        PyFormatter {
            source,
            chunks: String::new(),
            blocks: Vec::new(),
            delimiters: Vec::new(),
            chain_tops: [-1; 4],
            pending_horizontal: String::new(),
            output_line_start: true,
            line_indent: None,
            current_output_indent: 0,
            source_line_start: true,
            source_indent: 0,
            current_source_indent: 0,
            skip_generated_newline: false,
            statement_prepared: false,
            statement_kind: None,
            statement_header_end: -1,
            statement_has_payload: false,
            statement_indent: 0,
            statement_source_indent: 0,
            statement_chain: None,
            pending_colon: None,
            string_state: None,
            in_comment: false,
        }
    }

    fn current_block_indent(&self) -> usize {
        self.blocks
            .last()
            .map_or(0, |frame| frame.header_indent + 1)
    }

    fn pop_block(&mut self) {
        let index = self.blocks.len().saturating_sub(1) as i32;
        if let Some(frame) = self.blocks.pop() {
            if let Some(chain) = frame.chain {
                if self.chain_tops[chain.index()] == index {
                    self.chain_tops[chain.index()] = frame.previous_chain;
                }
            }
        }
    }

    fn pop_through(&mut self, index: i32) {
        while self.blocks.len() as i32 > index {
            self.pop_block();
        }
    }

    fn push_block(&mut self, frame: &PendingSuiteColon) {
        let previous_chain = frame.chain.map_or(-1, |chain| self.chain_tops[chain.index()]);
        self.blocks.push(BlockFrame {
            kind: frame.kind,
            chain: frame.chain,
            header_indent: frame.header_indent,
            source_indent: frame.source_indent,
            previous_chain,
        });
        if let Some(chain) = frame.chain {
            self.chain_tops[chain.index()] = self.blocks.len() as i32 - 1;
        }
    }

    fn reset_statement(&mut self) {
        self.statement_prepared = false;
        self.statement_kind = None;
        self.statement_header_end = -1;
        self.statement_has_payload = false;
        self.statement_indent = self.current_block_indent();
        self.statement_source_indent = self.current_source_indent;
        self.statement_chain = None;
    }

    fn flush_horizontal(&mut self) {
        if self.pending_horizontal.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.pending_horizontal).replace('\t', "    ");
        self.chunks.push_str(&text);
    }

    fn append_normal(&mut self, text: &str) {
        if self.output_line_start {
            let indent = self.line_indent.unwrap_or_else(|| self.current_block_indent());
            if indent > 0 {
                self.chunks.push_str(&"    ".repeat(indent));
            }
            self.current_output_indent = indent;
            self.output_line_start = false;
        }
        self.flush_horizontal();
        self.chunks.push_str(text);
    }

    fn append_raw(&mut self, text: &str) {
        self.chunks.push_str(text);
        if self.output_line_start {
            self.output_line_start = false;
            self.current_output_indent = 0;
        }
    }

    fn finish_output_line(&mut self) {
        self.pending_horizontal.clear();
        self.chunks.push('\n');
        self.output_line_start = true;
        self.line_indent = None;
        self.current_output_indent = 0;
    }

    fn consume_source_newline(&mut self) {
        self.source_line_start = true;
        self.source_indent = 0;
        self.current_source_indent = 0;
        self.skip_generated_newline = false;
        if self.delimiters.is_empty() {
            self.reset_statement();
        }
    }

    fn pop_source_dedents(&mut self, indent: usize) {
        while self
            .blocks
            .last()
            .is_some_and(|frame| indent <= frame.source_indent)
        {
            self.pop_block();
        }
    }

    fn align_to(&mut self, index: i32, fallback: ChainKind) -> ClauseAlignment {
        if index < 0 {
            return ClauseAlignment {
                indent: self.current_block_indent(),
                chain: Some(fallback),
            };
        }
        let frame = self.blocks[index as usize];
        let alignment = ClauseAlignment {
            indent: frame.header_indent,
            chain: Some(frame.chain.unwrap_or(fallback)),
        };
        self.pop_through(index);
        alignment
    }

    fn align_clause(&mut self, kind: HeaderKind) -> Option<ClauseAlignment> {
        match kind {
            HeaderKind::Elif => Some(self.align_to(self.chain_tops[ChainKind::If.index()], ChainKind::If)),
            HeaderKind::Except | HeaderKind::Finally => {
                Some(self.align_to(self.chain_tops[ChainKind::Try.index()], ChainKind::Try))
            }
            HeaderKind::Else => {
                let target = self.chain_tops[ChainKind::If.index()]
                    .max(self.chain_tops[ChainKind::Loop.index()])
                    .max(self.chain_tops[ChainKind::Try.index()]);
                Some(self.align_to(target, ChainKind::If))
            }
            HeaderKind::Case => {
                let target = self.chain_tops[ChainKind::Match.index()];
                if target < 0 {
                    return Some(ClauseAlignment {
                        indent: self.current_block_indent(),
                        chain: Some(ChainKind::Match),
                    });
                }
                let frame = self.blocks[target as usize];
                if frame.kind == HeaderKind::Match {
                    while self.blocks.len() as i32 > target + 1 {
                        self.pop_block();
                    }
                    return Some(ClauseAlignment {
                        indent: frame.header_indent + 1,
                        chain: Some(ChainKind::Match),
                    });
                }
                let indent = frame.header_indent;
                self.pop_through(target);
                Some(ClauseAlignment {
                    indent,
                    chain: Some(ChainKind::Match),
                })
            }
            _ => None,
        }
    }

    fn prepare_statement(&mut self, index: usize, physical_line_start: bool) {
        if physical_line_start {
            self.pop_source_dedents(self.current_source_indent);
        }
        let header = read_header(self.source, index);
        let alignment = header.and_then(|header| self.align_clause(header.kind));
        self.statement_prepared = true;
        self.statement_kind = header.map(|header| header.kind);
        self.statement_header_end = header.map_or(-1, |header| header.end as i32);
        self.statement_has_payload = false;
        self.statement_indent = alignment.map_or_else(|| self.current_block_indent(), |a| a.indent);
        self.statement_source_indent = self.current_source_indent;
        self.statement_chain = alignment
            .and_then(|alignment| alignment.chain)
            .or_else(|| header.and_then(|header| default_chain(header.kind)));
        self.line_indent = Some(self.statement_indent);
    }

    fn prepare_token(&mut self, index: usize, ch: char, comment: bool) {
        let physical_line_start = self.source_line_start;
        if physical_line_start {
            self.current_source_indent = self.source_indent;
            self.source_line_start = false;
        }
        if self.skip_generated_newline {
            self.skip_generated_newline = false;
        }
        if !self.output_line_start {
            return;
        }

        if let Some(delimiter) = self.delimiters.last().copied() {
            if self.statement_prepared {
                let closes = matching_closer(delimiter.opener, ch);
                let structural_indent = delimiter.output_indent + usize::from(!closes);
                self.line_indent = Some(
                    structural_indent.max((self.current_source_indent + 3) / 4),
                );
                return;
            }
        }

        if comment {
            self.line_indent = Some(if physical_line_start {
                self.current_block_indent()
                    .min((self.current_source_indent + 3) / 4)
            } else {
                self.current_block_indent()
            });
            return;
        }
        if !self.statement_prepared {
            self.prepare_statement(index, physical_line_start);
        } else {
            self.line_indent = Some(self.statement_indent);
        }
    }

    fn note_payload(&mut self, index: usize) {
        if self.statement_kind.is_some() && index as i32 >= self.statement_header_end {
            self.statement_has_payload = true;
        }
    }

    fn open_pending_suite(&mut self, frame: PendingSuiteColon) {
        self.push_block(&frame);
        self.reset_statement();
    }

    fn format(mut self) -> String {
        let mut index = 0usize;
        while index < self.source.len() {
            let mut ch = self.source[index];

            if self.string_state.is_some() {
                let mut state = self.string_state.take().unwrap();
                let newline = ch == '\n' || ch == '\r';
                if newline {
                    let width = if ch == '\r' && self.source.get(index + 1) == Some(&'\n') {
                        2
                    } else {
                        1
                    };
                    let raw: String = self.source[index..index + width].iter().collect();
                    self.append_raw(&raw);
                    self.output_line_start = true;
                    self.line_indent = None;
                    self.current_output_indent = 0;
                    self.source_line_start = true;
                    self.source_indent = 0;
                    state.escaped = false;
                    self.string_state = Some(state);
                    index += width;
                    continue;
                }

                if state.triple
                    && !state.escaped
                    && ch == state.quote
                    && self.source.get(index + 1) == Some(&ch)
                    && self.source.get(index + 2) == Some(&ch)
                {
                    let raw: String = self.source[index..index + 3].iter().collect();
                    self.append_raw(&raw);
                    self.source_line_start = false;
                    index += 3;
                    continue;
                }

                self.append_raw(&ch.to_string());
                self.source_line_start = false;
                if state.escaped {
                    state.escaped = false;
                } else if ch == '\\' {
                    state.escaped = true;
                } else if !state.triple && ch == state.quote {
                    index += 1;
                    continue;
                }
                self.string_state = Some(state);
                index += 1;
                continue;
            }

            if self.in_comment {
                if ch == '\n' || ch == '\r' {
                    let width = if ch == '\r' && self.source.get(index + 1) == Some(&'\n') {
                        2
                    } else {
                        1
                    };
                    self.finish_output_line();
                    self.in_comment = false;
                    self.consume_source_newline();
                    index += width;
                    continue;
                }
                self.append_raw(&ch.to_string());
                index += 1;
                continue;
            }
            if let Some(mut frame) = self.pending_colon.take() {
                if ch == ' ' || ch == '\t' {
                    frame.after_colon.push(ch);
                    self.pending_colon = Some(frame);
                    index += 1;
                    continue;
                }
                if ch == '=' && frame.after_colon.is_empty() {
                    self.append_normal(":");
                } else if ch == '#' {
                    self.append_normal(":");
                    if !frame.after_colon.is_empty() {
                        let text: String = frame.after_colon.iter().collect();
                        self.chunks.push_str(&text.replace('\t', "    "));
                    }
                    self.open_pending_suite(frame);
                } else if ch == '\n' || ch == '\r' {
                    let width = if ch == '\r' && self.source.get(index + 1) == Some(&'\n') {
                        2
                    } else {
                        1
                    };
                    self.append_normal(":");
                    self.open_pending_suite(frame);
                    self.finish_output_line();
                    self.consume_source_newline();
                    index += width;
                    continue;
                } else {
                    self.append_normal(":");
                    self.open_pending_suite(frame);
                    self.finish_output_line();
                    self.skip_generated_newline = true;
                    continue;
                }
                ch = self.source[index];
            }

            if self.source_line_start && (ch == ' ' || ch == '\t') {
                if ch == '\t' {
                    self.source_indent += 4 - (self.source_indent % 4);
                } else {
                    self.source_indent += 1;
                }
                index += 1;
                continue;
            }

            if ch == '\n' || ch == '\r' {
                let width = if ch == '\r' && self.source.get(index + 1) == Some(&'\n') {
                    2
                } else {
                    1
                };
                self.pending_horizontal.clear();
                if !self.skip_generated_newline {
                    self.finish_output_line();
                }
                self.consume_source_newline();
                index += width;
                continue;
            }

            if ch == ' ' || ch == '\t' {
                if !self.skip_generated_newline && (!self.output_line_start || self.statement_prepared) {
                    self.pending_horizontal.push(ch);
                }
                index += 1;
                continue;
            }

            self.prepare_token(index, ch, ch == '#');

            if ch == '#' {
                self.append_normal("#");
                self.in_comment = true;
                index += 1;
                continue;
            }

            if ch == '\'' || ch == '"' {
                self.note_payload(index);
                let triple = self.source.get(index + 1) == Some(&ch)
                    && self.source.get(index + 2) == Some(&ch);
                let opening: String = if triple {
                    self.source[index..index + 3].iter().collect()
                } else {
                    ch.to_string()
                };
                self.append_normal(&opening);
                self.string_state = Some(StringState {
                    quote: ch,
                    triple,
                    escaped: false,
                });
                index += if triple { 3 } else { 1 };
                continue;
            }

            if ch == ':'
                && self.delimiters.is_empty()
                && self.statement_kind.is_some_and(|kind| {
                    can_open_suite(kind, self.statement_has_payload)
                })
            {
                self.flush_horizontal();
                self.pending_colon = Some(PendingSuiteColon {
                    kind: self.statement_kind.unwrap(),
                    chain: self.statement_chain,
                    header_indent: self.statement_indent,
                    source_indent: self.statement_source_indent,
                    after_colon: Vec::new(),
                });
                index += 1;
                continue;
            }

            if ch == ';' && self.delimiters.is_empty() {
                self.append_normal(";");
                self.finish_output_line();
                self.reset_statement();
                self.skip_generated_newline = true;
                index += 1;
                continue;
            }

            self.note_payload(index);
            self.append_normal(&ch.to_string());
            if ch == '(' || ch == '[' || ch == '{' {
                self.delimiters.push(DelimiterFrame {
                    opener: ch,
                    output_indent: self.current_output_indent,
                });
            } else if self
                .delimiters
                .last()
                .is_some_and(|delimiter| matching_closer(delimiter.opener, ch))
            {
                self.delimiters.pop();
            }
            index += 1;
        }

        if self.pending_colon.is_some() {
            self.append_normal(":");
        }
        self.chunks
    }
}

/// Formats an arbitrary Python source prefix for stable, readable display.
pub fn format_python_for_display(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    PyFormatter::new(&chars).format()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_compact_nested_suites() {
        assert_eq!(
            format_python_for_display(
                "class Classifier:def classify(self,value):if value>0:return \"positive\";elif value<0:return \"negative\";else:return \"zero\""
            ),
            [
                "class Classifier:",
                "    def classify(self,value):",
                "        if value>0:",
                "            return \"positive\";",
                "        elif value<0:",
                "            return \"negative\";",
                "        else:",
                "            return \"zero\"",
            ]
            .join("\n")
        );
    }

    #[test]
    fn preserves_triple_quoted_strings() {
        assert_eq!(
            format_python_for_display("x = '''a\n  b\n'''\n"),
            "x = '''a\n  b\n'''\n"
        );
    }

    #[test]
    fn incomplete_headers_are_safe() {
        assert_eq!(format_python_for_display(""), "");
        assert_eq!(format_python_for_display("if x"), "if x");
    }
}
