// Ported subset of l/test/table-chart.test.ts (pure parse/analyze/plan/build/is_transition cases)
// to Rust charts APIs. Deviations: Rust types/enums/field names differ (Cell/NumberCell/Dimension/
// CellFit/ChartKind/ChartPlan/ChartSpec), and large TS planner/renderer suites (SVG, status, ladders,
// etc.) are not exhaustively portable here; assertions map to equivalent Rust semantics for covered cases.

use maple_tui::charts::chart_plan::{build_chart, is_transition, plan_chart, ChartKind};
use maple_tui::charts::table_data::{analyze_table, parse_cell, Cell, CellFit, Dimension};

fn analyze(header: &[&str], rows: &[&[&str]]) -> maple_tui::charts::table_data::TableAnalysis {
    analyze_table(
        &header.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        &rows
            .iter()
            .map(|r| r.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
    )
}

#[test]
fn parse_cell_normalizes_common_units_and_kinds() {
    match parse_cell("1.2 s") {
        Cell::Number(n) => {
            assert!((n.value - 1.2).abs() < 1e-9);
            assert_eq!(n.dim, Dimension::Duration);
        }
        _ => panic!("expected number"),
    }
    match parse_cell("`900 ms`") {
        Cell::Number(n) => {
            assert!((n.value - 0.9).abs() < 1e-9);
            assert_eq!(n.dim, Dimension::Duration);
            assert!(n.figure.contains("900"));
        }
        _ => panic!("expected number"),
    }
    match parse_cell("1.5 KiB") {
        Cell::Number(n) => {
            assert!((n.value - 1536.0).abs() < 1e-9);
            assert_eq!(n.dim, Dimension::Bytes);
        }
        _ => panic!("expected number"),
    }
    assert!(parse_cell("#618").as_number().is_none());
    assert!(parse_cell("v1.2.3").as_number().is_none());
    assert!(parse_cell("2026-01-02").as_number().is_none());
    assert!(parse_cell("renamed the helper").as_number().is_none());
}

#[test]
fn parse_cell_reads_ranges_and_arrows() {
    match parse_cell("24–72 h") {
        Cell::Number(n) => assert_eq!(n.fit, CellFit::Range),
        _ => panic!("expected number"),
    }
    match parse_cell("2,049 → 1,973") {
        Cell::Number(n) => {
            assert_eq!(n.fit, CellFit::Arrow);
            assert!(n.to.is_some());
        }
        _ => panic!("expected number"),
    }
}

#[test]
fn analyze_table_labels_by_text_column_and_excludes_totals() {
    let shares = analyze(
        &["#", "Bucket", "Tokens", "Share"],
        &[
            &["1", "Tool results", "2,020,235", "74.6%"],
            &["2", "Tool args", "544,859", "20.1%"],
            &["3", "User text", "92,905", "3.4%"],
            &["4", "Assistant text", "48,834", "1.9%"],
            &["", "Total", "2,706,833", "100%"],
        ],
    );
    assert_eq!(shares.label.as_ref().map(|c| c.header.as_str()), Some("Bucket"));
    assert_eq!(shares.rows, vec![0, 1, 2, 3]);
    assert!(shares.totals.contains(&4));
    assert_eq!(
        shares.measures.iter().map(|c| c.header.as_str()).collect::<Vec<_>>(),
        vec!["Tokens", "Share"]
    );
}

#[test]
fn is_transition_distinguishes_condition_headers() {
    assert!(!is_transition("Median", "Runs with ≥10"));
    assert!(is_transition("build without cache", "build with cache"));
    assert!(is_transition("Before (kata)", "After (vmon)"));
}

#[test]
fn plan_and_build_simple_bar_and_share_shapes() {
    let timings = analyze(
        &["Section", "Time"],
        &[
            &["generate_analysis", "10879 ms (98.6%)"],
            &["git_commit", "20.5 ms"],
            &["collect_context", "1.2 s"],
        ],
    );
    let plan = plan_chart(&timings).expect("timings should chart");
    assert_eq!(plan.kind, ChartKind::Bar);
    assert!(build_chart(&timings, &plan).is_some());

    let shares = analyze(
        &["#", "Bucket", "Tokens", "Share"],
        &[
            &["1", "Tool results", "2,020,235", "74.6%"],
            &["2", "Tool args", "544,859", "20.1%"],
            &["3", "User text", "92,905", "3.4%"],
            &["4", "Assistant text", "48,834", "1.9%"],
            &["", "Total", "2,706,833", "100%"],
        ],
    );
    let plan = plan_chart(&shares).expect("shares should chart");
    assert_eq!(plan.kind, ChartKind::Share);
    assert!(build_chart(&shares, &plan).is_some());
}

#[test]
fn prose_tables_or_two_values_do_not_plan_a_chart() {
    let prose = analyze(
        &["File", "Change"],
        &[&["a.ts", "renamed the helper"], &["b.ts", "dropped the shim"]],
    );
    assert!(plan_chart(&prose).is_none());
    let two = analyze(&["k", "v"], &[&["a", "1"], &["b", "2"]]);
    assert!(plan_chart(&two).is_none());
}
