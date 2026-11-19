//! Reference: `l/src/overlays/plan-review-overlay.ts`.
//!
//! Fullscreen plan-review overlay. The plan is split into sections (preamble +
//! one per heading), each rendered through its own markdown renderer and
//! windowed by a scroll offset, while the approval options (plus the optional
//! model-tier slider) sit beneath inside the same outlined box.
//!
//! When the terminal is wide enough and the plan has two or more headings, a
//! Contents sidebar appears: it tracks the scrolled section with an accent
//! glow and, when focused, lets the operator jump between sections, delete one
//! (with undo) and annotate sections with feedback that feeds the Refine loop.
//!
//! Deviations:
//! - The reference drives a `ScrollView` with SGR mouse hit-testing and hover
//!   bands; this port keeps a plain scroll offset and routes rows through
//!   native `select`/`activate` events, so there is no mouse click routing.
//! - The annotation draft is a [`crate::components::Editor`]; the
//!   annotation-chooser overlay (several notes on one body row) edits the first
//!   matching note instead of showing its own list.
//! - `onAnnotationExternalEditor` and `onCopyPlan` are host callbacks; the
//!   editor round-trip goes through [`PlanReviewOverlay::set_plan_content`].
//! - Section content hashes come from a stable FNV-1a hash (the reference uses
//!   `Bun.hash`), so restored annotations also fall back to index + title.
//! - The process-global theme and keybindings are injected.

use std::collections::{HashMap, HashSet};

use crate::chrome::keybinding_hints::{editor_key, editor_keys, interrupt_key};
use crate::chrome::overlay_box::{
	bottom_border, divider, divider_split, row as box_row, split_body_width, split_row, top_border,
	top_border_split, OverlayTheme,
};
use crate::chrome::segment_track::{TrackSegment, render_segment_track};
use crate::component::Component;
use crate::components::editor::Editor;
use crate::components::markdown::Markdown;
use crate::key_hint_format::KeyHintFormatter;
use crate::keybindings::KeybindingsManager;
use crate::keys::Key;
use crate::native::backend::NativeUiEvent;
use crate::native::describe::{col, md, node, span, text};
use crate::native::node::{DescribeContext, NativeNode, NodeKind, NodeText, Props};
use crate::native::overlay::{
	ActionButtonOptions, ActionButtonTone, action_bar, action_button, action_hint, hints_row, select_list,
};
use crate::overlays::hook_selector::HookSelectorSlider;
use crate::overlays::plan_toc::{PlanSection, join_plan_sections, parse_plan_sections, section_deletion_span};
use crate::render::render_utils::replace_tabs;
use crate::text_shape::{Ellipsis, truncate_to_width, visible_width};
use crate::theme::Theme;

/// Title shown in the overlay's top border.
pub const OVERLAY_TITLE: &str = "Plan Review";
/// Native sheet title when the plan has no title heading of its own.
pub const NATIVE_TITLE: &str = "Plan review";
/// Role of the native sheet.
pub const PLAN_REVIEW_ROLE: &str = "omp.overlay.plan-review";
/// Minimum plan-body rows kept visible even on short terminals.
pub const MIN_BODY_ROWS: usize = 3;
/// Visible rows for the in-overlay annotation editor before it scrolls.
pub const MAX_ANNOTATION_EDITOR_ROWS: usize = 6;
/// Sidebar gate: minimum headings.
pub const SIDEBAR_MIN_HEADINGS: usize = 2;
/// Sidebar gate: minimum terminal width.
pub const SIDEBAR_MIN_TOTAL_WIDTH: usize = 64;
/// Sidebar gate: minimum usable body column.
pub const SIDEBAR_MIN_BODY_WIDTH: usize = 40;
/// Persisted line-context cap.
pub const MAX_ANNOTATION_CONTEXT_WIDTH: usize = 120;
/// Oldest undo snapshots are dropped past this depth.
pub const MAX_UNDO_ENTRIES: usize = 100;
/// Footer separator between hint clauses.
pub const HINT_SEPARATOR: &str = " · ";

/// Focus regions, cycled with Tab/Shift+Tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlanReviewFocus {
	/// The Contents sidebar.
	Toc,
	/// The plan body.
	Body,
	/// The approval options.
	#[default]
	Actions,
}

impl PlanReviewFocus {
	/// The focus id as the wire spells it.
	pub fn as_str(self) -> &'static str {
		match self {
			PlanReviewFocus::Toc => "toc",
			PlanReviewFocus::Body => "body",
			PlanReviewFocus::Actions => "actions",
		}
	}
}

/// What an annotation is attached to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnotationTarget {
	/// The whole section.
	Section,
	/// One rendered line of the section.
	Line {
		/// Rendered row within the section.
		row: usize,
		/// Caption of the anchored line.
		context: String,
		/// Whether the caption was truncated.
		context_truncated: bool,
	},
}

/// One annotation on the plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayAnnotation {
	/// The operator's note.
	pub note: String,
	/// What the note is attached to.
	pub target: AnnotationTarget,
}

/// A section of the plan under review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlaySection {
	/// `0` = preamble, `1..=6` = heading depth.
	pub level: usize,
	/// Heading label (empty for the preamble).
	pub title: String,
	/// Exact source slice.
	pub raw: String,
	/// Annotations on this section, in insertion order.
	pub annotations: Vec<OverlayAnnotation>,
}

impl OverlaySection {
	/// Build a bare section from the parsed plan.
	pub fn from_plan_section(section: PlanSection) -> Self {
		OverlaySection { level: section.level, title: section.title, raw: section.raw, annotations: Vec::new() }
	}
}

/// Section identity retained between overlays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SerializedSection {
	/// Index in the parsed plan.
	pub index: usize,
	/// Heading title.
	pub title: String,
	/// Heading ancestry from the document root.
	pub path: Vec<String>,
	/// Hash of the section source.
	pub content_hash: Option<u64>,
}

/// One serialized annotation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedAnnotation {
	/// The section the note belongs to.
	pub section: SerializedSection,
	/// What the note is attached to.
	pub target: AnnotationTarget,
	/// The operator's note.
	pub note: String,
}

/// Serializable annotations retained by the plan-review owner between overlays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlanReviewAnnotationState {
	/// Every retained annotation.
	pub annotations: Vec<SerializedAnnotation>,
}

/// Host callbacks of [`PlanReviewOverlay`].
#[derive(Default)]
pub struct PlanReviewOverlayCallbacks {
	/// A choice was made; the label is never a disabled one.
	pub on_pick: Option<Box<dyn FnMut(&str)>>,
	/// Esc was pressed.
	pub on_cancel: Option<Box<dyn Fn()>>,
	/// The copy hotkey was pressed with the current full plan text.
	pub on_copy_plan: Option<Box<dyn FnMut(&str)>>,
	/// The external-editor key was pressed.
	pub on_external_editor: Option<Box<dyn FnMut()>>,
	/// The plan text changed through an in-overlay delete/undo.
	pub on_plan_edited: Option<Box<dyn FnMut(&str)>>,
	/// The Refine feedback markdown changed.
	pub on_feedback_change: Option<Box<dyn FnMut(&str)>>,
	/// The annotation snapshot changed.
	pub on_annotation_state_change: Option<Box<dyn FnMut(&PlanReviewAnnotationState)>>,
}

/// Construction options of [`PlanReviewOverlay`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanReviewOverlayOptions {
	/// Prompt rendered above the options.
	pub prompt_title: Option<String>,
	/// The approval option labels.
	pub options: Vec<String>,
	/// Indices into `options` that render dimmed and cannot be selected.
	pub disabled_indices: Vec<usize>,
	/// Trailing footer hint (cancel hint).
	pub help_text: Option<String>,
	/// Initially highlighted option index.
	pub initial_index: Option<usize>,
	/// Optional model-tier slider between the plan body and the options.
	pub slider: Option<HookSelectorSlider>,
	/// Display label for the external-editor key.
	pub external_editor_label: Option<String>,
	/// The external editor's name for the native Edit button.
	pub external_editor_name: Option<String>,
	/// Annotations restored into this overlay instance.
	pub annotation_state: Option<PlanReviewAnnotationState>,
}

/// A rendered line's caption, used to re-locate line anchors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineAnchorContext {
	/// Caption text.
	pub text: String,
	/// Whether the caption was truncated to the context cap.
	pub truncated: bool,
}

