//! LSP tool render-model types and helpers.
//!
 //! Reference: `l/src/tools/lsp.ts`.
 //!
//! Deviations:
//! - JS-runtime-only facilities dropped: TUI `Component`/`Text` renderers,
//!   cached/memoized views, ANSI theme coloring, syntax highlighting,
//!   interactive expansion and themed code-frame rendering.
//! - Ported structurally: location/reference grouping, source-line marking,
//!   diagnostics grouping and per-file status rows, plus plain render helpers.
/// LSP action names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LspAction {
	Diagnostics,
	Definition,
	References,
	Hover,
	Symbols,
	Rename,
	RenameFile,
	CodeActions,
	TypeDefinition,
	Implementation,
	Status,
	Reload,
	Capabilities,
	Request,
}

impl LspAction {
	pub fn parse(s: &str) -> Option<Self> {
		Some(match s {
			"diagnostics" => Self::Diagnostics,
			"definition" => Self::Definition,
			"references" => Self::References,
			"hover" => Self::Hover,
			"symbols" => Self::Symbols,
			"rename" => Self::Rename,
			"rename_file" => Self::RenameFile,
			"code_actions" => Self::CodeActions,
			"type_definition" => Self::TypeDefinition,
			"implementation" => Self::Implementation,
			"status" => Self::Status,
			"reload" => Self::Reload,
			"capabilities" => Self::Capabilities,
			"request" => Self::Request,
			_ => return None,
		})
	}

	pub fn as_str(self) -> &'static str {
		match self {
			Self::Diagnostics => "diagnostics",
			Self::Definition => "definition",
			Self::References => "references",
			Self::Hover => "hover",
			Self::Symbols => "symbols",
			Self::Rename => "rename",
			Self::RenameFile => "rename_file",
			Self::CodeActions => "code_actions",
			Self::TypeDefinition => "type_definition",
			Self::Implementation => "implementation",
			Self::Status => "status",
			Self::Reload => "reload",
			Self::Capabilities => "capabilities",
			Self::Request => "request",
		}
	}

	pub fn label(self) -> String {
		self.as_str().replace('_', " ")
	}
}

/// Display arguments for an LSP tool request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LspParams {
	pub action: Option<LspAction>,
	pub file: Option<String>,
	pub line: Option<u32>,
	pub symbol: Option<String>,
	pub query: Option<String>,
	pub new_name: Option<String>,
	pub apply: Option<bool>,
	pub timeout: Option<u64>,
	pub payload: Option<String>,
}

/// Details accompanying an LSP tool response.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LspToolDetails {
	pub server_name: Option<String>,
	pub action: Option<LspAction>,
	pub success: bool,
	pub request: Option<LspParams>,
}

/// Outcome of automatic file formatting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFormatResult {
	Unchanged,
	Formatted,
	Failed,
	Unsupported,
}

impl FileFormatResult {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Unchanged => "unchanged",
			Self::Formatted => "formatted",
			Self::Failed => "failed",
			Self::Unsupported => "unsupported",
		}
	}
}

/// Diagnostics and formatting status for one file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiagnosticsResult {
	pub server: Option<String>,
	pub messages: Vec<String>,
	pub summary: String,
	pub errored: bool,
	pub unreported: Vec<String>,
	pub formatter: Option<FileFormatResult>,
}

/// LSP request status summary (description + meta facts).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LspRequestStatus {
	pub description: String,
	pub meta: Vec<String>,
}

fn sanitize_inline_text(value: &str) -> String {
	value.replace(['\t', '\n', '\r'], " ")
}

fn truncate_to_width(s: &str, max: usize) -> String {
	let count = s.chars().count();
	if count <= max {
		s.to_string()
	} else if max <= 1 {
		"…".to_string()
	} else {
		let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
		out.push('…');
		out
	}
}

fn shorten_path(p: &str) -> String {
	// plain basename-ish fallback; full shortenPath depends on cwd/theme utils
	p.to_string()
}

