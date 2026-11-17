use maple_tui::tools::lsp::{
	group_locations_by_file, parse_lsp_locations, parse_location_rows, render_locations, render_references,
	render_result_expanded, LspAction, LspParams,
};

fn definitions() -> String {
    [
	"Found 3 definition(s):",
	"  src/a.ts:42:17",
	"    41: ",
	"    42: export function validate(token: string) {",
	"    43:   return token;",
	"  src/b.ts:7:3",
	"    6: // overload",
	"    7: function validate(): void;",
	"  src/c.d.ts:1:1",
	"    1: declare function validate(): void;",
]
.join("\n")
}

#[test]
fn parses_locations_and_marks_their_source_line() {
	let parsed = parse_lsp_locations(&definitions()).expect("locations header");
	assert_eq!(parsed.noun, "definition");
	assert_eq!(parsed.count, 3);
	assert_eq!(parsed.locations.len(), 3);
	assert_eq!((parsed.locations[0].line, parsed.locations[0].col), (42, 17));
	assert_eq!(parsed.locations[0].context.len(), 3);
	assert_eq!(parsed.locations[0].context[1].n, 42);
	assert!(parsed.locations[0].context[1].marked);
	assert_eq!(parsed.locations[0].context[1].text, "export function validate(token: string) {");
	assert!(!parsed.locations[0].context[2].marked);
}

#[test]
fn collapsed_locations_show_two_and_hide_the_rest() {
	let parsed = parse_lsp_locations(&definitions()).unwrap();
	let rows = render_locations(&parsed, false);
	let text = rows.join("\n");
	assert!(text.contains("src/a.ts:42:17"));
	assert!(text.contains("  *42│export function validate(token: string) {"));
	assert!(text.contains("   43│  return token;"));
	assert!(text.contains("  *7│function validate(): void;"));
	assert!(!text.contains("src/c.d.ts"));
	assert!(text.contains("… 1 more definition"));
}

#[test]
fn expanded_locations_show_every_row() {
	let parsed = parse_lsp_locations(&definitions()).unwrap();
	let text = render_locations(&parsed, true).join("\n");
	assert!(text.contains("src/c.d.ts:1:1"));
	assert!(text.contains("*1│declare function validate(): void;"));
	assert!(!text.contains("more definition"));
}

#[test]
fn references_group_by_file_and_right_align_positions() {
	let text = [
		"Found 3 reference(s):",
		"  src/a.ts:42:14",
		"    41: ",
		"    42: export function validate(token: string) {",
		"    43:   return token;",
		"  src/b.ts:7:1",
		"    7: validate(input);",
		"  ... 1 additional reference(s) shown without context",
		"  src/b.ts:120:3",
	]
	.join("\n");
	let parsed = parse_lsp_locations(&text).unwrap();
	assert_eq!(parsed.noun, "reference");
	assert_eq!(group_locations_by_file(&parsed.locations).len(), 2);
	assert_eq!(
		render_references(&parsed, false),
		vec![
			"src/a.ts (1)",
			"  42:14│ export function validate(token: string) {",
			"src/b.ts (2)",
			"    7:1│ validate(input);",
			"  120:3",
		]
	);
	let args = LspParams {
		action: Some(LspAction::References),
		file: Some("src/a.ts".into()),
		line: Some(42),
		symbol: Some("validate".into()),
		..Default::default()
	};
	let rendered = render_result_expanded(Some(&args), &text, false, false);
	assert_eq!(rendered[0], "LSP: references src/a.ts:42 (validate) 3 references · 2 files");
}

#[test]
fn location_path_starting_with_space_stays_a_location() {
	let text = [
		"Found 2 definition(s):",
		"  src/a.ts:1:1",
		"    1: export const a = 1;",
		"   spaced.ts:3:5",
		"    3: export const spaced = 2;",
	]
	.join("\n");
	let parsed = parse_lsp_locations(&text).unwrap();
	assert_eq!(parsed.locations.len(), 2);
	assert_eq!(parsed.locations[1].file, " spaced.ts");
	assert_eq!(parse_location_rows("  src/a.ts:9:2").len(), 1);
	let rows = render_locations(&parsed, true).join("\n");
	assert!(rows.contains(" spaced.ts:3:5"));
	assert!(rows.contains("*3│export const spaced = 2;"));
}

#[test]
fn definitions_header_and_body_render_together() {
	let parsed = parse_lsp_locations(&definitions()).unwrap();
	let args = LspParams { action: Some(LspAction::Definition), file: Some("src/a.ts".into()), line: Some(42), ..Default::default() };
	let rows = render_result_expanded(Some(&args), &definitions(), false, true);
	assert_eq!(rows[0], "LSP: definition src/a.ts:42 3 definitions");
	assert_eq!(parsed.locations.len(), 3);
	assert!(rows.iter().any(|row| row == "src/c.d.ts:1:1"));
}