/// The caption for one rendered line.
pub fn line_context(line: &str) -> LineAnchorContext {
	let sanitized = replace_tabs(line);
	let truncated = visible_width(&sanitized) > MAX_ANNOTATION_CONTEXT_WIDTH;
	let text = truncate_to_width(&sanitized, MAX_ANNOTATION_CONTEXT_WIDTH, Ellipsis::Unicode);
	let text = if text.is_empty() { "(blank line)".to_string() } else { text };
	LineAnchorContext { text, truncated }
}

/// Normalize a caption for matching.
pub fn normalize_line_context(context: &LineAnchorContext) -> String {
	let normalized = context.text.split_whitespace().collect::<Vec<_>>().join(" ");
	if context.truncated {
		normalized.trim_end_matches('…').to_string()
	} else {
		normalized
	}
}

/// Row in `contexts` nearest `stored_row` whose text matches the stored
/// context.
pub fn resolve_line_row(stored_row: usize, stored: &LineAnchorContext, contexts: &[LineAnchorContext]) -> Option<usize> {
	if contexts.is_empty() {
		return None;
	}
	let normalized_stored = normalize_line_context(stored);
	if normalized_stored.is_empty() {
		return None;
	}
	let mut best: Option<usize> = None;
	let mut best_distance = usize::MAX;
	for (row, context) in contexts.iter().enumerate() {
		let normalized = normalize_line_context(context);
		if normalized != normalized_stored
			&& !normalized.contains(&normalized_stored)
			&& !normalized_stored.contains(&normalized)
		{
			continue;
		}
		let distance = row.abs_diff(stored_row);
		if distance < best_distance {
			best = Some(row);
			best_distance = distance;
		}
	}
	best
}

/// One annotation note formatted for the Refine feedback markdown.
pub fn format_annotation_feedback(note: &str) -> String {
	if !note.contains('\n') {
		return format!("- {note}\n");
	}
	let fence = markdown_fence_for(note);
	format!("{fence}md\n{note}\n{fence}\n")
}

/// A backtick fence long enough to wrap `text` without closing early.
pub fn markdown_fence_for(text: &str) -> String {
	let mut fence = "```".to_string();
	while text.contains(&fence) {
		fence.push('`');
	}
	fence
}

/// Stable content hash of a section (`len:fnv1a`).
pub fn section_content_hash(section: &OverlaySection) -> u64 {
	let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
	for byte in section.raw.as_bytes() {
		hash ^= u64::from(*byte);
		hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
	}
	hash
}

/// A row rendered from the plan body, mapped back to its anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyRowAnchor {
	/// Section the row belongs to.
	pub section_index: usize,
	/// Rendered row within the section.
	pub row: usize,
	/// Caption of the row.
	pub context: String,
	/// Whether the caption was truncated.
	pub context_truncated: bool,
}

/// Undo snapshot: joined plan text, annotations aligned by section, and the
/// accumulated deleted-section feedback at the time of the snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UndoEntry {
	/// Joined plan text.
	pub text: String,
	/// Annotations per section.
	pub annotations: Vec<Vec<OverlayAnnotation>>,
	/// Deleted section titles.
	pub deleted: Vec<String>,
}

/// Fullscreen plan-review overlay.
pub struct PlanReviewOverlay {
	sections: Vec<OverlaySection>,
	toc: Vec<usize>,
	title_index: Option<usize>,
	toc_base_level: usize,
	section_offsets: Vec<usize>,
	body_anchors: Vec<BodyRowAnchor>,
	body_lines: Vec<String>,
	body_width: usize,
	scroll_offset: usize,
	undo: Vec<UndoEntry>,
	deleted: Vec<String>,
	options: Vec<String>,
	disabled: HashSet<usize>,
	help_suffix: Option<String>,
	external_editor_label: Option<String>,
	external_editor_name: Option<String>,
	prompt_title: Option<String>,
	selected_index: Option<usize>,
	slider: Option<HookSelectorSlider>,
	slider_index: usize,
	focus: PlanReviewFocus,
	toc_cursor: usize,
	committed: Option<String>,
	annotating: bool,
	annotate_target: Option<(usize, AnnotationTarget)>,
	editing_annotation: Option<(usize, usize)>,
	editor: Editor,
	callbacks: PlanReviewOverlayCallbacks,
	theme: Theme,
	bindings: KeybindingsManager,
	panel_theme: OverlayTheme,
}

impl PlanReviewOverlay {
	/// A review overlay for `plan_content` with `options`.
	pub fn new(
		theme: Theme,
		bindings: KeybindingsManager,
		plan_content: &str,
		options: PlanReviewOverlayOptions,
		callbacks: PlanReviewOverlayCallbacks,
	) -> Self {
		let disabled: HashSet<usize> = options
			.disabled_indices
			.into_iter()
			.filter(|index| *index < options.options.len())
			.collect();
		let mut overlay = PlanReviewOverlay {
			sections: Vec::new(),
			toc: Vec::new(),
			title_index: None,
			toc_base_level: 1,
			section_offsets: Vec::new(),
			body_anchors: Vec::new(),
			body_lines: Vec::new(),
			body_width: 0,
			scroll_offset: 0,
			undo: Vec::new(),
			deleted: Vec::new(),
			options: options.options,
			disabled,
			help_suffix: options.help_text,
			external_editor_label: options.external_editor_label,
			external_editor_name: options.external_editor_name,
			prompt_title: options.prompt_title,
			selected_index: None,
			slider: options.slider.filter(|slider| !slider.segments.is_empty()),
			slider_index: 0,
			focus: PlanReviewFocus::Actions,
			toc_cursor: 0,
			committed: None,
			annotating: false,
			annotate_target: None,
			editing_annotation: None,
			editor: Editor::new(),
			callbacks,
			theme,
			bindings,
			panel_theme: OverlayTheme::default(),
		};
		if let Some(slider) = overlay.slider.clone() {
			overlay.slider_index = slider.index.min(slider.segments.len().saturating_sub(1));
		}
		overlay.selected_index = overlay.coerce_index(options.initial_index.unwrap_or(0) as isize);
		overlay.set_sections(plan_content);
		if let Some(state) = options.annotation_state {
			overlay.restore_annotation_state(&state);
			if !state.annotations.is_empty() {
				overlay.recompute_feedback();
			}
		}
		overlay
	}

	/// The approval options.
	pub fn options(&self) -> &[String] {
		&self.options
	}

	/// The highlighted option index (`None` when every option is disabled).
	pub fn selected_index(&self) -> Option<usize> {
		self.selected_index
	}

	/// The focus region.
	pub fn focus(&self) -> PlanReviewFocus {
		self.focus
	}

	/// The Contents entries (section indices, title heading excluded).
	pub fn toc(&self) -> &[usize] {
		&self.toc
	}

	/// Index of the section promoted to the sheet title heading.
	pub fn title_index(&self) -> Option<usize> {
		self.title_index
	}

	/// The plan sections.
	pub fn sections(&self) -> &[OverlaySection] {
		&self.sections
	}

	/// The model-tier slider, when one was supplied.
	pub fn slider(&self) -> Option<&HookSelectorSlider> {
		self.slider.as_ref()
	}

	/// The highlighted slider segment.
	pub fn slider_index(&self) -> usize {
		self.slider_index
	}

	/// The label committed by Enter, while the host settles the choice.
	pub fn committed_label(&self) -> Option<&str> {
		self.committed.as_deref()
	}

	/// Whether an annotation draft is open.
	pub fn is_annotating(&self) -> bool {
		self.annotating
	}

	/// The current plan text.
	pub fn plan_content(&self) -> String {
		join_plan_sections(
			&self
				.sections
				.iter()
				.map(|section| PlanSection { level: section.level, title: section.title.clone(), raw: section.raw.clone() })
				.collect::<Vec<_>>(),
		)
	}

	/// Swap the displayed plan (e.g. after an external-editor round-trip) and
	/// reset scroll/focus. Does not emit `on_plan_edited`.
	pub fn set_plan_content(&mut self, plan_content: &str) {
		let annotations = self.annotation_state();
		self.set_sections(plan_content);
		self.restore_annotation_state(&annotations);
		self.scroll_offset = 0;
		self.toc_cursor = 0;
		self.deleted.clear();
		self.undo.clear();
		self.recompute_feedback();
	}

	/// Parse `plan_content` into sections and rebuild the ToC.
	fn set_sections(&mut self, plan_content: &str) {
		self.sections = parse_plan_sections(plan_content)
			.into_iter()
			.map(OverlaySection::from_plan_section)
			.collect();
		self.rebuild_toc();
		self.toc_cursor = self.toc_cursor.min(self.toc.len().saturating_sub(1));
	}