/// Build request status: `action target` description and meta facts.
pub fn lsp_request_status(args: Option<&LspParams>, fallback_action: &str) -> LspRequestStatus {
	let action_label = args
		.and_then(|a| a.action)
		.map(|a| a.label())
		.unwrap_or_else(|| fallback_action.replace('_', " "));
	let query_preview = args
		.and_then(|a| a.query.as_deref())
		.map(|q| truncate_to_width(q, 60));
	let symbol_preview = args
		.and_then(|a| a.symbol.as_deref())
		.map(|s| truncate_to_width(&sanitize_inline_text(s), 60));

	let mut target: Option<String> = None;
	if let Some(file) = args.and_then(|a| a.file.as_deref()) {
		let mut t = shorten_path(file);
		if let Some(line) = args.and_then(|a| a.line) {
			t.push_str(&format!(":{line}"));
			if let Some(sym) = &symbol_preview {
				t.push_str(&format!(" ({sym})"));
			}
		}
		target = Some(t);
	} else if let Some(line) = args.and_then(|a| a.line) {
		let mut t = format!("line {line}");
		if let Some(sym) = &symbol_preview {
			t.push_str(&format!(" ({sym})"));
		}
		target = Some(t);
	}

	let mut meta = Vec::new();
	if query_preview.is_some() && target.is_some() {
		meta.push(format!("query:{}", query_preview.as_deref().unwrap_or("")));
	}
	if let Some(new_name) = args.and_then(|a| a.new_name.as_deref()) {
		meta.push(format!("new:{new_name}"));
	}
	if let Some(apply) = args.and_then(|a| a.apply) {
		meta.push(format!("apply:{}", if apply { "true" } else { "false" }));
	}

	let subject = target.or(query_preview);
	let description = if let Some(s) = subject {
		format!("{action_label} {s}")
	} else {
		action_label
	};
	LspRequestStatus { description, meta }
}

/// Whether a line is an LSP per-file status row starting with `glyph`.
pub fn is_status_row(line: &str, glyph: &str) -> bool {
	line.starts_with(&format!("{glyph} "))
}

/// Whether result has a status row (diagnostics action or unknown).
pub fn has_status_row(action: Option<LspAction>, lines: &[String], glyph: &str) -> bool {
	if let Some(a) = action {
		if !matches!(a, LspAction::Diagnostics) {
			return false;
		}
	}
	lines.iter().any(|l| is_status_row(l, glyph))
}

/// Plain render of an LSP call.
pub fn render_call(args: Option<&LspParams>) -> String {
	let st = lsp_request_status(args, "request");
	if st.meta.is_empty() {
		format!("LSP: {}", st.description)
	} else {
		format!("LSP: {} [{}]", st.description, st.meta.join(", "))
	}
}

/// Plain render of an LSP result (collapsed form).
pub fn render_result(args: Option<&LspParams>, output: &str, is_error: bool) -> Vec<String> {
	render_result_expanded(args, output, is_error, false)
}

/// One source line beside an LSP location; the location's own line is marked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspContextLine {
	pub n: u32,
	pub text: String,
	pub marked: bool,
}

/// One definition-style or references location with numbered source context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspLocation {
	pub file: String,
	pub line: u32,
	pub col: u32,
	pub context: Vec<LspContextLine>,
}

impl LspLocation {
	pub fn position(&self) -> String {
		format!("{}:{}", self.line, self.col)
	}
}

/// A parsed `Found N definition(s):` style response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspLocations {
	/// `definition`, `type definition`, `implementation` or `reference`.
	pub noun: String,
	pub count: usize,
	pub locations: Vec<LspLocation>,
}

/// Parse the `  file:line:col` rows and their `    N: source` context.
pub fn parse_location_rows(text: &str) -> Vec<LspLocation> {
	let mut locations: Vec<LspLocation> = Vec::new();
	for line in text.lines() {
		if let Some(current) = locations.last_mut() {
			if let Some(context) = parse_context_row(line) {
				let marked = context.0 == current.line;
				current.context.push(LspContextLine { n: context.0, text: context.1, marked });
				continue;
			}
		}
		if let Some((file, line_no, col)) = parse_location_row(line) {
			locations.push(LspLocation { file, line: line_no, col, context: Vec::new() });
		}
	}
	locations
}

