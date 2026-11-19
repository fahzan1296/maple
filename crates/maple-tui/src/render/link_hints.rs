//! Keyboard link hints: tags drawn over the extension link labels
//! ([`extension_link_labels`]) on the rows a surface shows, so typing a tag
//! picks its link.
//!
//! Reference: `l/src/render/link-hints.ts`.
//!
//! The composer tags the live viewport; the fullscreen link browser tags the
//! transcript rows it scrolls to. A tag is drawn over the first cell of its
//! label, so it prefers the letters it covers: the label's first letter, then
//! its others, then the home row. Tags never prefix one another.
//!
//! # Deviations
//!
//! The reference reads the process-global `theme` inside [`LinkHints::paint`];
//! the port takes `&Theme` explicitly. `LinkHints::type` is renamed
//! [`LinkHints::type_letter`] (`type` is a Rust keyword) and returns a
//! [`TypeLetter`] instead of `ViewportLink | boolean`. `LinkHints::update`
//! takes `origin` explicitly because Rust has no default arguments.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ansi_scan::strip_ansi;
use crate::render::extension_links::{extension_link_labels, ExtensionLinkLabel};
use crate::render::render_utils::{composite_line_at, paint, slice_by_column};
use crate::text_shape::visible_width;
use crate::theme::Theme;

/// Hint tag letters, home row first: what a tag falls back to after its
/// label's own letters.
const LINK_HINT_ALPHABET: &str = "asdfghjklqwertyuiopzxcvbnm";
/// Most tags one session holds: every two-letter tag. Links past it stay
/// untagged.
const MAX_TAGS: usize = LINK_HINT_ALPHABET.len() * LINK_HINT_ALPHABET.len();

/// An extension link label drawn on a shown row: what a click or hint tag
/// lands on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewportLink {
	/// The link's URL.
	pub url: String,
	/// Shown row the label is drawn on (its first row, for a label wrapped
	/// across rows).
	pub row: usize,
	/// First cell of the label's part on that row.
	pub start: usize,
}

/// Where one shown row's part of a label starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewportLinkPart {
	/// Shown row the part is drawn on.
	pub row: usize,
	/// First cell of the part on that row.
	pub start: usize,
}

/// A label [`viewport_link_targets`] found: where it is drawn, its marker id
/// and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewportLinkTarget {
	/// The link's URL.
	pub url: String,
	/// Shown row the label starts on.
	pub row: usize,
	/// First cell of the label's first part.
	pub start: usize,
	/// The label's marker id ([`ExtensionLinkLabel::id`]).
	pub id: String,
	/// The label's text, across every shown row it wraps to; its tag prefers
	/// these letters.
	pub text: String,
	/// Where each shown row's part of the label starts, top to bottom; the
	/// first is `row`/`start`.
	pub parts: Vec<ViewportLinkPart>,
}

impl ViewportLinkTarget {
	/// The click/hint target this label's first part lands on.
	fn to_link(&self) -> ViewportLink {
		ViewportLink {
			url: self.url.clone(),
			row: self.row,
			start: self.start,
		}
	}
}

/// An extension link label tagged for picking by keyboard.
#[derive(Clone, Debug)]
struct LinkHint {
	tag: String,
	link: ViewportLinkTarget,
	/// The row each shown row part of the label is on in the whole content
	/// (shown row plus origin): with the part's first cell, marker id and URL,
	/// what tells the label apart across repaints.
	rows: Vec<usize>,
}

/// What [`LinkHints::type_letter`] answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeLetter {
	/// The whole tag is typed; the link it picks.
	Picked(ViewportLink),
	/// The letters still lead to several tags.
	Ambiguous,
	/// No tag starts with the letters.
	Miss,
}

/// What tells one row part of a label apart across repaints.
fn part_key(row: usize, start: usize, link: &ViewportLinkTarget) -> String {
	format!("{row}\0{start}\0{}\0{}", link.id, link.url)
}