	/// Heading ancestry of every section.
	pub fn section_paths(&self) -> Vec<Vec<String>> {
		let mut stack: Vec<(usize, String)> = Vec::new();
		self.sections
			.iter()
			.map(|section| {
				if section.level < 1 {
					return Vec::new();
				}
				while stack.last().is_some_and(|(level, _)| *level >= section.level) {
					stack.pop();
				}
				stack.push((section.level, section.title.clone()));
				stack.iter().map(|(_, title)| title.clone()).collect()
			})
			.collect()
	}

	/// A serializable snapshot of every annotation.
	pub fn annotation_state(&self) -> PlanReviewAnnotationState {
		let paths = self.section_paths();
		let mut annotations = Vec::new();
		for (index, section) in self.sections.iter().enumerate() {
			for annotation in &section.annotations {
				annotations.push(SerializedAnnotation {
					section: SerializedSection {
						index,
						title: section.title.clone(),
						path: paths.get(index).cloned().unwrap_or_default(),
						content_hash: Some(section_content_hash(section)),
					},
					target: annotation.target.clone(),
					note: annotation.note.clone(),
				});
			}
		}
		PlanReviewAnnotationState { annotations }
	}

	/// Restore annotations, matching sections by path + hash first and by
	/// index + title otherwise.
	pub fn restore_annotation_state(&mut self, state: &PlanReviewAnnotationState) {
		let paths = self.section_paths();
		let hashes: Vec<u64> = self.sections.iter().map(section_content_hash).collect();
		for entry in &state.annotations {
			let note = entry.note.trim();
			if note.is_empty() {
				continue;
			}
			let stored_index = entry.section.index;
			let matching: Vec<usize> = if !entry.section.path.is_empty() && entry.section.content_hash.is_some() {
				(0..self.sections.len())
					.filter(|index| {
						self.sections[*index].title == entry.section.title
							&& Some(hashes[*index]) == entry.section.content_hash
							&& paths.get(*index).is_some_and(|path| *path == entry.section.path)
					})
					.collect()
			} else if self
				.sections
				.get(stored_index)
				.is_some_and(|section| section.title == entry.section.title)
			{
				vec![stored_index]
			} else {
				Vec::new()
			};
			let Some(&section_index) = matching
				.iter()
				.min_by_key(|candidate| candidate.abs_diff(stored_index))
			else {
				continue;
			};
			let target = match &entry.target {
				AnnotationTarget::Section => {
					if self.sections[section_index].level < 1 {
						continue;
					}
					AnnotationTarget::Section
				}
				AnnotationTarget::Line { row, context, context_truncated } => {
					let contexts = self.rendered_contexts(section_index);
					let Some(resolved) = resolve_line_row(
						*row,
						&LineAnchorContext { text: context.clone(), truncated: *context_truncated },
						&contexts,
					) else {
						continue;
					};
					let context = contexts[resolved].clone();
					AnnotationTarget::Line { row: resolved, context: context.text, context_truncated: context.truncated }
				}
			};
			self.sections[section_index].annotations.push(OverlayAnnotation { note: note.to_string(), target });
		}
	}

	/// Rendered captions of one section at the annotation context width.
	fn rendered_contexts(&self, section_index: usize) -> Vec<LineAnchorContext> {
		let Some(section) = self.sections.get(section_index) else {
			return Vec::new();
		};
		let mut markdown = Markdown::new();
		markdown.set_text(&section.raw);
		markdown
			.render(MAX_ANNOTATION_CONTEXT_WIDTH as u16)
			.iter()
			.map(|line| line_context(line))
			.collect()
	}

	/// Rebuild the ToC, dropping the plan's title heading from it.
	fn rebuild_toc(&mut self) {
		let headings: Vec<usize> = (0..self.sections.len()).filter(|index| self.sections[*index].level >= 1).collect();
		let min_level = headings.iter().map(|index| self.sections[*index].level).min();
		let title_index = match (min_level, headings.first()) {
			(Some(level), Some(first)) => {
				let top: Vec<usize> = headings
					.iter()
					.copied()
					.filter(|index| self.sections[*index].level == level)
					.collect();
				if top.len() == 1 && top[0] == *first { Some(top[0]) } else { None }
			}
			_ => None,
		};
		self.title_index = title_index;
		self.toc = headings.into_iter().filter(|index| Some(*index) != title_index).collect();
		self.toc_base_level =
			self.toc.iter().map(|index| self.sections[*index].level).min().unwrap_or(1);
	}

	/// Clamp `index` to range, then walk to the nearest enabled option.
	fn coerce_index(&self, index: isize) -> Option<usize> {
		let max = self.options.len().checked_sub(1)?;
		let clamped = index.clamp(0, max as isize) as usize;
		if !self.disabled.contains(&clamped) {
			return Some(clamped);
		}
		for candidate in clamped + 1..=max {
			if !self.disabled.contains(&candidate) {
				return Some(candidate);
			}
		}
		for candidate in (0..clamped).rev() {
			if !self.disabled.contains(&candidate) {
				return Some(candidate);
			}
		}
		Some(clamped)
	}

	/// First enabled option index, used to detect the "top".
	pub fn first_enabled_index(&self) -> Option<usize> {
		(0..self.options.len()).find(|index| !self.disabled.contains(index))
	}

	/// Move the option cursor by `delta`, skipping disabled rows.
	fn move_selection(&mut self, delta: isize) {
		let Some(mut index) = self.selected_index else {
			return;
		};
		let max = self.options.len().saturating_sub(1);
		loop {
			let next = (index as isize + delta).clamp(0, max as isize) as usize;
			if next == index {
				return;
			}
			index = next;
			if !self.disabled.contains(&index) {
				self.selected_index = Some(index);
				return;
			}
		}
	}

	/// Step the slider by `delta`, clamped to its edges.
	fn move_slider(&mut self, delta: isize) {
		let Some(slider) = self.slider.as_ref() else {
			return;
		};
		let max = slider.segments.len().saturating_sub(1);
		let next = (self.slider_index as isize + delta).clamp(0, max as isize) as usize;
		self.slider_index = next;
	}

	/// Synchronize an external tier pick with the displayed slider.
	pub fn set_slider_index(&mut self, index: usize) {
		if self.committed.is_some() || self.slider.is_none() {
			return;
		}
		self.move_slider(index as isize - self.slider_index as isize);
	}

	/// Commit an external answer through the same path as Enter; false when
	/// nothing was committed.
	pub fn select_option(&mut self, label: &str) -> bool {
		if self.committed.is_some() {
			return false;
		}
		let Some(index) = self.options.iter().position(|option| option == label) else {
			return false;
		};
		if self.disabled.contains(&index) {
			return false;
		}
		self.selected_index = Some(index);
		self.confirm_selection();
		self.committed.as_deref() == Some(label)
	}

	fn confirm_selection(&mut self) {
		let Some(index) = self.selected_index else {
			return;
		};
		let Some(label) = self.options.get(index).cloned() else {
			return;
		};
		if self.disabled.contains(&index) {
			return;
		}
		self.committed = Some(label.clone());
		if let Some(mut on_pick) = self.callbacks.on_pick.take() {
			on_pick(&label);
			self.callbacks.on_pick = Some(on_pick);
		}
	}

	fn cancel(&mut self) {
		self.committed = Some(String::new());
		if let Some(on_cancel) = self.callbacks.on_cancel.take() {
			on_cancel();
			self.callbacks.on_cancel = Some(on_cancel);
		}
	}

	/// Cycle the focus region; the sidebar is skipped when it is not shown.
	fn cycle_region(&mut self, direction: isize) {
		let regions: Vec<PlanReviewFocus> = if self.sidebar_visible(self.body_width.max(SIDEBAR_MIN_TOTAL_WIDTH)) {
			vec![PlanReviewFocus::Toc, PlanReviewFocus::Body, PlanReviewFocus::Actions]
		} else {
			vec![PlanReviewFocus::Body, PlanReviewFocus::Actions]
		};
		let current = regions.iter().position(|region| *region == self.focus);
		let length = regions.len() as isize;
		let base = current.map(|position| position as isize).unwrap_or(length - 1);
		let next = (base + direction).rem_euclid(length) as usize;
		self.focus = regions[next];
		if self.focus == PlanReviewFocus::Toc {
			self.toc_cursor = self.derive_toc_cursor();
		}
	}