fn parse_location_row(line: &str) -> Option<(String, u32, u32)> {
	let body = line.strip_prefix("  ")?;
	let (rest, col) = body.rsplit_once(':')?;
	let (file, line_no) = rest.rsplit_once(':')?;
	if file.is_empty() || col.is_empty() || line_no.is_empty() {
		return None;
	}
	if !col.chars().all(|c| c.is_ascii_digit()) || !line_no.chars().all(|c| c.is_ascii_digit()) {
		return None;
	}
	Some((file.to_string(), line_no.parse().ok()?, col.parse().ok()?))
}

fn parse_context_row(line: &str) -> Option<(u32, String)> {
	let body = line.strip_prefix("    ")?;
	let (number, text) = body.split_once(':')?;
	if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
		return None;
	}
	let text = text.strip_prefix(' ').unwrap_or(text).trim_end();
	Some((number.parse().ok()?, text.to_string()))
}

/// Parse a locations/references response; `None` when the header is absent.
pub fn parse_lsp_locations(text: &str) -> Option<LspLocations> {
	let first = text.lines().next()?;
	let body = first.strip_prefix("Found ")?;
	let (count, noun) = body.split_once(' ')?;
	let noun = noun.strip_suffix("(s):")?;
	if !matches!(noun, "definition" | "type definition" | "implementation" | "reference") {
		return None;
	}
	let rest = text.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
	Some(LspLocations { noun: noun.to_string(), count: count.parse().ok()?, locations: parse_location_rows(rest) })
}

/// Group locations by file, preserving first-seen file and location order.
pub fn group_locations_by_file(locations: &[LspLocation]) -> Vec<(&str, Vec<&LspLocation>)> {
	let mut groups: Vec<(&str, Vec<&LspLocation>)> = Vec::new();
	for location in locations {
		if let Some(index) = groups.iter().position(|(file, _)| *file == location.file.as_str()) {
			groups[index].1.push(location);
		} else {
			groups.push((&location.file, vec![location]));
		}
	}
	groups
}

fn plural_count(kind: &str, n: usize) -> String {
	format!("{n} {kind}{}", if n == 1 { "" } else { "s" })
}

/// Body rows for definitions/type definitions/implementations, source marked.
pub fn render_locations(parsed: &LspLocations, expanded: bool) -> Vec<String> {
	const COLLAPSED_LOCATIONS: usize = 2;
	let limit = if expanded { parsed.locations.len() } else { COLLAPSED_LOCATIONS };
	let mut rows = Vec::new();
	for location in parsed.locations.iter().take(limit) {
		rows.push(format!("{}:{}:{}", location.file, location.line, location.col));
		let width = location.context.iter().map(|l| l.n.to_string().len()).max().unwrap_or(0);
		for context in &location.context {
			let marker = if context.marked { '*' } else { ' ' };
			rows.push(format!("  {marker}{:>width$}│{}", context.n, context.text, width = width));
		}
	}
	let hidden = parsed.locations.len().saturating_sub(limit);
	if hidden > 0 {
		rows.push(format!("… {hidden} more {}{}", parsed.noun, if hidden == 1 { "" } else { "s" }));
	}
	rows
}

