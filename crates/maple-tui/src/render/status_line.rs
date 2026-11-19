//! Standardized status header rendering for tool output.
//!
//! Reference: `l/src/render/status-line.ts`.
//!
//! One bordered header row: an optional status icon (or a pre-rendered
//! override), a colored title, an optional `: description`, an optional
//! bracketed badge, and dot-separated metadata.
//!
//! # Deviations
//!
//! The reference reads the process-global `theme`; the port takes `&Theme`
//! explicitly, like every other renderer in this crate.

use std::sync::LazyLock;

use regex::Regex;

use crate::render::render_utils::{format_status_icon, paint, ToolUiStatus};
use crate::theme::{Theme, ThemeColor};

/// Flatten CR/LF runs in caller-supplied header fragments so a single newline
/// embedded in a description or metadata cannot expand the status line into
/// multiple rows — which would otherwise break the bordered output block the
/// header sits on. Tab characters are left alone; tool renderers that need
/// tab-safe text run `replace_tabs` themselves.
static NEWLINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\r\n?|\n").expect("valid regex"));

/// A bracketed badge drawn after the description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBadge {
	/// Badge text, without brackets.
	pub label: String,
	/// Badge color token.
	pub color: ThemeColor,
}

/// Status icon, title, description, and metadata for a single header row.
#[derive(Clone, Debug, Default)]
pub struct StatusLineOptions {
	/// Status glyph to draw, unless [`Self::icon_override`] is set.
	pub icon: Option<ToolUiStatus>,
	/// Pre-rendered glyph that replaces the status icon (e.g. a magnifier for
	/// search-family tools). Takes precedence over [`Self::icon`].
	pub icon_override: Option<String>,
	/// Spinner frame for [`ToolUiStatus::Running`].
	pub spinner_frame: Option<usize>,
	/// Header title.
	pub title: String,
	/// Title color; defaults to `accent`.
	pub title_color: Option<ThemeColor>,
	/// Optional `: description` suffix.
	pub description: Option<String>,
	/// Optional bracketed badge.
	pub badge: Option<StatusBadge>,
	/// Optional dot-separated metadata; blank entries are dropped.
	pub meta: Option<Vec<String>>,
}

fn flatten_for_header(text: &str) -> String {
	NEWLINES.replace_all(text, " ").into_owned()
}

