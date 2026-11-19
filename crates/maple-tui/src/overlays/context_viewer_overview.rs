//! Context viewer overview model: usage attribution, legend rows, and occupancy map.
//!
//! Reference: `l/src/overlays/context-viewer-overview.ts`.
//!
//! Deviations:
//! - ANSI legend/map rendering and native node builders (`legendLine`,
//!   `legendItem`, `mapRowLine`, `mapRowSpans`, `describeContextMeter`) depend
//!   on theme, `TspSpan`/native describe APIs and `CONTEXT_PRESENTATION`
//!   descriptors not fully available here. This port provides the pure
//!   allocation/geometry/row-planning/formatting/snapcompact/usage-summary
//!   logic and data structures; render helpers return plain-text/model data.
//! - `ContextViewCategory`/`ContextViewData`/`ContextBreakdown`/map planning
//!   types from `chrome/context-breakdown` and `context-map` are modeled as
//!   lightweight local structs capturing fields required by algorithms.
//! - `formatNumber` is implemented locally with simple grouping.

/// The map is hidden below this body width.
pub const MAP_MIN_WIDTH: usize = 52;
/// Map cells separated by a space from this body width.
pub const MAP_GAP_MIN_WIDTH: usize = 72;
/// Maximum map columns and rows.
pub const MAP_MAX_CELLS: usize = 16;
/// Width reserved for legend beside the map.
pub const LEGEND_MIN_WIDTH: usize = 32;
/// Columns between map and legend.
pub const MAP_LEGEND_GAP: usize = 3;

/// Overview legend/map row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverviewRow {
	pub key: String,
	pub id: String,
	pub label: String,
	/// Attributed usage; undefined when weights cannot scale current usage.
	pub tokens: Option<u64>,
	/// Whether row is openable/selectable as a category.
	pub openable: bool,
	pub child: bool,
	pub token_column: String,
	pub share_column: String,
}

/// Map geometry beside legend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapGeometry {
	pub columns: usize,
	pub rows: usize,
	pub cell_width: usize,
	pub gap: String,
	pub width: usize,
}

/// Minimal child category view.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextChildCategory {
	pub key: String,
	pub id: String,
	pub label: String,
	pub tokens: u64,
	pub entries_count: usize,
}

/// Minimal category view.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextViewCategory {
	pub id: String,
	pub label: String,
	pub overview_label: Option<String>,
	pub tokens: u64,
	pub entries_count: usize,
	pub children: Vec<ContextChildCategory>,
	pub openable: bool,
}

/// Snapcompact system prompt detail.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapcompactSystemPrompt {
	pub scope: String, // "agents-md" | "all"
	pub applied: bool,
	pub frames: u64,
	pub saved_tokens: u64,
	pub text_tokens: u64,
	pub image_tokens: u64,
	pub reason: Option<String>, // "budget" | "empty" | other
}

/// Snapcompact tool results detail.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapcompactToolResults {
	pub saved_tokens: u64,
	pub swapped: u64,
	pub total: u64,
	pub text_tokens: u64,
	pub image_tokens: u64,
	pub frames: u64,
}

/// Snapcompact breakdown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapcompactBreakdown {
	pub vision_capable: bool,
	pub system_prompt: Option<SnapcompactSystemPrompt>,
	pub tool_results: Option<SnapcompactToolResults>,
	pub saved_tokens: u64,
}

/// Context usage breakdown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextBreakdown {
	pub context_window: u64,
	pub used_tokens: u64,
	pub free_tokens: u64,
	pub auto_compact_buffer_tokens: u64,
	pub threshold_tokens: Option<u64>,
	pub snapcompact: Option<SnapcompactBreakdown>,
}

/// Context view data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextViewData {
	pub categories: Vec<ContextViewCategory>,
	pub breakdown: ContextBreakdown,
}

/// Map cell (window order planning output).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextMapCell {
	pub id: String,
	pub full: bool,
}

/// Meter category input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeterCategory {
	pub label: String,
	pub tokens: u64,
	pub color: String,
}

/// Meter data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextMeterData {
	pub context_window: u64,
	pub used_tokens: u64,
	pub free_tokens: u64,
	pub auto_compact_buffer_tokens: u64,
	pub threshold_tokens: Option<u64>,
	pub categories: Vec<MeterCategory>,
}