/// Body rows for references grouped by file, positions right-aligned.
pub fn render_references(parsed: &LspLocations, expanded: bool) -> Vec<String> {
	const COLLAPSED_FILES: usize = 3;
	const COLLAPSED_LOCATIONS: usize = 2;
	let groups = group_locations_by_file(&parsed.locations);
	let width = parsed.locations.iter().map(|l| l.position().len()).max().unwrap_or(0);
	let file_limit = if expanded { groups.len() } else { COLLAPSED_FILES };
	let mut rows = Vec::new();
	for (file, locations) in groups.iter().take(file_limit) {
		rows.push(format!("{file} ({})", locations.len()));
		let location_limit = if expanded { locations.len() } else { COLLAPSED_LOCATIONS };
		for location in locations.iter().take(location_limit) {
			let position = format!("{:>width$}", location.position(), width = width);
			match location.context.iter().find(|l| l.marked).map(|l| l.text.trim()).filter(|s| !s.is_empty()) {
				Some(source) => rows.push(format!("  {position}│ {source}")),
				None => rows.push(format!("  {position}")),
			}
		}
		let hidden = locations.len().saturating_sub(location_limit);
		if hidden > 0 {
			rows.push(format!("  … {hidden} more {}", if hidden == 1 { "location" } else { "locations" }));
		}
	}
	let hidden_files = groups.len().saturating_sub(file_limit);
	if hidden_files > 0 {
		rows.push(format!("… {hidden_files} more {}", if hidden_files == 1 { "file" } else { "files" }));
	}
	rows
}

/// Per-file diagnostics parsing: diagnostics, server failures and warnings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LspDiagnosticsParse {
	pub diagnostics: Vec<crate::tools::native_view::ParsedDiagnostic>,
	pub failures: Vec<String>,
	pub warnings: Vec<String>,
	/// Files a server sent no report for in time; unverified, never clean.
	pub unreported: Vec<String>,
	/// Lines neither diagnostics nor status rows, kept verbatim.
	pub other: Vec<String>,
}

fn strip_status_glyph(line: &str) -> &str {
	let text = line.trim();
	if let Some(end) = text.strip_prefix('[').and_then(|rest| rest.find(']')) {
		return text[end + 2..].trim_start();
	}
	let first = text.chars().next();
	match first {
		Some(c) if !c.is_alphanumeric() && !c.is_whitespace() && !"./~\\".contains(c) => text[c.len_utf8()..].trim_start(),
		_ => text,
	}
}

fn is_summary_counts(text: &str) -> bool {
	let body = text.strip_suffix(':').unwrap_or(text);
	!body.is_empty()
		&& body.split(", ").all(|part| match part.split_once(' ') {
			Some((n, kind)) => n.chars().all(|c| c.is_ascii_digit()) && kind.ends_with("(s)"),
			None => false,
		})
}

fn is_file_summary(line: &str) -> bool {
	line.rsplit_once(": ").is_some_and(|(_, tail)| tail == "no issues" || is_summary_counts(tail))
}

fn is_diagnostic_at(line: &str) -> bool {
	let Some((line_no, rest)) = line.split_once(':') else { return false };
	let Some((col, rest)) = rest.split_once(|c: char| !c.is_ascii_digit()) else { return false };
	!line_no.is_empty()
		&& !col.is_empty()
		&& line_no.chars().all(|c| c.is_ascii_digit())
		&& !col.is_empty()
		&& rest.trim_start().starts_with('[')
}

/// Parse a diagnostics response, grouped (`# dir/`, `## file`, `  12:5 [error] …`)
/// or flat (`file:12:5 [error] …`), plus per-file status rows.
pub fn parse_lsp_diagnostics(text: &str) -> LspDiagnosticsParse {
	use crate::tools::native_view::parse_diagnostic_message;
	let mut out = LspDiagnosticsParse::default();
	let mut dir: Option<String> = None;
	let mut file: Option<String> = None;
	for raw in text.lines() {
		if let Some(path) = raw.strip_prefix("# ") {
			dir = Some(path.trim_end_matches('/').to_string());
			continue;
		}
		if let Some(path) = raw.strip_prefix("## ") {
			file = Some(match dir.as_deref() {
				Some(dir) if !dir.is_empty() => format!("{dir}/{path}"),
				_ => path.to_string(),
			});
			continue;
		}
		let line = raw.trim();
		if line.is_empty() || line == "OK" || is_summary_counts(line) || is_file_summary(line) {
			continue;
		}
		if line.contains("all language servers failed") {
			out.failures.push(strip_status_glyph(line).to_string());
		} else if line.contains("some servers failed") {
			out.warnings.push(strip_status_glyph(line).to_string());
		} else if line.contains("no diagnostics report yet") {
			out.unreported.push(strip_status_glyph(line).to_string());
		} else {
			let candidate = match &file {
				Some(file) if is_diagnostic_at(line) => format!("{file}:{line}"),
				_ => line.to_string(),
			};
			match parse_diagnostic_message(&candidate) {
				Some(diagnostic) => out.diagnostics.push(diagnostic),
				None => out.other.push(line.to_string()),
			}
		}
	}
	out
}

