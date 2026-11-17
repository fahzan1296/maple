//! Splits a rendered form body into pages of a fixed row budget.
//!
//! Reference: `l/src/html-form/paginate.ts`.
//!
//! The body arrives as atoms: unbreakable row runs (an option with its wrapped
//! label, a text field with its input, one line of a paragraph). Each atom ranks
//! how good a page break before it is; a page ends at the break that best
//! trades rank against the rows it leaves empty ([`break_score`]), so a
//! section boundary wins unless it strands most of the page, and a break
//! never lands before a [`BreakRank::Never`] atom (a heading's first line,
//! a block's widow) unless nothing else fits.

/// How good a page break before an atom is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum BreakRank {
    /// Keep with the previous atom (the line after a heading, an error after its field).
    #[default]
    Never = 0,
    /// Inside a block: between options of a group, lines of a paragraph.
    Inside = 1,
    /// Between blocks of one section.
    Block = 2,
    /// Between sections.
    Section = 3,
}

/// One unbreakable run of body rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageAtom {
    /// Rows of the run.
    pub lines: Vec<String>,
    /// Blank rows before this atom; dropped when the atom starts a page.
    pub gap: usize,
    /// How good a break before this atom is.
    pub break_rank: BreakRank,
    /// Title repeated atop a page that starts at this atom (`Database (cont.)`);
    /// costs one row.
    pub continuation: Option<String>,
}

/// Atoms `[start, end)` of one page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub start: usize,
    pub end: usize,
}

/// Rows a page uses when it holds atoms `[start, end)`.
pub fn page_height(atoms: &[PageAtom], start: usize, end: usize) -> usize {
    let mut used = usize::from(atoms.get(start).and_then(|atom| atom.continuation.as_ref()).is_some());
    for (index, atom) in atoms.iter().enumerate().take(end).skip(start) {
        used += if index == start { 0 } else { atom.gap } + atom.lines.len();
    }
    used
}

/// Pack atoms into pages of at most `rows` rows. An atom taller than a page
/// gets a page of its own (the caller clips it). Always returns at least one page.
pub fn paginate(atoms: &[PageAtom], rows: usize) -> Vec<Page> {
    let budget = rows.max(1);
    let mut pages = Vec::new();
    let mut start = 0;
    while start < atoms.len() {
        let mut end = start + 1;
        while end < atoms.len() && page_height(atoms, start, end + 1) <= budget {
            end += 1;
        }
        if end < atoms.len() {
            end = best_break(atoms, start, end, budget);
        }
        pages.push(Page { start, end });
        start = end;
    }
    if pages.is_empty() {
        pages.push(Page { start: 0, end: 0 });
    }
    pages
}

/// Worth of ending a page with `used` of `budget` rows at a break of `rank`:
/// the rank's bonus minus the fraction left empty. A section break beats a
/// full page while it leaves under half the page empty; a block break, under a quarter.
fn break_score(rank: BreakRank, used: usize, budget: usize) -> f64 {
    let bonus = match rank {
        BreakRank::Section => 0.5,
        BreakRank::Block => 0.25,
        _ => 0.0,
    };
    bonus - budget.saturating_sub(used) as f64 / budget as f64
}