/// Stacked meter part.
#[derive(Clone, Debug, PartialEq)]
pub struct MeterPart {
	pub value: f64,
	pub token: Option<String>,
	pub label: Option<String>,
	pub hatch: bool,
}

/// Usage summary labels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextUsageSummary {
	pub used: String,
	pub detail: String,
	pub room: String,
}

fn format_number(n: u64) -> String {
	let s = n.to_string();
	let mut out = String::with_capacity(s.len() + s.len() / 3);
	for (i, ch) in s.chars().rev().enumerate() {
		if i > 0 && i % 3 == 0 {
			out.push(',');
		}
		out.push(ch);
	}
	out.chars().rev().collect()
}

/// Largest-remainder split of `total` by `weights`; None when weights cannot scale.
#[must_use]
pub fn allocate_tokens(weights: &[u64], total: u64) -> Option<Vec<u64>> {
	let sum: u128 = weights.iter().map(|&w| w as u128).sum();
	if sum == 0 {
		return None;
	}
	let mut shares: Vec<(usize, u64, f64)> = weights
		.iter()
		.enumerate()
		.map(|(index, &w)| {
			let exact = (total as f64) * (w as f64) / (sum as f64);
			let floor = exact.floor() as u64;
			(index, floor, exact - floor as f64)
		})
		.collect();
	let used: u64 = shares.iter().map(|s| s.1).sum();
	let remaining = total.saturating_sub(used) as usize;
	let mut ranked = shares.clone();
	ranked.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0)));
	for i in 0..remaining.min(ranked.len()) {
		let idx = ranked[i].0;
		if let Some(s) = shares.iter_mut().find(|s| s.0 == idx) {
			s.1 += 1;
		}
	}
	Some(shares.into_iter().map(|s| s.1).collect())
}

/// Attribute current usage to categories by weight, then children.
#[must_use]
pub fn allocate_usage(categories: &[ContextViewCategory], usage: u64) -> Option<std::collections::HashMap<String, u64>> {
	fn allocate(
		siblings: &[(String, u64, Vec<(String, u64)>)],
		total: u64,
		out: &mut std::collections::HashMap<String, u64>,
	) -> bool {
		let weights: Vec<u64> = siblings.iter().map(|s| s.1).collect();
		let Some(shares) = allocate_tokens(&weights, total) else {
			return false;
		};
		for (i, (key, _, children)) in siblings.iter().enumerate() {
			let share = shares[i];
			out.insert(key.clone(), share);
			if !children.is_empty() {
				let child_siblings: Vec<(String, u64, Vec<(String, u64)>)> =
					children.iter().map(|c| (c.0.clone(), c.1, vec![])).collect();
				let _ = allocate(&child_siblings, share, out);
			}
		}
		true
	}
	let mut out = std::collections::HashMap::new();
	let top: Vec<(String, u64, Vec<(String, u64)>)> = categories
		.iter()
		.map(|c| {
			(
				c.id.clone(),
				c.tokens,
				c.children.iter().map(|ch| (ch.key.clone(), ch.tokens)).collect(),
			)
		})
		.collect();
	if allocate(&top, usage, &mut out) { Some(out) } else { None }
}

/// Overview row label for a category.
#[must_use]
pub fn overview_label(category: &ContextViewCategory) -> String {
	category.overview_label.clone().unwrap_or_else(|| category.label.clone())
}

/// Format share percentage.
#[must_use]
pub fn format_share(tokens: u64, window: u64) -> String {
	if window == 0 {
		return "—".to_string();
	}
	let percent = (tokens as f64 / window as f64) * 100.0;
	if percent > 0.0 && percent < 0.05 {
		"<0.1%".to_string()
	} else {
		format!("{percent:.1}%")
	}
}