/// Per-file server status derived from a diagnostics status row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LspServerState {
	/// No report yet: the file is unverified.
	Pending,
	/// Every reporting server answered with the file's diagnostics.
	Ready,
	/// A server failed entirely or the file has errors.
	Error,
}

/// One parsed `<glyph> file: …` diagnostics status row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspStatusRow {
	pub state: LspServerState,
	pub file: String,
	pub servers: Vec<String>,
	pub message: String,
}

fn parenthesized_servers(text: &str) -> Vec<String> {
	let Some(start) = text.find('(') else { return Vec::new() };
	let Some(end) = text[start..].find(')') else { return Vec::new() };
	text[start + 1..start + end]
		.split(',')
		.map(str::trim)
		.filter(|s| !s.is_empty())
		.map(str::to_string)
		.collect()
}

/// Parse a per-file status row; only rows beginning with `glyph` count.
pub fn parse_status_row(line: &str, glyph: &str) -> Option<LspStatusRow> {
	let body = line.strip_prefix(&format!("{glyph} "))?.trim();
	let (file, message) = body.split_once(':').map(|(f, m)| (f.trim().to_string(), m.trim().to_string())).unwrap_or((String::new(), body.to_string()));
	let state = if body.contains("all language servers failed") {
		LspServerState::Error
	} else if body.contains("some servers failed") {
		LspServerState::Error
	} else if body.contains("no diagnostics report yet") {
		LspServerState::Pending
	} else if message == "no issues" {
		LspServerState::Ready
	} else {
		let errors: u64 = message
			.split(", ")
			.filter_map(|part| part.split_once(' '))
			.filter(|(_, kind)| kind.starts_with("error"))
			.filter_map(|(n, _)| n.parse::<u64>().ok())
			.sum();
		if errors > 0 { LspServerState::Error } else { LspServerState::Ready }
	};
	Some(LspStatusRow { state, file, servers: parenthesized_servers(body), message })
}

fn diagnostic_meta(parsed: &LspDiagnosticsParse) -> Vec<String> {
	use crate::tools::native_view::DiagnosticSeverity;
	let mut meta = Vec::new();
	for severity in [DiagnosticSeverity::Error, DiagnosticSeverity::Warning, DiagnosticSeverity::Info, DiagnosticSeverity::Hint] {
		let n = parsed.diagnostics.iter().filter(|d| d.severity == severity).count();
		if n > 0 {
			meta.push(plural_count(severity.as_str(), n));
		}
	}
	if !parsed.failures.is_empty() {
		meta.push(plural_count("server failure", parsed.failures.len()));
	}
	if !parsed.unreported.is_empty() {
		meta.push(plural_count("unverified file", parsed.unreported.len()));
	}
	let items = parsed.diagnostics.len() + parsed.failures.len() + parsed.unreported.len() + parsed.warnings.len() + parsed.other.len();
	if meta.is_empty() && items == 0 {
		meta.push("No issues".to_string());
	}
	meta
}

fn diagnostic_message(diagnostic: &crate::tools::native_view::ParsedDiagnostic) -> String {
	let source = diagnostic.source.as_deref().map(|s| format!("[{s}] ")).unwrap_or_default();
	let code = diagnostic.code.as_deref().map(|c| format!(" ({c})")).unwrap_or_default();
	format!("{source}{}{code}", diagnostic.message)
}

