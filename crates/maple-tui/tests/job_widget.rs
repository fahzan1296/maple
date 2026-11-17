//! Port of `l/test/job-widget.test.ts`.
//!
//! The crate exposes the widget data model and the glance/summary helpers, but
//! not the native tile/chart/meter/badge composition or the themed block
//! renderer. Only the `jobWidgetGlance` cases are portable; the rest are
//! ignored with the missing APIs named.

use maple_tui::tools::job_widget::{
	JobWidgetBlock, JobWidgetHeadline, JobWidgetMeterBlock, JobWidgetState, JobWidgetTrend,
	JobWidgetView, TrendTone, job_widget_glance,
};

fn trial_sweep() -> JobWidgetView {
	JobWidgetView {
		title: "ablation · 9 trials".to_string(),
		headline: Some(JobWidgetHeadline {
			text: "0.842".to_string(),
			label: Some("best".to_string()),
			trend: Some(JobWidgetTrend {
				text: "▲ 2.1%".to_string(),
				tone: TrendTone::Success,
			}),
		}),
		stage: None,
		blocks: vec![
			JobWidgetBlock::Meter(JobWidgetMeterBlock {
				label: "trials".to_string(),
				value: 7.0 / 9.0,
				text: "7/9 trials".to_string(),
				tone: None,
			}),
			JobWidgetBlock::Text("trial 8: lr=3e-4\twarmup=500 seed=7".to_string()),
		],
		state: JobWidgetState::Live,
		updated_at: 1_760_000_003_000,
	}
}

#[test]
fn reads_a_meters_whole_where_the_headline_repeats_only_its_numerator() {
	let build = JobWidgetView {
		title: "flameshot build".to_string(),
		headline: Some(JobWidgetHeadline {
			text: "140".to_string(),
			label: None,
			trend: None,
		}),
		stage: None,
		blocks: vec![JobWidgetBlock::Meter(JobWidgetMeterBlock {
			label: "steps".to_string(),
			value: 140.0 / 190.0,
			text: "140/190".to_string(),
			tone: None,
		})],
		state: JobWidgetState::Live,
		updated_at: 0,
	};
	assert_eq!(job_widget_glance(&build).expect("glance").text, "140/190");
	assert_eq!(job_widget_glance(&trial_sweep()).expect("glance").text, "0.842");
}

#[test]
#[ignore = "missing API: chart/meter/badge native node rendering in describe_job_widget"]
fn draws_a_spark_as_a_bars_chart_with_its_domain_and_target_mark() {}

#[test]
#[ignore = "missing API: chart/meter/badge native node rendering in describe_job_widget"]
fn maps_bars_meter_grid_and_stat_blocks_onto_meters_badges_and_toned_spans() {}

#[test]
#[ignore = "missing API: omp.job.widget.head native head row role"]
fn puts_the_headline_and_a_sanitized_stage_in_the_head_row() {}

#[test]
#[ignore = "missing API: omp.jobs.hovercard / omp.job.tiles hovercard composition"]
fn lays_running_jobs_out_as_clockless_tiles_in_an_id_less_hover_card() {}

#[test]
#[ignore = "missing API: describe_job_tile / render_job_tile_lines / JobTile"]
fn puts_status_title_id_and_clock_over_the_tile_body() {}

#[test]
#[ignore = "missing API: describe_job_tile / render_job_tile_lines / JobTile"]
fn names_a_subagents_tile_by_the_subagent_with_a_labelled_ring() {}

#[test]
#[ignore = "missing API: describe_job_tile / render_job_tile_lines / JobTile"]
fn splits_a_ratio_figure_and_lists_the_first_keys_of_a_bars_block() {}

#[test]
#[ignore = "missing API: describe_job_tile / render_job_tile_lines / JobTile"]
fn gives_a_tile_with_nothing_to_chart_its_stage_as_the_body() {}

#[test]
#[ignore = "missing API: describe_job_tile / render_job_tile_lines / JobTile"]
fn draws_nothing_of_a_widget_no_spec_drives() {}

#[test]
#[ignore = "missing API: render_job_tile_lines"]
fn draws_the_tile_title_line_with_the_figure_and_meta() {}

#[test]
#[ignore = "missing API: width-aware themed block renderer in render_job_widget_lines"]
fn keeps_every_line_of_every_block_within_the_width() {}

#[test]
#[ignore = "missing API: width-aware themed block renderer in render_job_widget_lines"]
fn draws_a_spark_as_a_block_sparkline_with_the_latest_value() {}

#[test]
#[ignore = "missing API: width-aware themed block renderer in render_job_widget_lines"]
fn aligns_bars_under_one_label_column_with_right_aligned_values() {}

#[test]
#[ignore = "missing API: width-aware themed block renderer in render_job_widget_lines"]
fn draws_a_meter_a_wrapped_status_grid_and_a_sanitized_text_line() {}