/// Build overview rows.
#[must_use]
pub fn build_overview_rows(
	data: &ContextViewData,
	allocation: Option<&std::collections::HashMap<String, u64>>,
) -> Vec<OverviewRow> {
	struct Raw {
		key: String,
		id: String,
		label: String,
		tokens: Option<u64>,
		openable: bool,
		child: bool,
	}
	let mut raws: Vec<Raw> = Vec::new();
	for cat in &data.categories {
		let openable = cat.openable && cat.entries_count > 0;
		raws.push(Raw {
			key: cat.id.clone(),
			id: cat.id.clone(),
			label: overview_label(cat),
			tokens: allocation.and_then(|a| a.get(&cat.id).copied()),
			openable,
			child: false,
		});
		for child in &cat.children {
			raws.push(Raw {
				key: child.key.clone(),
				id: cat.id.clone(),
				label: child.label.clone(),
				tokens: allocation.and_then(|a| a.get(&child.key).copied()),
				openable: cat.openable,
				child: true,
			});
		}
	}
	raws.push(Raw {
		key: "buffer".into(),
		id: "buffer".into(),
		label: "Auto-Compact Buffer".into(),
		tokens: Some(data.breakdown.auto_compact_buffer_tokens),
		openable: false,
		child: false,
	});
	raws.push(Raw {
		key: "free".into(),
		id: "free".into(),
		label: "Free Space".into(),
		tokens: Some(data.breakdown.free_tokens),
		openable: false,
		child: false,
	});
	let token_width = raws
		.iter()
		.map(|r| r.tokens.map(|t| format_number(t).len()).unwrap_or(1))
		.max()
		.unwrap_or(1);
	let share_width = raws
		.iter()
		.map(|r| {
			r.tokens
				.map(|t| format_share(t, data.breakdown.context_window).len())
				.unwrap_or(1)
		})
		.max()
		.unwrap_or(1);
	raws.into_iter()
		.map(|r| {
			let token_column = r.tokens.map(|t| format!("{:>w$}", format_number(t), w = token_width)).unwrap_or_else(|| "—".to_string());
			let share_column = r
				.tokens
				.map(|t| format!("  {:>w$}", format_share(t, data.breakdown.context_window), w = share_width))
				.unwrap_or_default();
			OverviewRow {
				key: r.key,
				id: r.id,
				label: r.label,
				tokens: r.tokens,
				openable: r.openable,
				child: r.child,
				token_column,
				share_column,
			}
		})
		.collect()
}

/// Map size beside legend.
#[must_use]
pub fn map_geometry(width: usize, height: usize) -> Option<MapGeometry> {
	if width < MAP_MIN_WIDTH || height < 2 {
		return None;
	}
	let gap = if width >= MAP_GAP_MIN_WIDTH { " ".to_string() } else { String::new() };
	let cell_width = 1usize;
	let avail = width.saturating_sub(LEGEND_MIN_WIDTH + MAP_LEGEND_GAP);
	let denom = cell_width + gap.len();
	if denom == 0 {
		return None;
	}
	let columns = ((avail + gap.len()) / denom).min(MAP_MAX_CELLS);
	if columns < 1 {
		return None;
	}
	let rows = height.min(MAP_MAX_CELLS);
	let map_width = columns * (cell_width + gap.len()) - gap.len();
	Some(MapGeometry { columns, rows, cell_width, gap, width: map_width })
}

/// Plan map rows in window order: categories, free space, buffer.
#[must_use]
pub fn plan_map_rows(
	data: &ContextViewData,
	allocation: &std::collections::HashMap<String, u64>,
	geometry: &MapGeometry,
) -> Vec<Vec<ContextMapCell>> {
	let total_cells = geometry.columns * geometry.rows;
	if total_cells == 0 || data.breakdown.context_window == 0 {
		return vec![vec![]; geometry.rows];
	}
	let mut segments: Vec<(String, u64)> = data
		.categories
		.iter()
		.map(|c| (c.id.clone(), allocation.get(&c.id).copied().unwrap_or(0)))
		.collect();
	segments.push(("free".into(), data.breakdown.free_tokens));
	segments.push(("buffer".into(), data.breakdown.auto_compact_buffer_tokens));
	// largest-remainder allocation of cells by tokens
	let window = data.breakdown.context_window as f64;
	let mut exacts: Vec<(usize, usize, f64)> = segments
		.iter()
		.enumerate()
		.map(|(i, (_, t))| {
			let ex = (*t as f64 / window) * total_cells as f64;
			(i, ex.floor() as usize, ex - ex.floor())
		})
		.collect();
	let used: usize = exacts.iter().map(|e| e.1).sum();
	let mut rem = total_cells.saturating_sub(used);
	let mut ranked = exacts.clone();
	ranked.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0)));
	for r in ranked.iter().take(rem.min(ranked.len())) {
		if let Some(e) = exacts.iter_mut().find(|e| e.0 == r.0) {
			e.1 += 1;
		}
	}
	let mut cells: Vec<ContextMapCell> = Vec::with_capacity(total_cells);
	for (i, count, _) in exacts {
		for _ in 0..count {
			cells.push(ContextMapCell { id: segments[i].0.clone(), full: true });
		}
	}
	cells.truncate(total_cells);
	while cells.len() < total_cells {
		cells.push(ContextMapCell { id: "free".into(), full: false });
	}
	(0..geometry.rows)
		.map(|r| {
			let start = r * geometry.columns;
			let end = (start + geometry.columns).min(cells.len());
			cells[start..end].to_vec()
		})
		.collect()
}