/// Body rows for diagnostics, failures and unverified files first.
pub fn render_diagnostics(parsed: &LspDiagnosticsParse, expanded: bool) -> Vec<String> {
	const COLLAPSED_DIAGNOSTICS: usize = 3;
	let mut items: Vec<String> = Vec::new();
	items.extend(parsed.failures.iter().cloned());
	items.extend(parsed.unreported.iter().cloned());
	items.extend(parsed.diagnostics.iter().map(|d| {
		format!("{}:{}:{} [{}] {}", d.file_path, d.line, d.col, d.severity.as_str(), diagnostic_message(d))
	}));
	items.extend(parsed.warnings.iter().cloned());
	items.extend(parsed.other.iter().cloned());
	let limit = if expanded { items.len() } else { COLLAPSED_DIAGNOSTICS };
	let mut rows: Vec<String> = items.iter().take(limit).cloned().collect();
	let hidden = items.len().saturating_sub(limit);
	if hidden > 0 {
		rows.push(format!("… {hidden} more {}", if hidden == 1 { "diagnostic" } else { "diagnostics" }));
	}
	rows
}

/// One `Symbols in …:` entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspSymbol {
	/// Kind glyph (`class`, `method`, `function`, …).
	pub icon: String,
	/// Symbol name and signature.
	pub name: String,
	/// Declaration line.
	pub line: u32,
	/// Leading whitespace of the source row; nesting is two cells per level.
	pub indent: usize,
}

/// A parsed `Symbols in <file>:` response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspSymbols {
	/// File named in the header.
	pub file: String,
	/// Symbols in response order.
	pub symbols: Vec<LspSymbol>,
}

/// Parse a `Symbols in <file>:` response. `None` when the header is absent.
pub fn parse_symbols(text: &str) -> Option<LspSymbols> {
	let file = text.lines().find_map(|line| {
		let rest = line.trim().strip_prefix("Symbols in ")?;
		let name = rest.strip_suffix(':')?;
		Some(name.to_string())
	})?;
	let mut symbols = Vec::new();
	for line in text.lines() {
		if !line.contains('@') || !line.contains("line") {
			continue;
		}
		let indent = line.len() - line.trim_start_matches(' ').len();
		let trimmed = line.trim();
		let at = trimmed.find('@')?;
		let after = trimmed[at + 1..].trim_start();
		let after = after.strip_prefix("line")?;
		let digits: String = after.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
		if digits.is_empty() {
			continue;
		}
		let head = trimmed[..at].trim();
		let (icon, name) = head.split_once(' ')?;
		symbols.push(LspSymbol {
			icon: icon.to_string(),
			name: name.trim().to_string(),
			line: digits.parse().expect("digits"),
			indent,
		});
	}
	Some(LspSymbols { file, symbols })
}