/// The extension link labels drawn on `rows`, top to bottom, a wrapped label
/// once at its first row: a row's first label continues the previous row's
/// last label when that has the same marker id and ends its row.
pub fn viewport_link_targets(rows: &[String]) -> Vec<ViewportLinkTarget> {
	let mut targets: Vec<ViewportLinkTarget> = Vec::new();
	let mut previous: Option<ExtensionLinkLabel> = None;
	for (row, line) in rows.iter().enumerate() {
		let labels = extension_link_labels(line);
		for (index, label) in labels.iter().enumerate() {
			let text = strip_ansi(&slice_by_column(line, label.start, label.end - label.start, false).text);
			let continues = index == 0
				&& previous
					.as_ref()
					.is_some_and(|previous| previous.ends_row && previous.id == label.id)
				&& !targets.is_empty();
			if continues {
				let continued = targets.last_mut().expect("checked non-empty");
				continued.text.push_str(&text);
				continued.parts.push(ViewportLinkPart {
					row,
					start: label.start,
				});
			} else {
				targets.push(ViewportLinkTarget {
					url: label.url.clone(),
					row,
					start: label.start,
					id: label.id.clone(),
					text,
					parts: vec![ViewportLinkPart {
						row,
						start: label.start,
					}],
				});
			}
		}
		previous = labels.last().cloned();
	}
	targets
}

/// The letters a label's tag tries, in order: the label's own (its first one
/// first), then the alphabet.
fn tag_letters(text: &str) -> Vec<char> {
	let mut letters: Vec<char> = Vec::new();
	for letter in text.to_lowercase().chars() {
		if letter.is_ascii_lowercase() && !letters.contains(&letter) {
			letters.push(letter);
		}
	}
	for letter in LINK_HINT_ALPHABET.chars() {
		if !letters.contains(&letter) {
			letters.push(letter);
		}
	}
	letters
}

/// The first tag for a label reading `text` that keeps `used` prefix-free: a
/// single letter that is no tag and starts none, else two letters whose first
/// is no single-letter tag; `None` when none is left. With `two_letters`, only
/// two-letter tags are tried.
fn pick_tag(text: &str, used: &HashSet<String>, two_letters: bool) -> Option<String> {
	let letters = tag_letters(text);
	if !two_letters {
		let firsts: HashSet<char> = used.iter().filter_map(|tag| tag.chars().next()).collect();
		if let Some(single) = letters.iter().find(|letter| !firsts.contains(*letter)) {
			return Some(single.to_string());
		}
	}
	for first in &letters {
		if used.contains(&first.to_string()) {
			continue;
		}
		for second in &letters {
			let tag = format!("{first}{second}");
			if !used.contains(&tag) {
				return Some(tag);
			}
		}
	}
	None
}

/// The tags of one hint session and the letters typed so far.
#[derive(Clone, Debug, Default)]
pub struct LinkHints {
	hints: Option<Vec<LinkHint>>,
	typed: String,
}

impl LinkHints {
	/// An empty session: nothing tagged, nothing typed.
	pub fn new() -> Self {
		LinkHints {
			hints: None,
			typed: String::new(),
		}
	}

	/// Whether labels have been tagged since construction.
	pub fn tagged(&self) -> bool {
		self.hints.is_some()
	}

	/// Tag every label `rows` draw afresh and forget the letters typed;
	/// returns how many were tagged.
	pub fn assign(&mut self, rows: &[String]) -> usize {
		self.hints = None;
		self.typed.clear();
		self.update(rows, 0);
		self.hints.as_ref().map_or(0, Vec::len)
	}

	/// Keep the tags while `rows` still draw the tagged labels first, in the
	/// same order and columns (they may sit on other rows, and links may be
	/// added after them), following them to the rows they are on now. Returns
	/// false, keeping nothing to pick, when they no longer are.
	pub fn follow(&mut self, rows: &[String]) -> bool {
		let Some(hints) = self.hints.clone() else {
			return false;
		};
		let targets = viewport_link_targets(rows);
		let unchanged = targets.len() >= hints.len()
			&& hints
				.iter()
				.zip(targets.iter())
				.all(|(hint, target)| hint.link.url == target.url && hint.link.start == target.start);
		if !unchanged {
			self.hints = Some(Vec::new());
			return false;
		}
		self.hints = Some(
			hints
				.into_iter()
				.zip(targets)
				.map(|(hint, target)| LinkHint {
					tag: hint.tag,
					rows: hint.rows,
					link: target,
				})
				.collect(),
		);
		true
	}