/// Snapcompact savings lines.
#[must_use]
pub fn context_snapcompact_lines(breakdown: &ContextBreakdown) -> Vec<String> {
	let Some(snap) = &breakdown.snapcompact else {
		return Vec::new();
	};
	if !snap.vision_capable {
		return vec!["Snapcompact: inactive (model has no image input)".to_string()];
	}
	let mut lines = vec!["Snapcompact (estimated wire savings)".to_string()];
	if let Some(sp) = &snap.system_prompt {
		let scope = if sp.scope == "agents-md" { "AGENTS.md" } else { "all" };
		if sp.applied {
			let frames = format!("{} frame{}", sp.frames, if sp.frames == 1 { "" } else { "s" });
			lines.push(format!(
				"  System prompt ({scope}): saves ~{} ({} text → {frames} ≈ {})",
				format_number(sp.saved_tokens),
				format_number(sp.text_tokens),
				format_number(sp.image_tokens)
			));
		} else {
			let reason = match sp.reason.as_deref() {
				Some("budget") => "image budget exhausted",
				Some("empty") => "nothing to image",
				_ => "frames would not save tokens",
			};
			lines.push(format!("  System prompt ({scope}): stays text ({reason})"));
		}
	}
	if let Some(tr) = &snap.tool_results {
		if tr.swapped > 0 {
			lines.push(format!(
				"  Tool results: saves ~{} ({}/{} imaged, {} text → {} frames ≈ {})",
				format_number(tr.saved_tokens),
				tr.swapped,
				tr.total,
				format_number(tr.text_tokens),
				tr.frames,
				format_number(tr.image_tokens)
			));
		} else {
			lines.push(format!("  Tool results: none imaged ({} in history)", tr.total));
		}
	}
	if snap.saved_tokens > 0 {
		let next = breakdown.used_tokens.saturating_sub(snap.saved_tokens);
		lines.push(format!("  Next request: ~{} tokens on the wire", format_number(next)));
	}
	lines
}

fn percent_string(part: u64, whole: u64, fraction_digits: usize) -> String {
	if whole == 0 {
		return "0%".to_string();
	}
	let pct = (part as f64 / whole as f64) * 100.0;
	if pct > 0.0 && pct < 0.05 {
		"<0.1%".to_string()
	} else {
		format!("{pct:.prec$}%", prec = fraction_digits)
	}
}

/// Stacked meter parts.
#[must_use]
pub fn context_meter_parts(breakdown: &ContextMeterData) -> Vec<MeterPart> {
	let window = breakdown.context_window.max(1) as f64;
	let mut parts: Vec<MeterPart> = breakdown
		.categories
		.iter()
		.filter(|c| c.tokens > 0)
		.map(|c| MeterPart {
			value: c.tokens as f64 / window,
			token: Some(c.color.clone()),
			label: Some(format!(
				"{} · {} tokens ({})",
				c.label,
				format_number(c.tokens),
				percent_string(c.tokens, breakdown.context_window, 1)
			)),
			hatch: false,
		})
		.collect();
	parts.push(MeterPart {
		value: breakdown.free_tokens as f64 / window,
		token: Some("track".into()),
		label: Some(format!(
			"Free space · {} tokens ({})",
			format_number(breakdown.free_tokens),
			percent_string(breakdown.free_tokens, breakdown.context_window, 1)
		)),
		hatch: false,
	});
	if breakdown.auto_compact_buffer_tokens > 0 {
		parts.push(MeterPart {
			value: breakdown.auto_compact_buffer_tokens as f64 / window,
			token: Some("warning".into()),
			label: Some(format!(
				"Autocompact buffer · {} tokens ({})",
				format_number(breakdown.auto_compact_buffer_tokens),
				percent_string(breakdown.auto_compact_buffer_tokens, breakdown.context_window, 1)
			)),
			hatch: true,
		});
	}
	parts
}