/// Body rows for a symbols response: a branch and its `line N` detail per
/// symbol, guides rails joining ancestors that are still open.
pub fn render_symbols(symbols: &LspSymbols, theme: &crate::theme::Theme, expanded: bool) -> Vec<String> {
	const COLLAPSED_SYMBOLS: usize = 3;
	let tree = theme.tree();
	let count = symbols.symbols.len();
	// `is_last[i]`: the next symbol at an indent <= mine is not a sibling.
	let mut is_last = vec![false; count];
	let mut pending: Vec<usize> = Vec::new();
	for i in (0..count).rev() {
		let my_indent = symbols.symbols[i].indent;
		while pending
			.last()
			.is_some_and(|top| symbols.symbols[*top].indent > my_indent)
		{
			pending.pop();
		}
		is_last[i] = pending
			.last()
			.map(|next| symbols.symbols[*next].indent != my_indent)
			.unwrap_or(true);
		pending.push(i);
	}
	// Per-ancestor rails: a last child releases its own rail to blank cells.
	let mut prefixes: Vec<String> = Vec::with_capacity(count);
	let mut last_at_indent: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
	for (i, sym) in symbols.symbols.iter().enumerate() {
		let mut prefix = String::new();
		let mut level = 2;
		while level <= sym.indent {
			let ancestor = last_at_indent.get(&(level - 2)).copied();
			match ancestor {
				Some(j) if is_last[j] => prefix.push_str("   "),
				_ => prefix.push_str(&format!("{}  ", tree.vertical)),
			}
			level += 2;
		}
		prefixes.push(prefix);
		last_at_indent.insert(sym.indent, i);
	}
	if !expanded {
		let shown = symbols.symbols.iter().filter(|sym| sym.indent == 0).take(COLLAPSED_SYMBOLS);
		let mut rows: Vec<String> = shown
			.map(|sym| format!("{} {} line {}", sym.icon, sym.name, sym.line))
			.collect();
		let hidden = count.saturating_sub(COLLAPSED_SYMBOLS);
		if hidden > 0 {
			rows.push(format!("… {hidden} more {}", if hidden == 1 { "symbol" } else { "symbols" }));
		}
		return rows;
	}
	let mut rows = Vec::with_capacity(count * 2);
	for (i, sym) in symbols.symbols.iter().enumerate() {
		let last = is_last[i];
		let branch = if last { &tree.last } else { &tree.branch };
		rows.push(format!("{}{} {} {}", prefixes[i], branch, sym.icon, sym.name));
		let detail = if last {
			"   ".to_string()
		} else {
			format!("{}  ", tree.vertical)
		};
		rows.push(format!("{}{}line {}", prefixes[i], detail, sym.line));
	}
	rows
}

/// Render a result, grouping locations, references and diagnostics.
pub fn render_result_expanded(args: Option<&LspParams>, output: &str, is_error: bool, expanded: bool) -> Vec<String> {
	let st = lsp_request_status(args, "response");
	if is_error {
		let mut rows = vec![format!("LSP failed: {}", st.description)];
		rows.extend(output.lines().filter(|l| !l.trim().is_empty()).map(|l| format!("  {}", l.replace('\t', " "))));
		return rows;
	}
	let text = output.trim_end();
	if text.is_empty() {
		return vec![format!("LSP: {} No result", st.description)];
	}
	let action = args.and_then(|a| a.action);
	let locations = parse_lsp_locations(text);
	let diagnostics_like = match action {
		Some(LspAction::Diagnostics) => true,
		Some(_) => text.contains("no diagnostics report yet"),
		None => {
			text.contains("all language servers failed")
				|| text.contains("no diagnostics report yet")
				|| text.lines().any(is_diagnostic_at)
		}
	};
	let (meta, body) = if let Some(parsed) = locations {
		let meta = if parsed.noun == "reference" {
			let files = group_locations_by_file(&parsed.locations).len();
			let mut meta = vec![plural_count("reference", parsed.count)];
			if files > 0 {
				meta.push(plural_count("file", files));
			}
			meta
		} else {
			vec![plural_count(&parsed.noun, parsed.count)]
		};
		let body = if parsed.noun == "reference" { render_references(&parsed, expanded) } else { render_locations(&parsed, expanded) };
		(meta, body)
	} else if let Some(symbols) = parse_symbols(text) {
		let show_file = args.and_then(|a| a.file.as_deref()).is_none();
		let mut meta = vec![plural_count("symbol", symbols.symbols.len())];
		if show_file {
			meta.push(format!("in {}", symbols.file));
		}
		let body = render_symbols(&symbols, &crate::theme::default_theme(), expanded);
		(meta, body)
	} else {
		const COLLAPSED_GENERIC_LINES: usize = 4;
		let lines: Vec<&str> = text.lines().collect();
		let limit = if expanded { lines.len() } else { COLLAPSED_GENERIC_LINES };
		let mut body: Vec<String> = lines.iter().take(limit).map(|line| line.to_string()).collect();
		let hidden = lines.len().saturating_sub(limit);
		if hidden > 0 {
			body.push(format!("… {hidden} more {}", if hidden == 1 { "line" } else { "lines" }));
		}
		(Vec::new(), body)
	};
	let suffix = if meta.is_empty() { String::new() } else { format!(" {}", meta.join(" · ")) };
	let mut rows = vec![format!("LSP: {}{suffix}", st.description)];
	rows.extend(body);
	rows
}