	/// Greatest ToC position whose section starts at or above the scroll offset.
	fn derive_toc_cursor(&self) -> usize {
		if self.toc.is_empty() {
			return 0;
		}
		let mut current = 0;
		for (index, offset) in self.section_offsets.iter().enumerate() {
			if *offset <= self.scroll_offset {
				current = index;
			} else {
				break;
			}
		}
		self.toc.iter().rposition(|index| *index <= current).unwrap_or(0)
	}

	/// The maximum body scroll offset.
	fn max_scroll_offset(&self, height: usize) -> usize {
		self.body_lines.len().saturating_sub(height)
	}

	fn scroll(&mut self, delta: isize, height: usize) {
		let max = self.max_scroll_offset(height);
		self.scroll_offset = (self.scroll_offset as isize + delta).clamp(0, max as isize) as usize;
	}

	/// Scroll the body so the selected ToC section's heading sits at the top.
	fn scrub_body_to_toc(&mut self) {
		let Some(section_index) = self.toc.get(self.toc_cursor) else {
			return;
		};
		if let Some(offset) = self.section_offsets.get(*section_index) {
			self.scroll_offset = *offset;
		}
	}

	fn move_toc_cursor(&mut self, delta: isize) {
		if self.toc.is_empty() {
			return;
		}
		let next = (self.toc_cursor as isize + delta).clamp(0, self.toc.len() as isize - 1) as usize;
		if next == self.toc_cursor {
			return;
		}
		self.toc_cursor = next;
		self.scrub_body_to_toc();
	}

	fn push_undo(&mut self) {
		if self.undo.len() >= MAX_UNDO_ENTRIES {
			self.undo.remove(0);
		}
		self.undo.push(UndoEntry {
			text: self.plan_content(),
			annotations: self.sections.iter().map(|section| section.annotations.clone()).collect(),
			deleted: self.deleted.clone(),
		});
	}

	fn undo_last(&mut self) {
		let Some(entry) = self.undo.pop() else {
			return;
		};
		self.set_sections(&entry.text);
		for (index, annotations) in entry.annotations.into_iter().enumerate() {
			if let Some(section) = self.sections.get_mut(index) {
				section.annotations = annotations;
			}
		}
		self.deleted = entry.deleted;
		self.toc_cursor = self.toc_cursor.min(self.toc.len().saturating_sub(1));
		self.emit_plan_edited();
		self.recompute_feedback();
	}

	/// Delete the selected ToC section (with its nested subsections).
	pub fn delete_selected_section(&mut self) {
		let Some(section_index) = self.toc.get(self.toc_cursor).copied() else {
			return;
		};
		let plan_sections: Vec<PlanSection> = self
			.sections
			.iter()
			.map(|section| PlanSection { level: section.level, title: section.title.clone(), raw: section.raw.clone() })
			.collect();
		let span = section_deletion_span(&plan_sections, section_index);
		if span.is_empty() {
			return;
		}
		self.push_undo();
		for index in &span {
			let section = &self.sections[*index];
			if section.level >= 1 && !section.title.is_empty() {
				self.deleted.push(section.title.clone());
			}
		}
		for index in span.into_iter().rev() {
			self.sections.remove(index);
		}
		self.rebuild_toc();
		self.toc_cursor = self.toc_cursor.min(self.toc.len().saturating_sub(1));
		self.emit_plan_edited();
		self.recompute_feedback();
	}

	fn emit_plan_edited(&mut self) {
		if self.callbacks.on_plan_edited.is_none() {
			return;
		}
		let plan = self.plan_content();
		if let Some(mut on_edit) = self.callbacks.on_plan_edited.take() {
			on_edit(&plan);
			self.callbacks.on_plan_edited = Some(on_edit);
		}
	}

	/// Start a section annotation on the selected ToC entry.
	fn start_section_annotate(&mut self) {
		let Some(section_index) = self.toc.get(self.toc_cursor).copied() else {
			return;
		};
		self.start_annotate(section_index, AnnotationTarget::Section);
	}

	/// Start a line annotation on the anchor under the top visible body row.
	fn start_body_annotate(&mut self) {
		let Some(anchor) = self.body_anchors.get(self.scroll_offset).cloned() else {
			return;
		};
		self.start_annotate(anchor.section_index, anchor.annotation_target());
	}

	fn start_annotate(&mut self, section_index: usize, target: AnnotationTarget) {
		self.annotate_target = Some((section_index, target));
		self.editing_annotation = None;
		self.annotating = true;
		self.editor.set_text("");
	}

	/// Edit the first annotation attached to the focused section or body row.
	pub fn edit_current_annotation(&mut self) {
		let candidates = self.annotation_candidates();
		let Some((section_index, annotation_index)) = candidates.first().copied() else {
			return;
		};
		let note = self.sections[section_index].annotations[annotation_index].note.clone();
		let target = self.sections[section_index].annotations[annotation_index].target.clone();
		self.editing_annotation = Some((section_index, annotation_index));
		self.annotate_target = Some((section_index, target));
		self.annotating = true;
		self.editor.set_text(note);
	}

	/// Annotations the current focus points at.
	pub fn annotation_candidates(&self) -> Vec<(usize, usize)> {
		match self.focus {
			PlanReviewFocus::Toc => {
				let Some(section_index) = self.toc.get(self.toc_cursor).copied() else {
					return Vec::new();
				};
				(0..self.sections[section_index].annotations.len())
					.map(|annotation_index| (section_index, annotation_index))
					.collect()
			}
			PlanReviewFocus::Body => {
				let Some(anchor) = self.body_anchors.get(self.scroll_offset) else {
					return Vec::new();
				};
				let section = &self.sections[anchor.section_index];
				(0..section.annotations.len())
					.filter(|index| {
						let annotation = &section.annotations[*index];
						match &annotation.target {
							AnnotationTarget::Section => true,
							AnnotationTarget::Line { row, context, context_truncated } => {
								*row == anchor.row && context == &anchor.context && *context_truncated == anchor.context_truncated
							}
						}
					})
					.map(|annotation_index| (anchor.section_index, annotation_index))
					.collect()
			}
			PlanReviewFocus::Actions => Vec::new(),
		}
	}

	/// Commit the annotation draft, replacing or deleting when editing.
	pub fn submit_annotation(&mut self, value: &str) {
		let note = value.trim().to_string();
		let target = self.annotate_target.take();
		let editing = self.editing_annotation.take();
		self.annotating = false;
		self.editor.set_text("");
		if let Some((section_index, annotation_index)) = editing {
			let known = self
				.sections
				.get(section_index)
				.is_some_and(|section| annotation_index < section.annotations.len());
			if !known {
				return;
			}
			self.push_undo();
			if note.is_empty() {
				self.sections[section_index].annotations.remove(annotation_index);
			} else {
				self.sections[section_index].annotations[annotation_index].note = note;
			}
			self.recompute_feedback();
			return;
		}
		let Some((section_index, target)) = target else {
			return;
		};
		if note.is_empty() || section_index >= self.sections.len() {
			return;
		}
		self.push_undo();
		self.sections[section_index].annotations.push(OverlayAnnotation { note, target });
		self.recompute_feedback();
	}

	/// Abandon the annotation draft.
	pub fn exit_annotate(&mut self) {
		self.annotating = false;
		self.annotate_target = None;
		self.editing_annotation = None;
		self.editor.set_text("");
	}

	/// Recompute the Refine feedback and emit the annotation snapshot.
	fn recompute_feedback(&mut self) {
		let state = self.annotation_state();
		if let Some(mut on_state) = self.callbacks.on_annotation_state_change.take() {
			on_state(&state);
			self.callbacks.on_annotation_state_change = Some(on_state);
		}
		let annotated: Vec<&OverlaySection> =
			self.sections.iter().filter(|section| !section.annotations.is_empty()).collect();
		if annotated.is_empty() && self.deleted.is_empty() {
			if let Some(mut on_feedback) = self.callbacks.on_feedback_change.take() {
				on_feedback("");
				self.callbacks.on_feedback_change = Some(on_feedback);
			}
			return;
		}
		let mut feedback = String::from("Refinement feedback on the plan:\n");
		if !self.deleted.is_empty() {
			feedback.push_str("\nRemove these sections:\n");
			for title in &self.deleted {
				feedback.push_str(&format!("- {title}\n"));
			}
		}
		for section in &annotated {
			feedback.push_str(&format!(
				"\n## {}\n",
				if section.title.is_empty() { "Plan preamble" } else { &section.title }
			));
			for annotation in &section.annotations {
				if let AnnotationTarget::Line { context, .. } = &annotation.target {
					feedback.push_str(&format!("> Line: {context}\n"));
				}
				feedback.push_str(&format_annotation_feedback(&annotation.note));
			}
		}
		if let Some(mut on_feedback) = self.callbacks.on_feedback_change.take() {
			on_feedback(&feedback);
			self.callbacks.on_feedback_change = Some(on_feedback);
		}
	}

