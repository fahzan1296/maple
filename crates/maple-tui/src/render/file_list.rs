//! File listings with icons, metadata, and optional links.
//!
//! Reference: l/src/render/file-list.ts.
//!
//! Deviation: the reference's `hyperlinkFn` is an injected callback; this port
//! types it as [`FileHyperlinkFn`] so callers keep the same control.
//! `describeFileList` takes no theme (its spans carry semantic tokens), and it
//! links through the process-wide policy instead of a callback. Both link paths
//! delegate to `hyperlink::file_link_span`, the port of `fileLinkSpan`.

use crate::lang_from_path::get_language_from_path;
use crate::native::describe::{item, list, span};
use crate::native::node::{NativeNode, Props};
use crate::render::hyperlink::file_link_span;
use crate::render::render_utils::{format_styled_path, paint};
use crate::render::tree_list::{render_tree_list, TreeListOptions, TruncateFrom};
use crate::render::types::TreeContext;
use crate::theme::{Theme, ThemeColor};

/// An injectable file-path linker: absolute path, display text → linked text.
pub type FileHyperlinkFn = fn(&str, &str) -> String;

/// A file-list item with optional display metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileListEntry {
	/// Path shown to the user; also the icon/language lookup key.
	pub path: String,
	/// Absolute filesystem path; when set, the path text carries an OSC 8 link.
	pub abs_path: Option<String>,
	/// Directory override; defaults to whether `path` ends with `/`.
	pub is_directory: Option<bool>,
	/// Optional trailing detail, drawn dim.
	pub meta: Option<String>,
}

/// Files and presentation options for a tree-style listing.
pub struct FileListOptions<'a> {
	/// Entries to render, in display order.
	pub files: &'a [FileListEntry],
	/// Show every row.
	pub expanded: bool,
	/// Maximum rows when collapsed.
	pub max_collapsed: usize,
	/// Prefix each row with its language/folder icon.
	pub show_icons: bool,
	/// Optional linker invoked with the entry's absolute path.
	pub hyperlink_fn: Option<FileHyperlinkFn>,
}

impl<'a> Default for FileListOptions<'a> {
	fn default() -> Self {
		Self {
			files: &[],
			expanded: false,
			max_collapsed: 8,
			show_icons: true,
			hyperlink_fn: None,
		}
	}
}

/// Render file entries as a themed tree listing.
pub fn render_file_list(options: &FileListOptions, theme: &Theme) -> Vec<String> {
	let tree_options = TreeListOptions {
		items: options.files.iter().collect::<Vec<&FileListEntry>>(),
		expanded: options.expanded,
		max_collapsed: options.max_collapsed,
		max_collapsed_lines: None,
		item_type: "file".to_string(),
		truncate_from: TruncateFrom::End,
		trailing_summary: None,
		render_item: |entry: &&FileListEntry, _ctx: &TreeContext| {
			file_label(entry, options.show_icons, options.hyperlink_fn, theme)
		},
	};
	render_tree_list(&tree_options, theme)
}

/// One entry as `${iconPrefix}${linkedPath}${meta}`.
fn file_label(
	entry: &FileListEntry,
	show_icons: bool,
	hyperlink_fn: Option<FileHyperlinkFn>,
	theme: &Theme,
) -> String {
	let is_directory = entry
		.is_directory
		.unwrap_or_else(|| entry.path.ends_with('/'));
	let lang = if is_directory {
		None
	} else {
		get_language_from_path(&entry.path)
	};
	let icon = if !show_icons {
		String::new()
	} else if is_directory {
		paint(theme, ThemeColor::Muted, theme.symbol("icon.folder"))
	} else {
		paint(theme, ThemeColor::Muted, &theme.get_lang_icon(lang))
	};
	let icon_prefix = match icon.is_empty() {
		true => String::new(),
		false => format!("{icon} "),
	};
	let meta = match entry.meta.as_deref() {
		Some(meta) => format!(" {}", paint(theme, ThemeColor::Dim, meta)),
		None => String::new(),
	};
	let path_str = format_styled_path(&entry.path, theme);
	let linked = match (entry.abs_path.as_deref(), hyperlink_fn) {
		(Some(abs_path), Some(link)) => link(abs_path, &path_str),
		_ => path_str,
	};
	format!("{icon_prefix}{linked}{meta}")
}

/// The native form of [`render_file_list`]: a `list` of path-keyed `item`s with
/// language/folder icons and the meta as dim detail.
pub fn describe_file_list(options: &FileListOptions) -> NativeNode {
	let mut items = Vec::with_capacity(options.files.len());
	for entry in options.files {
		let is_directory = entry
			.is_directory
			.unwrap_or_else(|| entry.path.ends_with('/'));
		let style = if is_directory { "path accent" } else { "path" };
		let label = match entry.abs_path.as_deref() {
			Some(abs_path) => file_link_span(abs_path, &entry.path, style),
			None => span(entry.path.as_str(), Some(style)),
		};
		let mut props = Props::new();
		props.insert("label", serde_json::Value::Array(vec![label.to_value()]));
		if options.show_icons {
			let icon = if is_directory {
				"folder".to_string()
			} else {
				get_language_from_path(&entry.path)
					.unwrap_or("file")
					.to_string()
			};
			props.insert("icon", serde_json::Value::String(icon));
		}
		if let Some(meta) = entry.meta.as_deref() {
			props.insert(
				"detail",
				serde_json::Value::Array(vec![span(meta, Some("dim")).to_value()]),
			);
		}
		items.push(item(entry.path.clone(), props));
	}

	let mut props = Props::new();
	if !options.expanded {
		props.insert("max", serde_json::json!({ "lines": options.max_collapsed }));
	}
	list(items, props, None)
}