/// Native describe of an LSP call.
pub fn describe_call(args: Option<&LspParams>) -> crate::tools::native_view::NativeToolView {
	use crate::native::node::{NodeText, Span, Tone};
	use crate::tools::native_view::{tool_head, NativeToolView, ToolHeadDetail};
	let st = lsp_request_status(args, "request");
	let mut details = vec![ToolHeadDetail::Span(Span::styled(st.description, "accent"))];
	for m in st.meta {
		details.push(ToolHeadDetail::Span(Span::styled(m, "dim")));
	}
	NativeToolView {
		head: Some(NodeText::Spans(tool_head("LSP", &details))),
		tone: Some(Tone::Accent),
		..NativeToolView::default()
	}
}

/// Native describe of an LSP result.
pub fn describe_result(
	args: Option<&LspParams>,
	output: &str,
	is_error: bool,
) -> crate::tools::native_view::NativeToolView {
	use crate::native::describe::text as ntext;
	use crate::native::node::{NodeText, Props, Span, Tone};
	use crate::tools::native_view::{error_view, note_text, tool_head, NativeToolView, ToolHeadDetail};
	let st = lsp_request_status(args, "response");
	if is_error {
		let msg = output.trim();
		return error_view(
			"LSP",
			if msg.is_empty() { &st.description } else { msg },
			&[],
		);
	}
	let head = tool_head("LSP", &[ToolHeadDetail::Span(Span::styled(st.description, "accent"))]);
	let body = output.trim_end();
	if body.is_empty() {
		return NativeToolView {
			head: Some(NodeText::Spans(head)),
			body: Some(vec![note_text("No results", "dim", None)]),
			..NativeToolView::default()
		};
	}
	let mut p = Props::new();
	p.set_str("wrap", "word");
	NativeToolView {
		head: Some(NodeText::Spans(head)),
		body: Some(vec![ntext(NodeText::Plain(body.to_string()), p)]),
		..NativeToolView::default()
	}
}

#[cfg(test)]
mod render_describe_tests {
	use super::*;
	#[test]
	fn call_and_result_helpers() {
		let args = LspParams { action: Some(LspAction::Hover), file: Some("a.rs".into()), line: Some(2), ..Default::default() };
		let s = render_call(Some(&args));
		assert!(s.contains("hover") || s.contains("Hover"));
		let c = describe_call(Some(&args));
		assert!(c.head.is_some());
		let r = describe_result(Some(&args), "ok", false);
		assert!(r.body.is_some());
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn action_parse_label() {
		assert_eq!(LspAction::parse("type_definition"), Some(LspAction::TypeDefinition));
		assert_eq!(LspAction::TypeDefinition.label(), "type definition");
		assert_eq!(LspAction::parse("nope"), None);
	}

	#[test]
	fn request_status_with_file_symbol() {
		let p = LspParams {
			action: Some(LspAction::References),
			file: Some("src/a.ts".into()),
			line: Some(42),
			symbol: Some("validateToken".into()),
			..Default::default()
		};
		let s = lsp_request_status(Some(&p), "request");
		assert!(s.description.contains("references"));
		assert!(s.description.contains("src/a.ts:42"));
		assert!(s.description.contains("validateToken"));
	}

	#[test]
	fn format_result_strings() {
		assert_eq!(FileFormatResult::Formatted.as_str(), "formatted");
	}

	#[test]
	fn status_row_checks() {
		assert!(is_status_row("✓ src/a.ts: ok", "✓"));
		assert!(!is_status_row("has ✓ inside", "✓"));
	}
}