	/// The current Refine feedback markdown.
	pub fn feedback(&self) -> String {
		let annotated: Vec<&OverlaySection> =
			self.sections.iter().filter(|section| !section.annotations.is_empty()).collect();
		if annotated.is_empty() && self.deleted.is_empty() {
			return String::new();
		}
		let mut feedback = String::from("Refinement feedback on the plan:\n");
		if !self.deleted.is_empty() {
			feedback.push_str("\nRemove these sections:\n");
			for title in &self.deleted {
				feedback.push_str(&format!("- {title}\n"));
			}
		}
		for section in annotated {
			feedback.push_str(&format!(
				"\n## {}\n",
				if section.title.is_empty() { "Plan preamble" } else { &section.title }
			));
			for annotation in &section.annotations {
				if let AnnotationTarget::Line { context, .. } = &annotation.target {
					feedback.push_str(&format!("> Line: {context}\n"));
				}
				feedback.push_str(&format_annotation_feedback(&annotation.note));
			}
		}
		feedback
	}

	/// Rebuild the concatenated body rows and their anchors for `width`.
	fn build_body(&mut self, width: usize) {
		if self.body_width == width && !self.body_lines.is_empty() {
			return;
		}
		let mut lines: Vec<String> = Vec::new();
		let mut anchors: Vec<BodyRowAnchor> = Vec::new();
		let mut offsets: Vec<usize> = vec![0; self.sections.len()];
		for section_index in 0..self.sections.len() {
			offsets[section_index] = lines.len();
			let mut markdown = Markdown::new();
			markdown.set_text(&self.sections[section_index].raw);
			let rendered = markdown.render(width.max(1) as u16);
			let contexts: Vec<LineAnchorContext> = rendered.iter().map(|line| line_context(line)).collect();
			let mut notes_by_row: HashMap<usize, Vec<String>> = HashMap::new();
			for annotation in &self.sections[section_index].annotations {
				let row = match &annotation.target {
					AnnotationTarget::Section => Some(0),
					AnnotationTarget::Line { row, context, context_truncated } => resolve_line_row(
						*row,
						&LineAnchorContext { text: context.clone(), truncated: *context_truncated },
						&contexts,
					),
				};
				if let Some(row) = row {
					notes_by_row.entry(row).or_default().push(annotation.note.clone());
				}
			}
			for (row, line) in rendered.iter().enumerate() {
				let context = contexts.get(row).cloned().unwrap_or_else(|| line_context(line));
				let anchor = BodyRowAnchor {
					section_index,
					row,
					context: context.text.clone(),
					context_truncated: context.truncated,
				};
				lines.push(line.clone());
				anchors.push(anchor.clone());
				if let Some(notes) = notes_by_row.get(&row) {
					for note in notes {
						append_annotation_callout(&mut lines, &mut anchors, note, &anchor, width, &self.theme);
					}
				}
			}
		}
		self.section_offsets = offsets;
		self.body_anchors = anchors;
		self.body_lines = lines;
		self.body_width = width;
	}

	/// Whether the Contents sidebar shows at `width`.
	fn sidebar_visible(&self, width: usize) -> bool {
		if self.toc.len() < SIDEBAR_MIN_HEADINGS || width < SIDEBAR_MIN_TOTAL_WIDTH {
			return false;
		}
		split_body_width(width, sidebar_width_for(width)) >= SIDEBAR_MIN_BODY_WIDTH
	}

	/// Sidebar rows for the ToC window around the cursor.
	pub fn sidebar_lines(&self, rows: usize) -> Vec<String> {
		let slots = rows;
		let total = self.toc.len();
		let start = if total > slots { self.toc_cursor.saturating_sub(slots / 2).min(total.saturating_sub(slots)) } else { 0 };
		let mut lines = Vec::with_capacity(slots);
		for slot in 0..slots {
			let position = start + slot;
			if position >= total {
				lines.push(String::new());
				continue;
			}
			lines.push(self.render_toc_entry(position, 30));
		}
		lines
	}

	/// One Contents entry: indent, title, annotation marker and gutter glyph.
	pub fn render_toc_entry(&self, position: usize, width: usize) -> String {
		let Some(section_index) = self.toc.get(position).copied() else {
			return String::new();
		};
		let section = &self.sections[section_index];
		let highlighted = position == self.toc_cursor;
		let selected = highlighted && self.focus == PlanReviewFocus::Toc;
		let indent = " ".repeat(section.level.saturating_sub(self.toc_base_level));
		let marker = if section.annotations.is_empty() { "" } else { " ✎" };
		let available = width.saturating_sub(1 + visible_width(&indent) + visible_width(marker));
		let title = if section.title.is_empty() { "(untitled)" } else { &section.title };
		let title = truncate_to_width(title, available, Ellipsis::Unicode);
		let gutter = if selected {
			"›"
		} else if highlighted {
			"▎"
		} else {
			" "
		};
		let line = format!("{gutter}{indent}{title}{marker}");
		if selected {
			self.theme.bg("selectedBg", &line).unwrap_or(line)
		} else if highlighted {
			self.theme.fg("accent", &line).unwrap_or(line)
		} else {
			self.theme.fg("muted", &line).unwrap_or(line)
		}
	}

	/// The option rows, cursor on the selection.
	pub fn option_lines(&self) -> Vec<String> {
		let active = self.focus == PlanReviewFocus::Actions;
		self.options
			.iter()
			.enumerate()
			.map(|(index, label)| {
				let selected = self.selected_index == Some(index);
				let disabled = self.disabled.contains(&index);
				let cursor_glyph = self.theme.nav().cursor.clone();
				let cursor = if selected {
					self.theme
						.fg(if active { "accent" } else { "dim" }, &format!("{cursor_glyph} "))
						.unwrap_or_else(|_| format!("{cursor_glyph} "))
				} else {
					"  ".to_string()
				};
				let rendered = if disabled {
					self.theme.fg("dim", label).unwrap_or_else(|_| label.clone())
				} else if selected && active {
					self.theme.fg("accent", label).unwrap_or_else(|_| label.clone())
				} else {
					self.theme.fg("text", label).unwrap_or_else(|_| label.clone())
				};
				cursor + &rendered
			})
			.collect()
	}

	/// Slider rows: the chip track plus the active segment's detail.
	pub fn slider_lines(&self) -> Vec<String> {
		let Some(slider) = self.slider.as_ref() else {
			return Vec::new();
		};
		let segments: Vec<TrackSegment> =
			slider.segments.iter().map(|segment| TrackSegment::new(segment.label.clone())).collect();
		let track = render_segment_track(&segments, self.slider_index, &self.theme);
		let left = self
			.theme
			.fg(if self.slider_index > 0 { "accent" } else { "dim" }, "◂")
			.unwrap_or_else(|_| "◂".to_string());
		let right = self
			.theme
			.fg(
				if self.slider_index + 1 < slider.segments.len() { "accent" } else { "dim" },
				"▸",
			)
			.unwrap_or_else(|_| "▸".to_string());
		let caption = slider
			.caption
			.as_ref()
			.map(|caption| format!("{}  ", self.theme.fg("dim", caption).unwrap_or_else(|_| caption.clone())))
			.unwrap_or_default();
		let mut lines = vec![format!("{caption}{left}  {track}  {right}")];
		if let Some(detail) = slider.segments.get(self.slider_index).and_then(|segment| segment.detail.as_ref()) {
			let arrow = self.theme.fg("dim", "↳").unwrap_or_else(|_| "↳".to_string());
			let detail = self.theme.fg("muted", detail).unwrap_or_else(|_| detail.clone());
			lines.push(format!("  {arrow} {detail}"));
		}
		lines
	}

