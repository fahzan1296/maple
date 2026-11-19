// Ported from l/test/transcript-outline-row-cache.test.ts.
// Rust OutlineRowCache keys stripped rows by per-child rendered-row equality and takes
// `&mut [Box<dyn Component>]`/width; behavior should match TS contract.

use maple_tui::chat::transcript_outline::{strip_prompt_zones, OutlineRowCache};
use maple_tui::component::Component;

struct FixedChild {
    rows: Vec<String>,
}

impl FixedChild {
    fn new(text: &str) -> Self {
        Self { rows: vec![text.to_string()] }
    }
}

impl Component for FixedChild {
    fn render(&mut self, _width: u16) -> Vec<String> {
        self.rows.clone()
    }
}

#[test]
fn reuses_stripped_rows_while_child_returns_the_same_rows() {
    let mut cache = OutlineRowCache::new();
    let mut children: Vec<Box<dyn Component>> =
        vec![Box::new(FixedChild::new("a")), Box::new(FixedChild::new("b"))];
    let first = cache.rows(&mut children, 40);
    let second = cache.rows(&mut children, 40);
    assert_eq!(first, vec![vec!["a".to_string()], vec!["b".to_string()]]);
    assert_eq!(second[0], first[0]);
    assert_eq!(second[1], first[1]);
}

#[test]
fn serves_new_rows_after_a_child_repaints_itself() {
    let mut cache = OutlineRowCache::new();
    let mut children: Vec<Box<dyn Component>> = vec![Box::new(FixedChild::new("converting image…"))];
    cache.rows(&mut children, 40);
    // Replace child with one rendering updated content (simulates repaint/new rows).
    children[0] = Box::new(FixedChild::new("<image>"));
    assert_eq!(cache.rows(&mut children, 40), vec![vec!["<image>".to_string()]]);
}

#[test]
fn strips_osc_133_prompt_zones_which_would_garble_overlay_frame() {
    let stripped = strip_prompt_zones(&["\x1b]133;A\x07 prompt \x1b]133;B\x07".to_string()]);
    assert_eq!(stripped, vec![" prompt ".to_string()]);
}