	/// Tag what `rows` draw now, keeping each label's tag while it stays in
	/// view: tags of labels no longer drawn are dropped and only newly drawn
	/// labels get one, top to bottom. A label is the same across repaints when
	/// one of its row parts is: that part's row (shown row plus `origin`, so a
	/// scrolling surface passes the row its first shown row has in the whole
	/// content), first cell, marker id and URL; marker ids alone repeat across
	/// replies. Matching any part keeps a wrapped label's tag while only some
	/// of its rows stay in view.
	///
	/// The one time existing tags change: a new label finds no tag that keeps
	/// the set prefix-free (every letter is already a single-letter tag, say a
	/// 27th label joins 26). Then every label is tagged afresh with two
	/// letters, and the letters typed are forgotten.
	pub fn update(&mut self, rows: &[String], origin: usize) {
		let mut kept: HashMap<String, String> = HashMap::new();
		if let Some(hints) = &self.hints {
			for hint in hints {
				for (part, part_row) in hint.link.parts.iter().zip(hint.rows.iter()) {
					kept.insert(part_key(*part_row, part.start, &hint.link), hint.tag.clone());
				}
			}
		}
		let targets: Vec<(ViewportLinkTarget, Vec<usize>, Vec<String>)> = viewport_link_targets(rows)
			.into_iter()
			.map(|link| {
				let part_rows: Vec<usize> = link.parts.iter().map(|part| part.row + origin).collect();
				let keys: Vec<String> = link
					.parts
					.iter()
					.zip(part_rows.iter())
					.map(|(part, row)| part_key(*row, part.start, &link))
					.collect();
				(link, part_rows, keys)
			})
			.collect();
		let mut used: HashSet<String> = HashSet::new();
		let mut tags: Vec<Option<String>> = Vec::with_capacity(targets.len());
		for (_, _, keys) in &targets {
			let mut tag: Option<String> = None;
			for key in keys {
				if let Some(found) = kept.get(key) {
					if !used.contains(found) {
						tag = Some(found.to_string());
						break;
					}
				}
			}
			if let Some(tag) = &tag {
				used.insert(tag.clone());
			}
			tags.push(tag);
		}
		let mut hints: Vec<LinkHint> = Vec::new();
		let mut exhausted = false;
		for (index, (link, part_rows, _)) in targets.iter().enumerate() {
			let mut tag = tags[index].clone();
			if tag.is_none() {
				// Past the cap a label stays untagged.
				if used.len() >= MAX_TAGS {
					continue;
				}
				tag = pick_tag(&link.text, &used, false);
				if tag.is_none() {
					exhausted = true;
					break;
				}
				used.insert(tag.clone().expect("just picked"));
			}
			hints.push(LinkHint {
				tag: tag.expect("assigned above"),
				link: (*link).clone(),
				rows: part_rows.to_vec(),
			});
		}
		if exhausted {
			hints = Vec::new();
			used.clear();
			for (link, part_rows, _) in targets.iter().take(MAX_TAGS) {
				// Two letters each leave 676 prefix-free tags, so one is always
				// left.
				let tag = pick_tag(&link.text, &used, true).expect("two-letter tags always available");
				used.insert(tag.clone());
				hints.push(LinkHint {
					tag,
					link: (*link).clone(),
					rows: part_rows.to_vec(),
				});
			}
			self.typed.clear();
		}
		self.hints = Some(hints);
		if !self
			.hints
			.as_ref()
			.expect("just assigned")
			.iter()
			.any(|hint| hint.tag.starts_with(self.typed.as_str()))
		{
			self.typed.clear();
		}
	}

	/// The content moved under the tags, as when a reply above some labels is
	/// redrawn taller or shorter: `move_` maps the row a label was last drawn
	/// on in the whole content to the row it is on now, so the next
	/// [`LinkHints::update`] finds it there with its tag.
	pub fn move_rows(&mut self, move_: impl Fn(usize) -> usize) {
		if let Some(hints) = &mut self.hints {
			for hint in hints.iter_mut() {
				for row in hint.rows.iter_mut() {
					*row = move_(*row);
				}
			}
		}
	}