	/// The footer help for the focused region.
	pub fn help(&self) -> String {
		let formatter = KeyHintFormatter::new(&self.theme);
		let up_down = editor_keys(&self.bindings, &self.theme, &["tui.select.up", "tui.select.down"]);
		let enter = formatter.format_key_hint("enter");
		let mut parts: Vec<String> = match self.focus {
			PlanReviewFocus::Actions => {
				let mut parts = vec![format!("{up_down} select"), format!("{enter} confirm")];
				if self.slider.is_some() {
					parts.push(format!("{} model", formatter.format_key_hints(&["left", "right"])));
				}
				parts
			}
			PlanReviewFocus::Toc => vec![
				format!("{up_down} section"),
				format!("{enter} open"),
				format!("{} annotate", formatter.format_key_hint("a")),
				format!("{} edit", formatter.format_key_hint("e")),
				format!("{} delete", formatter.format_key_hint("d")),
				format!("{} undo", formatter.format_key_hint("u")),
			],
			PlanReviewFocus::Body => vec![
				format!("{up_down} scroll"),
				formatter.format_key_hints(&["pageUp", "pageDown"]),
				format!("{} annotate", formatter.format_key_hint("a")),
				format!("{} edit", formatter.format_key_hint("e")),
				format!("{} undo", formatter.format_key_hint("u")),
			],
		};
		if self.callbacks.on_copy_plan.is_some() {
			parts.push(format!("{} copy", formatter.format_key_hint("c")));
		}
		parts.push(format!("{} regions", formatter.format_key_hint("tab")));
		if let Some(label) = self.external_editor_label.as_deref() {
			if self.focus != PlanReviewFocus::Toc {
				parts.push(format!("{label} editor"));
			}
		}
		parts.push(self.help_suffix.clone().unwrap_or_else(|| format!("{} cancel", interrupt_key(&self.bindings, &self.theme))));
		parts.join(HINT_SEPARATOR)
	}

	/// The prompt line above the options.
	fn prompt_line(&self) -> String {
		self.prompt_title.clone().unwrap_or_default()
	}

	/// The plan body window at `height` rows.
	fn body_window(&self, height: usize) -> Vec<String> {
		self.body_lines.iter().skip(self.scroll_offset).take(height).cloned().collect()
	}

	/// The native body: each section as markdown, annotation callouts inline.
	fn describe_body(&self) -> NativeNode {
		let children: Vec<NativeNode> = self
			.sections
			.iter()
			.map(|section| {
				let mut props = Props::new();
				props.set_role("omp.plan.section");
				col(vec![md(section.raw.clone(), Props::new())], props)
			})
			.collect();
		let mut props = Props::new();
		props.set_role("omp.plan.body");
		props.set_str("gap", "sm");
		col(children, props)
	}

	/// The native Contents sidebar.
	fn describe_toc(&self) -> NativeNode {
		let items: Vec<NativeNode> = self
			.toc
			.iter()
			.map(|section_index| {
				let section = &self.sections[*section_index];
				let mut props = Props::new();
				props.set_str("label", section.title.clone());
				props.set_i64("depth", section.level as i64);
				if !section.annotations.is_empty() {
					props.set_str("detail", format!("✎ {}", section.annotations.len()));
				}
				let key = section_index.to_string();
				node(NodeKind::Item, props, Vec::new(), Some(key.as_str()))
			})
			.collect();
		let mut props = Props::new();
		props.set_role("omp.plan.toc");
		if let Some(section_index) = self.toc.get(self.toc_cursor) {
			props.set_str("selected", section_index.to_string());
		}
		let mut list_props = Props::new();
		list_props.set_bool("selectable", true);
		node(NodeKind::List, list_props, items, Some("toc"))
	}

	/// The native sheet: prompt, body, Contents, options and actions.
	pub fn describe(&self, _cx: &DescribeContext) -> NativeNode {
		let prompt = self.prompt_line();
		let mut children: Vec<NativeNode> = Vec::new();
		if !prompt.is_empty() {
			let mut props = Props::new();
			props.set_role("omp.plan.prompt");
			children.push(text(NodeText::Spans(vec![span(prompt, Some("muted"))]), props));
		}
		children.push(self.describe_body());
		if self.toc.len() >= SIDEBAR_MIN_HEADINGS {
			children.push(self.describe_toc());
		}
		for line in self.slider_lines() {
			children.push(text(NodeText::Plain(line), Props::new()));
		}
		let option_items: Vec<NativeNode> = self
			.options
			.iter()
			.enumerate()
			.map(|(index, label)| {
				let mut props = Props::new();
				props.set_str("label", label.clone());
				if self.disabled.contains(&index) {
					props.set_str("disabled", "Unavailable");
				}
				let key = format!("option-{index}");
				node(NodeKind::Item, props, Vec::new(), Some(key.as_str()))
			})
			.collect();
		let mut list_props = Props::new();
		if let Some(index) = self.selected_index {
			list_props.set_str("selected", format!("option-{index}"));
		}
		children.push(select_list("options", option_items, list_props));
		if self.annotating {
			let mut props = Props::new();
			props.set_str("role", "omp.plan.annotate");
			props.set_str("placeholder", "Note (Enter saves, Esc cancels)");
			children.push(node(NodeKind::Input, props, Vec::new(), Some("annotation")));
		}
		children.push(self.describe_actions());
		col(children, props_with_role(PLAN_REVIEW_ROLE))
	}

	fn describe_actions(&self) -> NativeNode {
		let confirm = action_button(
			"Choose",
			"confirm",
			ActionButtonOptions {
				keys: Some("enter".to_string()),
				tone: Some(ActionButtonTone::Accent),
				disabled: self.committed.as_ref().map(|_| "Choice already submitted".to_string()),
				..Default::default()
			},
		);
		let cancel = action_button(
			"Cancel",
			"cancel",
			ActionButtonOptions {
				keys: Some(editor_key(&self.bindings, &self.theme, "tui.select.cancel")),
				..Default::default()
			},
		);
		let hints = hints_row(
			&[
				action_hint(&self.bindings, &["tui.select.up", "tui.select.down"][..], "select"),
				Some(crate::native::overlay::NativeHint { keys: vec!["tab".to_string()], label: "regions".to_string() }),
			],
			"hints",
		);
		col(vec![action_bar(&[Some(confirm), Some(cancel)], PLAN_REVIEW_ROLE), hints], Props::new())
	}

	/// Route a native action or row selection.
	pub fn handle_native_event(&mut self, event: &NativeUiEvent) {
		match event {
			NativeUiEvent::Action { act, .. } => match act.as_str() {
				"confirm" => self.confirm_selection(),
				"cancel" => self.cancel(),
				_ => {}
			},
			NativeUiEvent::Select { key, item } => {
				if key.first().is_some_and(|part| part == "toc") {
					if let Ok(section_index) = item.parse::<usize>() {
						if let Some(position) = self.toc.iter().position(|index| *index == section_index) {
							self.toc_cursor = position;
							self.scrub_body_to_toc();
						}
					}
					return;
				}
				if let Some(index) = item.strip_prefix("option-").and_then(|index| index.parse::<usize>().ok()) {
					if !self.disabled.contains(&index) {
						self.selected_index = Some(index);
					}
				}
			}
			NativeUiEvent::Activate { key, item } => {
				if key.first().is_some_and(|part| part == "options") {
					if let Some(index) = item.strip_prefix("option-").and_then(|index| index.parse::<usize>().ok()) {
						if !self.disabled.contains(&index) {
							self.selected_index = Some(index);
							self.confirm_selection();
						}
					}
				}
			}
			_ => {}
		}
	}
}

impl BodyRowAnchor {
	/// The annotation target this row anchors.
	pub fn annotation_target(&self) -> AnnotationTarget {
		AnnotationTarget::Line {
			row: self.row,
			context: self.context.clone(),
			context_truncated: self.context_truncated,
		}
	}
}

fn props_with_role(role: &str) -> Props {
	let mut props = Props::new();
	props.set_role(role);
	props
}

/// Sidebar width for `width`: 24% of the box, clamped to 18–30 columns.
pub fn sidebar_width_for(width: usize) -> usize {
	((width as f64 * 0.24).round() as usize).clamp(18, 30)
}

/// Append one annotation callout (one row per note line) below `anchor`.
fn append_annotation_callout(
	lines: &mut Vec<String>,
	anchors: &mut Vec<BodyRowAnchor>,
	note: &str,
	anchor: &BodyRowAnchor,
	width: usize,
	theme: &Theme,
) {
	let bar = theme.fg("warning", "▎ ").unwrap_or_else(|_| "▎ ".to_string());
	for (index, note_line) in note.split('\n').enumerate() {
		let prefix = if index == 0 {
			format!("{bar}{}", theme.fg("dim", "note: ").unwrap_or_else(|_| "note: ".to_string()))
		} else {
			format!("{bar}{}", theme.fg("dim", "      ").unwrap_or_else(|_| "      ".to_string()))
		};
		let available = width.saturating_sub(visible_width(&prefix));
		let display = truncate_to_width(&replace_tabs(note_line), available, Ellipsis::Unicode);
		let display = theme.fg("accent", &display).unwrap_or(display);
		lines.push(truncate_to_width(&format!("{prefix}{display}"), width, Ellipsis::Omit));
		anchors.push(anchor.clone());
	}
}