#[cfg(test)]
mod tests {
	use super::*;
	use crate::native::node::NodeKind;
	use crate::native::spans::plain_text;

	fn theme() -> Theme {
		crate::render::render_utils::tests::sample_theme()
	}

	fn visible(text: &str) -> String {
		plain_text(text)
	}

	fn gutter(theme: &Theme, glyph: &str) -> String {
		format!("{} ", paint(theme, ThemeColor::Dim, glyph))
	}

	fn entries() -> Vec<FileListEntry> {
		vec![
			FileListEntry {
				path: "src/main.rs".to_string(),
				abs_path: None,
				is_directory: None,
				meta: Some("12 lines".to_string()),
			},
			FileListEntry {
				path: "docs/".to_string(),
				..FileListEntry::default()
			},
			FileListEntry {
				path: "README.md".to_string(),
				abs_path: Some("/home/user/maple/README.md".to_string()),
				is_directory: Some(false),
				meta: None,
			},
		]
	}

	#[test]
	fn file_rows_carry_icon_path_and_meta() {
		let theme = theme();
		let entries = entries();
		let options = FileListOptions {
			files: &entries,
			..FileListOptions::default()
		};
		let rows = render_file_list(&options, &theme);

		assert_eq!(rows.len(), 3);
		assert_eq!(visible(&rows[0]), "├─ 🦀 src/main.rs 12 lines");
		assert_eq!(visible(&rows[1]), "├─ 📁 docs/");
		assert_eq!(visible(&rows[2]), "└─ 📝 README.md");
		assert_eq!(
			rows[2],
			format!(
				"{}{} {}",
				gutter(&theme, "└─"),
				paint(&theme, ThemeColor::Muted, "📝"),
				format_styled_path("README.md", &theme)
			)
		);
	}

	#[test]
	fn icons_can_be_hidden() {
		let theme = theme();
		let entries = entries();
		let options = FileListOptions {
			files: &entries,
			show_icons: false,
			..FileListOptions::default()
		};
		let rows = render_file_list(&options, &theme);

		assert_eq!(visible(&rows[0]), "├─ src/main.rs 12 lines");
		assert_eq!(visible(&rows[1]), "├─ docs/");
	}

	#[test]
	fn hyperlink_callback_wraps_the_path() {
		let theme = theme();
		let mut entries = entries();
		entries[0].abs_path = Some("/repo/src/main.rs".to_string());
		let options = FileListOptions {
			files: &entries,
			hyperlink_fn: Some(|abs_path, text| format!("<{abs_path}>{text}")),
			..FileListOptions::default()
		};
		let rows = render_file_list(&options, &theme);

		let row = visible(&rows[0]);
		assert!(row.contains("<"), "row was {row}");
		assert!(row.contains("/repo/src/main.rs"), "row was {row}");
	}

	#[test]
	fn described_list_keys_items_by_path() {
		let entries = entries();
		let options = FileListOptions {
			files: &entries,
			..FileListOptions::default()
		};
		let node = describe_file_list(&options);

		assert_eq!(node.kind, NodeKind::List);
		assert_eq!(node.children.len(), 3);
		assert_eq!(node.children[0].key.as_deref(), Some("src/main.rs"));
		assert_eq!(node.children[1].key.as_deref(), Some("docs/"));

		let label = node.children[0].prop("label").expect("label").as_array().expect("spans");
		assert_eq!(label[0]["t"].as_str(), Some("src/main.rs"));
		assert_eq!(label[0]["s"].as_str(), Some("path"));
		assert_eq!(
			node.children[0].prop("icon").and_then(|value| value.as_str()),
			Some("rust")
		);
		let detail = node.children[0]
			.prop("detail")
			.expect("detail")
			.as_array()
			.expect("spans");
		assert_eq!(detail[0]["t"].as_str(), Some("12 lines"));
		assert_eq!(detail[0]["s"].as_str(), Some("dim"));

		let dir_label = node.children[1]
			.prop("label")
			.expect("label")
			.as_array()
			.expect("spans");
		assert_eq!(dir_label[0]["s"].as_str(), Some("path accent"));
		assert_eq!(
			node.children[1].prop("icon").and_then(|value| value.as_str()),
			Some("folder")
		);

		let max = node.prop("max").expect("max");
		assert_eq!(max["lines"].as_u64(), Some(8));
	}

	#[test]
	fn described_list_is_unbounded_when_expanded() {
		let entries = entries();
		let options = FileListOptions {
			files: &entries,
			expanded: true,
			..FileListOptions::default()
		};
		let node = describe_file_list(&options);

		assert!(node.prop("max").is_none());
	}

	#[test]
	fn unknown_extension_reports_the_generic_file_icon() {
		let entries = vec![FileListEntry {
			path: "notes.zzz".to_string(),
			..FileListEntry::default()
		}];
		let options = FileListOptions {
			files: &entries,
			..FileListOptions::default()
		};
		let node = describe_file_list(&options);

		assert_eq!(
			node.children[0].prop("icon").and_then(|value| value.as_str()),
			Some("file")
		);
	}

	#[test]
	fn link_span_keeps_display_text_and_style() {
		let linked = file_link_span("/repo/src/main.rs", "src/main.rs", "path accent");

		assert_eq!(linked.t, "src/main.rs");
		assert_eq!(linked.s.as_deref(), Some("path accent"));
	}
}