	/// Whether some row holds more tag cells than `width`, so its tags could
	/// not all be read side by side.
	pub fn crowded(&self, width: usize) -> bool {
		let mut cells: HashMap<usize, usize> = HashMap::new();
		for hint in self.hints.iter().flatten() {
			*cells.entry(hint.link.row).or_insert(0) += hint.tag.len();
		}
		cells.values().any(|count| *count > width)
	}

	/// Type one letter of a tag. Returns the link whose whole tag is now
	/// typed, [`TypeLetter::Ambiguous`] while the letters still lead to several
	/// tags, and [`TypeLetter::Miss`] when none starts with them. A pick or a
	/// miss forgets the letters typed.
	pub fn type_letter(&mut self, letter: &str) -> TypeLetter {
		let typed = format!("{}{}", self.typed, letter.to_lowercase());
		let matching: Vec<&LinkHint> = self
			.hints
			.iter()
			.flatten()
			.filter(|hint| hint.tag.starts_with(typed.as_str()))
			.collect();
		let picked = if matching.len() == 1 && matching[0].tag == typed {
			Some(matching[0].link.to_link())
		} else {
			None
		};
		if matching.is_empty() || picked.is_some() {
			self.typed.clear();
			return match picked {
				Some(link) => TypeLetter::Picked(link),
				None => TypeLetter::Miss,
			};
		}
		self.typed = typed;
		TypeLetter::Ambiguous
	}

	/// Draw every tag the typed letters still lead to over the start of its
	/// label, within the first `width` cells of each row (cells past it, such
	/// as a scrollbar, are kept). Tags on one row never overlap: one that
	/// would land on an earlier tag moves right, and tags that would run past
	/// `width` move left, so each stays readable as long as the row has room
	/// for them all.
	pub fn paint(&self, rows: &[String], width: usize, theme: &Theme) -> Vec<String> {
		let mut painted: Vec<String> = rows.to_vec();
		let mut by_row: BTreeMap<usize, Vec<(String, usize)>> = BTreeMap::new();
		for hint in self.hints.iter().flatten() {
			if hint.link.row >= rows.len() || !hint.tag.starts_with(self.typed.as_str()) {
				continue;
			}
			by_row
				.entry(hint.link.row)
				.or_default()
				.push((hint.tag.clone(), hint.link.start));
		}
		for (index, mut tags) in by_row {
			tags.sort_by_key(|(_, col)| *col);
			let mut free = 0usize;
			for (tag, col) in tags.iter_mut() {
				*col = (*col).max(free);
				free = *col + tag.len();
			}
			let mut limit = width;
			for (tag, col) in tags.iter_mut().rev() {
				*col = (*col).min(limit.saturating_sub(tag.len()));
				limit = *col;
			}
			let mut line = painted[index].clone();
			let line_width = width.max(visible_width(&line));
			for (tag, col) in &tags {
				// Raw SGR like the hover highlight: the theme's inverse/bold
				// drop out when it detects no color.
				let styled = format!("\x1b[1;7m{}\x1b[22;27m", paint(theme, "accent", tag));
				line = composite_line_at(&line, &styled, *col, tag.len(), line_width);
			}
			painted[index] = line;
		}
		painted
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::render::extension_links::{set_extension_links, test_lock};
	use crate::theme::builtin;
	use crate::theme::{create_theme, CreateThemeOptions};
	use std::sync::MutexGuard;

	fn use_omp_scheme() -> MutexGuard<'static, ()> {
		let guard = test_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
		set_extension_links(["omp"]);
		guard
	}

	fn marked(id: u32, url: &str, text: &str) -> String {
		format!("\x1b]8;id=omp-extension-link-{id};{url}\x07{text}\x1b]8;;\x07")
	}