/// Pick the break in `(start, end]` for a page that greedily filled up to `end`:
/// the allowed break with the best [`break_score`] (latest on ties), else
/// `end` itself when every candidate is [`BreakRank::Never`].
fn best_break(atoms: &[PageAtom], start: usize, end: usize, budget: usize) -> usize {
    let mut best = end;
    let mut best_score = f64::NEG_INFINITY;
    for candidate in (start + 1..=end).rev() {
        let rank = atoms[candidate].break_rank;
        if rank == BreakRank::Never {
            continue;
        }
        let score = break_score(rank, page_height(atoms, start, candidate), budget);
        if score > best_score {
            best = candidate;
            best_score = score;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(lines: &[&str], gap: usize, rank: BreakRank) -> PageAtom {
        PageAtom {
            lines: lines.iter().map(|line| (*line).to_string()).collect(),
            gap,
            break_rank: rank,
            continuation: None,
        }
    }

    #[test]
    fn empty_atoms_yield_one_empty_page() {
        assert_eq!(paginate(&[], 10), vec![Page { start: 0, end: 0 }]);
    }

    #[test]
    fn page_height_counts_gaps_lines_and_continuation() {
        let atoms = vec![
            PageAtom { lines: vec!["a".into()], gap: 1, break_rank: BreakRank::Section, continuation: Some("T".into()) },
            PageAtom { lines: vec!["b".into(), "c".into()], gap: 2, break_rank: BreakRank::Block, continuation: None },
            PageAtom { lines: vec!["d".into()], gap: 3, break_rank: BreakRank::Inside, continuation: None },
        ];
        assert_eq!(page_height(&atoms, 0, 1), 2);
        assert_eq!(page_height(&atoms, 0, 2), 2 + 2 + 2);
        assert_eq!(page_height(&atoms, 1, 3), 2 + 2 + 3 + 1);
        assert_eq!(page_height(&atoms, 2, 3), 1);
    }

    #[test]
    fn tall_atom_gets_a_page_of_its_own() {
        let atoms = vec![
            atom(&["one"], 0, BreakRank::Inside),
            atom(&["a", "b", "c", "d", "e"], 0, BreakRank::Inside),
            atom(&["two"], 0, BreakRank::Inside),
        ];
        assert_eq!(
            paginate(&atoms, 3),
            vec![
                Page { start: 0, end: 1 },
                Page { start: 1, end: 2 },
                Page { start: 2, end: 3 },
            ]
        );
    }

    #[test]
    fn section_break_wins_over_a_full_page() {
        // Four rows fit; the fifth would overflow, so the section break at atom 4
        // (leaving one row empty) beats filling the page.
        let atoms = vec![
            atom(&["a"], 0, BreakRank::Section),
            atom(&["b"], 0, BreakRank::Inside),
            atom(&["c"], 0, BreakRank::Inside),
            atom(&["d"], 0, BreakRank::Inside),
            atom(&["e"], 1, BreakRank::Section),
            atom(&["f"], 0, BreakRank::Inside),
        ];
        let pages = paginate(&atoms, 4);
        assert_eq!(pages[0], Page { start: 0, end: 4 });
        assert_eq!(pages[1], Page { start: 4, end: 6 });
    }
    #[test]
    fn never_break_is_skipped_unless_nothing_else_fits() {
        let atoms = vec![
            atom(&["h"], 0, BreakRank::Never),
            atom(&["a"], 0, BreakRank::Never),
            atom(&["b"], 0, BreakRank::Inside),
        ];
        // Budget 1: each atom needs its own page, and every candidate break is
        // Never, so each page fills to its greedy end.
        assert_eq!(
            paginate(&atoms, 1),
            vec![
                Page { start: 0, end: 1 },
                Page { start: 1, end: 2 },
                Page { start: 2, end: 3 },
            ]
        );
        // Budget 2: the page can hold two rows, and the break before atom 2 is
        // allowed, so it wins over the Never break before atom 1.
        assert_eq!(paginate(&atoms, 2), vec![Page { start: 0, end: 2 }, Page { start: 2, end: 3 }]);
    }
    #[test]
    fn latest_section_break_wins() {
        let atoms = vec![
            atom(&["a"], 0, BreakRank::Inside),
            atom(&["b"], 0, BreakRank::Section),
            atom(&["c"], 0, BreakRank::Section),
        ];
        // Two rows fit; both breaks leave one row empty, so the latest wins.
        assert_eq!(
            paginate(&atoms, 2),
            vec![Page { start: 0, end: 2 }, Page { start: 2, end: 3 }]
        );
    }

    #[test]
    fn gaps_are_dropped_at_a_page_start() {
        let atoms = vec![
            atom(&["a"], 0, BreakRank::Inside),
            PageAtom { lines: vec!["b".into()], gap: 9, break_rank: BreakRank::Section, continuation: None },
        ];
        assert_eq!(
            paginate(&atoms, 1),
            vec![Page { start: 0, end: 1 }, Page { start: 1, end: 2 }]
        );
        assert_eq!(page_height(&atoms, 1, 2), 1);
    }
}