impl Component for PlanReviewOverlay {
	fn render(&mut self, width: u16) -> Vec<String> {
		let width_usize = usize::from(width);
		let inner = width_usize.saturating_sub(4);
		let sidebar_shown = self.sidebar_visible(width_usize);
		let body_width = if sidebar_shown {
			split_body_width(width_usize, sidebar_width_for(width_usize))
		} else {
			inner
		};
		self.build_body(body_width.max(1));

		let slider_lines = self.slider_lines();
		let option_lines = self.option_lines();
		let prompt = self.prompt_line();
		let footer = self.help();
		let editor_lines = if self.annotating {
			self.editor.render(body_width.max(1) as u16).into_iter().take(MAX_ANNOTATION_EDITOR_ROWS).collect::<Vec<_>>()
		} else {
			Vec::new()
		};
		let fixed = 2 + prompt.is_empty().then_some(0).unwrap_or(1) + slider_lines.len() + option_lines.len() + editor_lines.len() + 1;
		let body_height = self.body_lines.len().min(MIN_BODY_ROWS.max(inner.saturating_sub(fixed)).max(MIN_BODY_ROWS));
		let body_window = self.body_window(body_height);

		let mut lines: Vec<String> = Vec::new();
		if sidebar_shown {
			lines.push(top_border_split(width, OVERLAY_TITLE, sidebar_width_for(width_usize), &self.panel_theme));
			let sidebar = self.sidebar_lines(body_height);
			for (row_index, body_line) in body_window.iter().enumerate() {
				let side = sidebar.get(row_index).cloned().unwrap_or_default();
				lines.push(split_row(&side, body_line, width, sidebar_width_for(width_usize), &self.panel_theme));
			}
			lines.push(divider_split(width, sidebar_width_for(width_usize), &self.panel_theme));
		} else {
			lines.push(top_border(width, OVERLAY_TITLE, &self.panel_theme));
			for body_line in &body_window {
				lines.push(box_row(body_line, width, &self.panel_theme));
			}
			lines.push(divider(width, &self.panel_theme));
		}
		if !prompt.is_empty() {
			let prompt = self.theme.fg("muted", &prompt).unwrap_or(prompt);
			lines.push(box_row(&prompt, width, &self.panel_theme));
		}
		for slider_line in slider_lines {
			lines.push(box_row(&slider_line, width, &self.panel_theme));
		}
		for option_line in option_lines {
			lines.push(box_row(&option_line, width, &self.panel_theme));
		}
		for editor_line in editor_lines {
			lines.push(box_row(&editor_line, width, &self.panel_theme));
		}
		let footer = self.theme.fg("dim", &footer).unwrap_or(footer);
		lines.push(box_row(&footer, width, &self.panel_theme));
		lines.push(bottom_border(width, &self.panel_theme));
		lines
	}

	fn handle_input(&mut self, key: &Key) {
		if self.committed.is_some() {
			return;
		}
		if self.annotating {
			if matches!(key, Key::Esc) {
				self.exit_annotate();
				return;
			}
			if matches!(key, Key::Enter) {
				let draft = self.editor.get_text();
				self.submit_annotation(&draft);
				return;
			}
			self.editor.handle_input(key);
			return;
		}
		if matches!(key, Key::Esc) {
			self.cancel();
			return;
		}
		if self.focus != PlanReviewFocus::Actions {
			if matches!(key, Key::Char('u')) {
				self.undo_last();
				return;
			}
			if matches!(key, Key::Char('e')) {
				self.edit_current_annotation();
				return;
			}
		}
		if matches!(key, Key::Char('c')) && self.callbacks.on_copy_plan.is_some() {
			let plan = self.plan_content();
			if let Some(mut on_copy) = self.callbacks.on_copy_plan.take() {
				on_copy(&plan);
				self.callbacks.on_copy_plan = Some(on_copy);
			}
			return;
		}
		if matches!(key, Key::Tab) {
			self.cycle_region(1);
			return;
		}
		if matches!(key, Key::BackTab) {
			self.cycle_region(-1);
			return;
		}
		let body_height = MIN_BODY_ROWS.max(self.body_lines.len().min(MIN_BODY_ROWS + 7));
		match self.focus {
			PlanReviewFocus::Actions => self.handle_actions(key, body_height),
			PlanReviewFocus::Body => self.handle_body(key, body_height),
			PlanReviewFocus::Toc => self.handle_toc(key),
		}
	}

	fn invalidate(&mut self) {
		self.body_width = 0;
		self.body_lines.clear();
	}
}

impl PlanReviewOverlay {
	/// Option-focus keys: slider on left/right (or h/l), selection on up/down.
	fn handle_actions(&mut self, key: &Key, body_height: usize) {
		match key {
			Key::Left => self.move_slider(-1),
			Key::Right => self.move_slider(1),
			Key::Char('h') if self.slider.is_some() => self.move_slider(-1),
			Key::Char('l') if self.slider.is_some() => self.move_slider(1),
			Key::Up | Key::Char('k') => {
				if self.selected_index.is_some() && self.selected_index == self.first_enabled_index() {
					self.focus = PlanReviewFocus::Body;
				} else {
					self.move_selection(-1);
				}
			}
			Key::Down | Key::Char('j') => self.move_selection(1),
			Key::Enter => self.confirm_selection(),
			other => self.handle_scroll(other, body_height),
		}
	}

	/// Body-focus keys: annotate, region hops and scrolling.
	fn handle_body(&mut self, key: &Key, body_height: usize) {
		match key {
			Key::Char('a') => self.start_body_annotate(),
			Key::Left | Key::Char('h') => {
				if self.sidebar_visible(self.body_width) {
					self.focus = PlanReviewFocus::Toc;
				}
			}
			Key::Right | Key::Char('l') | Key::Enter => self.focus = PlanReviewFocus::Actions,
			Key::Up | Key::Char('k') => {
				if self.scroll_offset == 0 && self.sidebar_visible(self.body_width) {
					self.focus = PlanReviewFocus::Toc;
				} else {
					self.scroll(-1, body_height);
				}
			}
			Key::Down | Key::Char('j') => {
				if self.scroll_offset >= self.max_scroll_offset(body_height) {
					self.focus = PlanReviewFocus::Actions;
				} else {
					self.scroll(1, body_height);
				}
			}
			other => self.handle_scroll(other, body_height),
		}
	}

	/// ToC-focus keys: move, jump, delete, annotate.
	fn handle_toc(&mut self, key: &Key) {
		match key {
			Key::Up | Key::Char('k') => self.move_toc_cursor(-1),
			Key::Down | Key::Char('j') => {
				if self.toc_cursor + 1 >= self.toc.len() {
					self.focus = PlanReviewFocus::Actions;
				} else {
					self.move_toc_cursor(1);
				}
			}
			Key::Right | Key::Char('l') | Key::Enter => self.focus = PlanReviewFocus::Body,
			Key::Char('d') | Key::Delete => self.delete_selected_section(),
			Key::Char('a') => self.start_section_annotate(),
			_ => {}
		}
	}