/// Render a single-line status header, flattening multiline fields.
pub fn render_status_line(options: &StatusLineOptions, theme: &Theme) -> String {
	let icon = options.icon_override.clone().unwrap_or_else(|| {
		options
			.icon
			.map(|status| format_status_icon(status, theme, options.spinner_frame))
			.unwrap_or_default()
	});
	let title_color = options.title_color.unwrap_or(ThemeColor::Accent);
	let title = paint(theme, title_color, &flatten_for_header(&options.title));
	let mut line = if icon.is_empty() {
		title
	} else {
		format!("{icon} {title}")
	};

	if let Some(description) = &options.description {
		if !description.is_empty() {
			line.push_str(": ");
			line.push_str(&paint(theme, "muted", &flatten_for_header(description)));
		}
	}

	if let Some(badge) = &options.badge {
		let bracketed = format!(
			"{}{}{}",
			theme.format().bracket_left,
			flatten_for_header(&badge.label),
			theme.format().bracket_right
		);
		line.push(' ');
		line.push_str(&paint(theme, badge.color, &bracketed));
	}

	let meta: Vec<String> = options
		.meta
		.as_ref()
		.map(|entries| {
			entries
				.iter()
				.map(|entry| flatten_for_header(entry))
				.filter(|entry| !entry.trim().is_empty())
				.collect()
		})
		.unwrap_or_default();
	if !meta.is_empty() {
		line.push(' ');
		line.push_str(&paint(theme, "dim", &meta.join(&theme.sep().dot)));
	}

	line
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::{create_theme, CreateThemeOptions};

	fn test_theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("embedded dark theme exists"),
			CreateThemeOptions::default(),
		)
		.expect("embedded dark theme resolves")
	}

	#[test]
	fn renders_icon_title_and_description() {
		let theme = test_theme();
		let line = render_status_line(
			&StatusLineOptions {
				icon: Some(ToolUiStatus::Success),
				title: "Build".to_string(),
				description: Some("finished".to_string()),
				..StatusLineOptions::default()
			},
			&theme,
		);
		assert!(line.contains(theme.symbol("status.success")), "{line:?}");
		assert!(line.contains("Build"), "{line:?}");
        assert!(line.contains(": "), "{line:?}");
        assert!(line.contains("finished"), "{line:?}");
	}

	#[test]
	fn icon_override_takes_precedence_over_the_status_icon() {
		let theme = test_theme();
		let line = render_status_line(
			&StatusLineOptions {
				icon: Some(ToolUiStatus::Success),
				icon_override: Some("MAG".to_string()),
				title: "Search".to_string(),
				..StatusLineOptions::default()
			},
			&theme,
		);
		assert!(line.contains("MAG"), "{line:?}");
		assert!(!line.contains(theme.symbol("status.success")), "{line:?}");
	}

	#[test]
	fn running_icon_uses_the_spinner_frame() {
		let theme = test_theme();
		let frames = theme.spinner_frames();
		assert!(!frames.is_empty());
		let line = render_status_line(
			&StatusLineOptions {
				icon: Some(ToolUiStatus::Running),
				spinner_frame: Some(0),
				title: "Working".to_string(),
				..StatusLineOptions::default()
			},
			&theme,
		);
		assert!(line.contains(frames[0].as_str()), "{line:?}");
	}

	#[test]
	fn flattens_newlines_in_every_field() {
		let theme = test_theme();
		let line = render_status_line(
			&StatusLineOptions {
				title: "a\nb".to_string(),
				description: Some("c\r\nd".to_string()),
				badge: Some(StatusBadge {
					label: "e\rf".to_string(),
					color: ThemeColor::Success,
				}),
				meta: Some(vec!["g\nh".to_string()]),
				..StatusLineOptions::default()
			},
			&theme,
		);
		assert!(!line.contains('\n') && !line.contains('\r'), "{line:?}");
		for fragment in ["a b", "c d", "e f", "g h"] {
			assert!(line.contains(fragment), "{line:?} missing {fragment}");
		}
	}

	#[test]
	fn badge_uses_the_theme_brackets() {
		let theme = test_theme();
		let line = render_status_line(
			&StatusLineOptions {
				title: "Build".to_string(),
				badge: Some(StatusBadge {
					label: "done".to_string(),
					color: ThemeColor::Success,
				}),
				..StatusLineOptions::default()
			},
			&theme,
		);
		let bracketed = format!("{}done{}", theme.format().bracket_left, theme.format().bracket_right);
		assert!(line.contains(bracketed.as_str()), "{line:?}");
	}

	#[test]
	fn metadata_is_dot_separated_and_blanks_are_dropped() {
		let theme = test_theme();
		let line = render_status_line(
			&StatusLineOptions {
				title: "Build".to_string(),
				meta: Some(vec!["one".to_string(), "   ".to_string(), "two".to_string()]),
				..StatusLineOptions::default()
			},
			&theme,
		);
		assert!(
			line.contains(format!("one{}two", theme.sep().dot).as_str()),
			"{line:?}"
		);
		assert!(!line.contains("   "), "{line:?}");
	}

	#[test]
	fn empty_description_is_omitted() {
		let theme = test_theme();
		let line = render_status_line(
			&StatusLineOptions {
				title: "Build".to_string(),
				description: Some(String::new()),
				..StatusLineOptions::default()
			},
			&theme,
		);
		assert!(!line.contains(": "), "{line:?}");
		assert_eq!(line, paint(&theme, ThemeColor::Accent, "Build"));
	}

	#[test]
	fn title_color_defaults_to_accent() {
		let theme = test_theme();
		let options = StatusLineOptions {
			title: "Build".to_string(),
			..StatusLineOptions::default()
		};
		assert_eq!(
			render_status_line(&options, &theme),
			render_status_line(
				&StatusLineOptions {
					title_color: Some(ThemeColor::Accent),
					..options.clone()
				},
				&theme
			)
		);
	}
}