/// Shared usage and compaction labels.
#[must_use]
pub fn context_usage_summary(
	breakdown: &ContextBreakdown,
) -> ContextUsageSummary {
	let window = breakdown.context_window;
	let used = breakdown.used_tokens;
	let room = match breakdown.threshold_tokens {
		None => format!("{} free", format_number(breakdown.free_tokens)),
		Some(th) if used >= th => format!(
			"Past auto-compaction at {}",
			percent_string(th, window, 0)
		),
		Some(th) => format!(
			"{} left before auto-compaction at {}",
			format_number(th.saturating_sub(used)),
			percent_string(th, window, 0)
		),
	};
	ContextUsageSummary {
		used: format_number(used),
		detail: format!(" of {} tokens · {}", format_number(window), percent_string(used, window, 0)),
		room,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn allocate_tokens_largest_remainder() {
		assert_eq!(allocate_tokens(&[1, 1, 1], 10), Some(vec![4, 3, 3]));
		assert_eq!(allocate_tokens(&[0, 0], 5), None);
	}

	#[test]
	fn format_share_and_overview_rows() {
		assert_eq!(format_share(1, 10000), "<0.1%");
		assert_eq!(format_share(500, 1000), "50.0%");
		assert_eq!(format_share(0, 0), "—");
		let data = ContextViewData {
			categories: vec![ContextViewCategory {
				id: "sys".into(),
				label: "System".into(),
				overview_label: None,
				tokens: 100,
				entries_count: 1,
				children: vec![],
				openable: true,
			}],
			breakdown: ContextBreakdown {
				context_window: 1000,
				used_tokens: 100,
				free_tokens: 800,
				auto_compact_buffer_tokens: 100,
				threshold_tokens: Some(900),
				snapcompact: None,
			},
		};
		let rows = build_overview_rows(&data, None);
		assert!(rows.iter().any(|r| r.key == "sys"));
		assert!(rows.iter().any(|r| r.key == "buffer"));
		assert!(rows.iter().any(|r| r.key == "free"));
	}

	#[test]
	fn map_geometry_and_snapcompact_and_usage_summary() {
		assert!(map_geometry(40, 10).is_none());
		let g = map_geometry(80, 10).expect("geom");
		assert!(g.columns >= 1);
		let bd = ContextBreakdown {
			context_window: 1000,
			used_tokens: 400,
			free_tokens: 500,
			auto_compact_buffer_tokens: 100,
			threshold_tokens: Some(900),
			snapcompact: Some(SnapcompactBreakdown {
				vision_capable: true,
				system_prompt: Some(SnapcompactSystemPrompt {
					scope: "all".into(),
					applied: true,
					frames: 2,
					saved_tokens: 50,
					text_tokens: 100,
					image_tokens: 50,
					reason: None,
				}),
				tool_results: None,
				saved_tokens: 50,
			}),
		};
		let lines = context_snapcompact_lines(&bd);
		assert!(lines.iter().any(|l| l.contains("System prompt")));
		assert!(lines.iter().any(|l| l.contains("Next request")));
		let s = context_usage_summary(&bd);
		assert!(s.detail.contains("of"));
		assert!(s.room.contains("left before auto-compaction"));
	}

	#[test]
	fn meter_parts_include_track_and_buffer() {
		let md = ContextMeterData {
			context_window: 1000,
			used_tokens: 300,
			free_tokens: 600,
			auto_compact_buffer_tokens: 100,
			threshold_tokens: Some(900),
			categories: vec![MeterCategory { label: "Sys".into(), tokens: 200, color: "accent".into() }],
		};
		let parts = context_meter_parts(&md);
		assert!(parts.iter().any(|p| p.token.as_deref() == Some("track")));
		assert!(parts.iter().any(|p| p.hatch));
	}
}