	/// Shared scroll dispatch: paging keys, g/G jumps.
	fn handle_scroll(&mut self, key: &Key, body_height: usize) {
		match key {
			Key::PageUp => self.scroll(-(body_height as isize), body_height),
			Key::PageDown => self.scroll(body_height as isize, body_height),
			Key::Home => self.scroll_offset = 0,
			Key::End => self.scroll_offset = self.max_scroll_offset(body_height),
			Key::Char('g') => self.scroll_offset = 0,
			Key::Char('G') => self.scroll_offset = self.max_scroll_offset(body_height),
			_ => {}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::theme::builtin;
	use crate::theme::{CreateThemeOptions, create_theme};
	use std::cell::RefCell;
	use std::rc::Rc;

	fn theme() -> Theme {
		create_theme(
			builtin::builtin_theme_json("dark").expect("builtin dark theme"),
			CreateThemeOptions::default(),
		)
		.expect("valid theme")
	}

	const PLAN: &str = "# Ship the feature\n\nIntro prose.\n\n## Design\n\nUse a queue.\n\n## Risks\n\nNone known.\n";

	fn options() -> PlanReviewOverlayOptions {
		PlanReviewOverlayOptions {
			prompt_title: Some("Plan mode - next step".to_string()),
			options: vec!["Approve".to_string(), "Refine".to_string(), "Cancel".to_string()],
			disabled_indices: vec![2],
			help_text: None,
			initial_index: Some(0),
			slider: None,
			external_editor_label: None,
			external_editor_name: None,
			annotation_state: None,
		}
	}

	fn overlay(options: PlanReviewOverlayOptions) -> (PlanReviewOverlay, Rc<RefCell<Vec<String>>>) {
		let picked = Rc::new(RefCell::new(Vec::new()));
		let sink = Rc::clone(&picked);
		let component = PlanReviewOverlay::new(
			theme(),
			KeybindingsManager::new(),
			PLAN,
			options,
			PlanReviewOverlayCallbacks {
				on_pick: Some(Box::new(move |label| sink.borrow_mut().push(label.to_string()))),
				..Default::default()
			},
		);
		(component, picked)
	}

	#[test]
	fn sections_and_toc_skip_the_plan_title() {
		let (overlay, _) = overlay(options());
		assert_eq!(overlay.sections().len(), 5);
		let titles: Vec<&str> =
			overlay.toc().iter().map(|index| overlay.sections()[*index].title.as_str()).collect();
		assert_eq!(titles, vec!["Design", "Risks"]);
		assert_eq!(overlay.toc_base_level, 2);
	}

	#[test]
	fn disabled_options_are_never_selected() {
		let (mut overlay, picked) = overlay(options());
		overlay.handle_input(&Key::Down);
		overlay.handle_input(&Key::Down);
		assert_eq!(overlay.selected_index(), Some(1));
		overlay.handle_input(&Key::Enter);
		assert_eq!(overlay.committed_label(), Some("Refine"));
		assert_eq!(picked.borrow().as_slice(), ["Refine"]);
		// A committed overlay ignores further input.
		overlay.handle_input(&Key::Up);
		assert_eq!(overlay.selected_index(), Some(1));
	}

	#[test]
	fn select_option_reports_whether_it_committed() {
		let (mut overlay, picked) = overlay(options());
		assert!(!overlay.select_option("Cancel"));
		assert!(overlay.selected_index().is_some());
		assert!(overlay.select_option("Approve"));
		assert_eq!(picked.borrow().as_slice(), ["Approve"]);
		assert!(!overlay.select_option("Refine"));
	}

	#[test]
	fn deleting_a_section_removes_its_subsections_and_can_be_undone() {
		let plan = "# Plan\n\n## A\n\n### A.1\n\nbody\n\n## B\n\nbody\n";
		let mut overlay = PlanReviewOverlay::new(
			theme(),
			KeybindingsManager::new(),
			plan,
			options(),
			PlanReviewOverlayCallbacks::default(),
		);
		// Focus the ToC and delete the first entry (section A, with A.1).
		overlay.cycle_region(1);
		assert_eq!(overlay.focus(), PlanReviewFocus::Toc);
		overlay.delete_selected_section();
		let titles: Vec<&str> = overlay.sections().iter().map(|section| section.title.as_str()).collect();
		assert_eq!(titles, vec!["Plan", "B"]);
		assert!(overlay.plan_content().contains("## B"));
		overlay.handle_input(&Key::Char('u'));
		let titles: Vec<&str> = overlay.sections().iter().map(|section| section.title.as_str()).collect();
		assert_eq!(titles, vec!["Plan", "A", "A.1", "B"]);
	}

	#[test]
	fn annotations_serialize_round_trip_and_drive_feedback() {
		let feedback = Rc::new(RefCell::new(String::new()));
		let sink = Rc::clone(&feedback);
		let mut overlay = PlanReviewOverlay::new(
			theme(),
			KeybindingsManager::new(),
			PLAN,
			options(),
			PlanReviewOverlayCallbacks {
				on_feedback_change: Some(Box::new(move |text| *sink.borrow_mut() = text.to_string())),
				..Default::default()
			},
		);
		overlay.cycle_region(1);
		overlay.handle_input(&Key::Char('a'));
		assert!(overlay.is_annotating());
		overlay.handle_input(&Key::Char('T'));
		overlay.handle_input(&Key::Enter);
		assert!(!overlay.is_annotating());
		let state = overlay.annotation_state();
		assert_eq!(state.annotations.len(), 1);
		assert_eq!(state.annotations[0].section.title, "Design");
		assert!(feedback.borrow().contains("## Design"));
		assert!(feedback.borrow().contains("- T"));

		let mut restored = PlanReviewOverlay::new(
			theme(),
			KeybindingsManager::new(),
			PLAN,
			PlanReviewOverlayOptions {
				annotation_state: Some(state),
				..options()
			},
			PlanReviewOverlayCallbacks::default(),
		);
		assert_eq!(restored.sections()[1].annotations.len(), 1);
		assert_eq!(restored.sections()[1].annotations[0].note, "T");
	}

	#[test]
	fn line_annotations_resolve_to_the_nearest_matching_row() {
		let mut overlay = PlanReviewOverlay::new(
			theme(),
			KeybindingsManager::new(),
			PLAN,
			options(),
			PlanReviewOverlayCallbacks::default(),
		);
		overlay.cycle_region(1);
		overlay.focus = PlanReviewFocus::Body;
		overlay.scroll_offset = 0;
		overlay.build_body(40);
		overlay.start_body_annotate();
		overlay.submit_annotation("check this line");
		let state = overlay.annotation_state();
		assert_eq!(state.annotations.len(), 1);
		assert!(matches!(state.annotations[0].target, AnnotationTarget::Line { .. }));
	}

	#[test]
	fn feedback_formats_multiline_notes_as_fenced_markdown() {
		assert_eq!(format_annotation_feedback("one line"), "- one line\n");
		let fenced = format_annotation_feedback("first\nsecond");
		assert!(fenced.starts_with("```md\n"));
		assert!(fenced.ends_with("```\n"));
		assert_eq!(markdown_fence_for("a ``` b"), "````");
	}

	#[test]
	fn rendering_frames_body_options_and_footer() {
		let (mut overlay, _) = overlay(options());
		let lines = overlay.render(80);
		assert!(lines[0].contains(OVERLAY_TITLE));
		assert!(lines.iter().any(|line| line.contains("Approve")));
		assert!(lines.iter().any(|line| line.contains("select")));
		assert!(lines.last().is_some_and(|line| !line.trim().is_empty()));
		// Wide plan with two headings shows the Contents column.
		assert!(lines.iter().any(|line| line.contains("Design")));
	}

	#[test]
	fn sidebar_width_is_clamped() {
		assert_eq!(sidebar_width_for(60), 18);
		assert_eq!(sidebar_width_for(200), 30);
		assert_eq!(sidebar_width_for(100), 24);
	}

	#[test]
	fn slider_moves_with_left_and_right_and_clamps() {
		let mut options = options();
		options.slider = Some(HookSelectorSlider {
			caption: Some("Model tier".to_string()),
			segments: vec![
				crate::overlays::hook_selector::HookSelectorSliderSegment {
					label: "quick".to_string(),
					detail: Some("fast".to_string()),
				},
				crate::overlays::hook_selector::HookSelectorSliderSegment { label: "deep".to_string(), detail: None },
			],
			index: 0,
		});
		let (mut overlay, _) = overlay(options);
		overlay.handle_input(&Key::Right);
		assert_eq!(overlay.slider_index(), 1);
		overlay.handle_input(&Key::Right);
		assert_eq!(overlay.slider_index(), 1);
		overlay.set_slider_index(0);
		assert_eq!(overlay.slider_index(), 0);
		assert_eq!(overlay.slider_lines().len(), 1);
	}

	#[test]
	fn body_scroll_clamps_at_both_ends() {
		let long_plan = format!("# Plan\n\n{}", "line\n".repeat(200));
		let mut overlay = PlanReviewOverlay::new(
			theme(),
			KeybindingsManager::new(),
			&long_plan,
			options(),
			PlanReviewOverlayCallbacks::default(),
		);
		overlay.focus = PlanReviewFocus::Body;
		overlay.build_body(40);
		let height = 5;
		overlay.scroll(3, height);
		assert_eq!(overlay.scroll_offset, 3);
		overlay.handle_input(&Key::End);
		assert_eq!(overlay.scroll_offset, overlay.max_scroll_offset(height));
		overlay.handle_input(&Key::Home);
		assert_eq!(overlay.scroll_offset, 0);
	}
}