	fn test_theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("embedded dark theme exists"),
			CreateThemeOptions::default(),
		)
		.expect("embedded dark theme resolves")
	}

	fn tags(hints: &LinkHints) -> Vec<(String, String)> {
		hints
			.hints
			.as_ref()
			.expect("tagged")
			.iter()
			.map(|hint| (hint.link.url.clone(), hint.tag.clone()))
			.collect()
	}

	#[test]
	fn targets_collect_labels_and_continue_a_wrapped_label() {
		let _guard = use_omp_scheme();
		let rows = vec![
			format!("see {}", marked(1, "omp://a", "Alpha")),
			format!("{} tail", marked(1, "omp://a", "Beta")),
		];
		let targets = viewport_link_targets(&rows);
		assert_eq!(targets.len(), 1);
		assert_eq!(targets[0].url, "omp://a");
		assert_eq!(targets[0].row, 0);
		assert_eq!(targets[0].start, 4);
		assert_eq!(targets[0].id, "id=omp-extension-link-1");
		assert_eq!(targets[0].text, "AlphaBeta");
		assert_eq!(
			targets[0].parts,
			vec![ViewportLinkPart { row: 0, start: 4 }, ViewportLinkPart { row: 1, start: 0 }]
		);
	}

	#[test]
	fn targets_keep_a_different_marker_id_separate() {
		let _guard = use_omp_scheme();
		let rows = vec![
			marked(1, "omp://a", "Alpha"),
			marked(2, "omp://a", "Beta"),
		];
		let targets = viewport_link_targets(&rows);
		assert_eq!(targets.len(), 2);
		assert_eq!(targets[0].text, "Alpha");
		assert_eq!(targets[1].text, "Beta");
		assert_eq!(targets[1].row, 1);
	}

	#[test]
	fn pick_tag_prefers_label_letters_then_the_alphabet() {
		let empty = HashSet::new();
		assert_eq!(pick_tag("Alpha", &empty, false).as_deref(), Some("a"));
		let used: HashSet<String> = ["a".to_string()].into_iter().collect();
		assert_eq!(pick_tag("Alpha", &used, false).as_deref(), Some("l"));
		assert_eq!(pick_tag("Alpha", &used, true).as_deref(), Some("la"));
		assert_eq!(pick_tag("Alpha", &empty, true).as_deref(), Some("aa"));
	}

	#[test]
	fn pick_tag_returns_none_once_every_tag_is_taken() {
		let all_single: HashSet<String> = LINK_HINT_ALPHABET.chars().map(|c| c.to_string()).collect();
		assert_eq!(pick_tag("Alpha", &all_single, false), None);
		let mut all_two: HashSet<String> = HashSet::new();
		for first in LINK_HINT_ALPHABET.chars() {
			for second in LINK_HINT_ALPHABET.chars() {
				all_two.insert(format!("{first}{second}"));
			}
		}
		assert_eq!(pick_tag("Alpha", &all_two, true), None);
	}

	#[test]
	fn assign_tags_are_prefix_free_and_type_picks_a_link() {
		let _guard = use_omp_scheme();
		let rows = vec![format!(
			"{} {}",
			marked(1, "omp://a", "Alpha"),
			marked(2, "omp://b", "Beta")
		)];
		let mut hints = LinkHints::new();
		assert!(!hints.tagged());
		assert_eq!(hints.assign(&rows), 2);
		assert!(hints.tagged());
		assert_eq!(
			tags(&hints),
			vec![
				("omp://a".to_string(), "a".to_string()),
				("omp://b".to_string(), "b".to_string()),
			]
		);
		assert_eq!(
			hints.type_letter("a"),
			TypeLetter::Picked(ViewportLink {
				url: "omp://a".to_string(),
				row: 0,
				start: 0,
			})
		);
		assert_eq!(hints.type_letter("x"), TypeLetter::Miss);
	}

	#[test]
	fn exhausted_session_retags_every_label_with_two_letters() {
		let _guard = use_omp_scheme();
		let mut row = String::new();
		for index in 0..27 {
			row.push_str(&marked(index + 1, &format!("omp://{index}"), "x"));
		}
		let rows = vec![row];
		let mut hints = LinkHints::new();
		assert_eq!(hints.assign(&rows), 27);
		let session = tags(&hints);
		assert!(session.iter().all(|(_, tag)| tag.len() == 2), "{session:?}");
		for (index, (first_url, first_tag)) in session.iter().enumerate() {
			for (second_url, second_tag) in session.iter().skip(index + 1) {
				assert!(!first_tag.starts_with(second_tag), "{first_tag} prefixes {second_tag}");
				assert!(!second_tag.starts_with(first_tag), "{second_tag} prefixes {first_tag}");
				assert_ne!(first_url, second_url);
			}
		}
		assert_eq!(hints.type_letter("a"), TypeLetter::Ambiguous);
	}

	#[test]
	fn max_tags_cap_leaves_extra_links_untagged() {
		let _guard = use_omp_scheme();
		let mut row = String::new();
		for index in 0..700 {
			row.push_str(&marked(index + 1, "omp://same", "x"));
		}
		let rows = vec![row];
		let mut hints = LinkHints::new();
		assert_eq!(hints.assign(&rows), MAX_TAGS);
		assert_eq!(tags(&hints).len(), MAX_TAGS);
	}

	#[test]
	fn follow_keeps_tags_while_the_tagged_prefix_holds() {
		let _guard = use_omp_scheme();
		let first = marked(1, "omp://a", "Alpha");
		let second = marked(2, "omp://b", "Beta");
		let mut hints = LinkHints::new();
		hints.assign(&[first.clone(), second.clone()]);
		let before = tags(&hints);

		assert!(hints.follow(&[String::new(), first, second]));
		assert_eq!(tags(&hints), before);

		assert!(!hints.follow(&[marked(2, "omp://b", "Beta"), marked(1, "omp://a", "Alpha")]));
		assert!(hints.tagged());
		assert!(hints.hints.as_ref().expect("still tagged").is_empty());
	}

	#[test]
	fn move_rows_keeps_tags_across_a_shifted_repaint() {
		let _guard = use_omp_scheme();
		let first = marked(1, "omp://u1", "Alpha");
		let second = marked(2, "omp://u2", "Beta");
		let rows = vec![first, second];
		let mut hints = LinkHints::new();
		hints.assign(&rows);
		let before = tags(&hints);

		// A line was inserted above: every label's whole-content row shifts down
		// one while the viewport still shows the same rows at origin 1.
		hints.move_rows(|row| row + 1);
		hints.update(&rows, 1);
		assert_eq!(tags(&hints), before);
	}

	#[test]
	fn update_without_move_reassigns_when_the_order_changes() {
		let _guard = use_omp_scheme();
		let first = marked(1, "omp://u1", "Alpha");
		let second = marked(2, "omp://u2", "Alpha");
		let rows = vec![first.clone(), second.clone()];
		let reversed = vec![second, first];

		let mut control = LinkHints::new();
		control.assign(&rows);
		assert_eq!(
			tags(&control),
			vec![
				("omp://u1".to_string(), "a".to_string()),
				("omp://u2".to_string(), "l".to_string()),
			]
		);
		control.update(&reversed, 0);
		assert_eq!(
			tags(&control),
			vec![
				("omp://u2".to_string(), "a".to_string()),
				("omp://u1".to_string(), "l".to_string()),
			]
		);
	}

	#[test]
	fn crowded_reports_rows_whose_tags_exceed_the_width() {
		let _guard = use_omp_scheme();
		let rows = vec![format!(
			"{} {}",
			marked(1, "omp://a", "Alpha"),
			marked(2, "omp://b", "Beta")
		)];
		let mut hints = LinkHints::new();
		hints.assign(&rows);
		assert!(hints.crowded(1));
		assert!(!hints.crowded(2));
	}

	#[test]
	fn paint_draws_tags_and_clamps_them_into_the_width() {
		let _guard = use_omp_scheme();
		let theme = test_theme();
		let rows = vec![format!(
			"{} {}",
			marked(1, "omp://a", "Alpha"),
			marked(2, "omp://b", "Beta")
		)];
		let mut hints = LinkHints::new();
		hints.assign(&rows);
		let painted = hints.paint(&rows, 40, &theme);
		assert_eq!(painted.len(), 1);
		assert!(painted[0].contains("\x1b[1;7m"), "{:?}", painted[0]);
		let plain = strip_ansi(&painted[0]);
        // Tags are the label's own first letters, drawn over the label start.
        assert!(plain.contains("alpha"), "{plain:?}");
        assert!(plain.contains("beta"), "{plain:?}");

		let padded = format!("{}{}", " ".repeat(10), marked(1, "omp://a", "Alpha"));
		let rows = vec![padded];
		let mut hints = LinkHints::new();
		hints.assign(&rows);
		let painted = hints.paint(&rows, 4, &theme);
		let plain = strip_ansi(&painted[0]);
		assert_eq!(plain.find('a'), Some(3), "{plain:?}");
	}
}
