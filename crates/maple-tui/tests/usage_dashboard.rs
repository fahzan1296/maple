//! Port of `l/test/usage-dashboard.test.ts` pure-layout cases.
//!
//! Deviations: the reference builds cards from raw account reports; the crate
//! exposes [`build_provider_cards`] over already-built [`ProviderCard`]s, so
//! these tests feed cards directly. Interactive overlay navigation is covered
//! by the component harness in the reference and is not portable here.

use maple_tui::overlays::usage_dashboard::{
	build_heatmap_layout, build_provider_cards, format_activity_error_detail, format_provider_name,
	CardResetCredits, CardWindowRow, DailyActivityPoint, ProviderCard,
};

fn point(day: &str, cost: f64, requests: u64) -> DailyActivityPoint {
	DailyActivityPoint {
		day: day.to_string(),
		cost,
		requests,
	}
}

fn window(label: &str, fraction: Option<f64>) -> CardWindowRow {
	CardWindowRow {
		label: label.to_string(),
		window_tag: None,
		fraction,
		status: "ok".to_string(),
		reset_ms: None,
		used_text: None,
	}
}

fn card(provider: &str, windows: Vec<CardWindowRow>) -> ProviderCard {
	ProviderCard {
		provider: provider.to_string(),
		name: format_provider_name(provider),
		accounts: 1,
		unavailable_accounts: Vec::new(),
		windows,
		unlimited: false,
		notes: Vec::new(),
		idle: false,
		reset_credits: None,
		daybreak_accounts: Vec::new(),
	}
}

#[test]
fn aligns_days_monday_first_and_marks_future_days_null() {
	let layout = build_heatmap_layout(&[point("2026-08-31", 5.0, 0)], 2, "2026-08-31");
	assert_eq!(layout.cells[0][1], Some(4));
	for row in 1..7 {
		assert_eq!(layout.cells[row][1], None);
	}
	for row in 0..7 {
		assert_eq!(layout.cells[row][0], Some(0));
	}
	assert_eq!(layout.start_day, "2026-08-24");
}

#[test]
fn scales_intensity_by_magnitude_against_the_busiest_day() {
	let layout = build_heatmap_layout(
		&[
			point("2026-08-25", 30.0, 0),
			point("2026-08-26", 6.0, 0),
			point("2026-08-27", 0.0, 0),
		],
		2,
		"2026-08-31",
	);
	assert_eq!(layout.cells[0][0], Some(4));
	assert_eq!(layout.cells[1][0], Some(3));
	assert_eq!(layout.cells[2][0], Some(1));
	assert_eq!(layout.cells[3][0], Some(0));
}

#[test]
fn falls_back_to_request_counts_when_nothing_is_priced() {
	let layout = build_heatmap_layout(
		&[point("2026-08-24", 0.0, 50), point("2026-08-25", 0.0, 3)],
		2,
		"2026-08-31",
	);
	assert_eq!(layout.cells[0][0], Some(4));
	assert_eq!(layout.cells[1][0], Some(1));
	assert_eq!(layout.total_requests, 53);
}

#[test]
fn labels_a_column_when_its_week_starts_a_new_month() {
	let layout = build_heatmap_layout(&[], 6, "2026-08-31");
	assert_eq!(layout.month_labels[0].as_deref(), Some("Jul"));
	let labels: Vec<&str> = layout
		.month_labels
		.iter()
		.filter_map(|label| label.as_deref())
		.collect();
	assert_eq!(labels, vec!["Jul", "Aug"]);
}

#[test]
fn sorts_pressured_providers_first_and_collapses_untouched_into_idle() {
	let cards = build_provider_cards(vec![
		card("cursor", vec![window("Cursor Models", Some(0.0))]),
		card("openai-codex", vec![window("7 days", Some(0.4))]),
		card("ollama-cloud", Vec::new()),
	]);
	assert_eq!(cards[0].provider, "openai-codex");
	assert!(!cards[0].idle);
	let mut idle: Vec<&str> = cards
		.iter()
		.filter(|c| c.idle)
		.map(|c| c.provider.as_str())
		.collect();
	idle.sort_unstable();
	assert_eq!(idle, vec!["cursor", "ollama-cloud"]);
	assert!(cards.iter().find(|c| c.provider == "ollama-cloud").unwrap().unlimited);
}

#[test]
fn keeps_an_exhausted_bucket_past_the_row_cap_visible() {
	let buckets = ["a", "b", "c", "d", "e"]
		.iter()
		.enumerate()
		.map(|(index, model)| {
			let fraction = if index == 4 {
				1.0
			} else {
				0.1 * (index as f64 + 1.0)
			};
			window(&format!("Model {model}"), Some(fraction))
		})
		.collect();
	let cards = build_provider_cards(vec![card("google-gemini-cli", buckets)]);
	let labels: Vec<&str> = cards[0]
		.windows
		.iter()
		.map(|w| w.label.as_str())
		.collect();
	assert_eq!(labels, vec!["Model e", "Model d", "Model c", "Model b", "Model a"]);
}

#[test]
fn dedupes_notes_and_keeps_reset_credit_cards_out_of_idle() {
	let mut input = card("anthropic", vec![window("Claude 5 Hour", Some(0.0))]);
	input.notes = vec!["zeta".into(), "alpha".into(), "zeta".into()];
	input.reset_credits = Some(CardResetCredits {
		banked_count: 5,
		redeemable_count: 2,
		soonest_expiry_ms: Some(17_280_000_000),
		unavailable_reasons: vec!["weekly cooldown".into()],
	});
	let cards = build_provider_cards(vec![input]);
	assert_eq!(cards[0].notes, vec!["alpha", "zeta"]);
	assert!(!cards[0].idle);
}

#[test]
fn formats_provider_names_from_slugs() {
	assert_eq!(format_provider_name("openai-codex"), "Openai Codex");
	assert_eq!(format_provider_name("cursor"), "Cursor");
	assert_eq!(format_provider_name(""), "Provider");
}

#[test]
fn sanitizes_activity_errors_and_shortens_home_paths() {
	assert_eq!(
		format_activity_error_detail("worker spawn failed\ntrace\x1b[2J\r\n\tsecond line", None),
		"worker spawn failed trace second line"
	);
	assert_eq!(
		format_activity_error_detail(
			"Error: failed to open /home/fahzan/.omp/stats.db...",
			Some("/home/fahzan"),
		),
		"Error: failed to open ~/.omp/stats.db"
	);
}
