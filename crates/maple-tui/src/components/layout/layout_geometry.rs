//! Layout child geometry: insets, allocation, and the child content union.
//!
//! Reference: `l/src/components/layout/geometry.ts`.
//!
//! This file reproduces that TypeScript module and is distinct from the
//! pre-existing [`super::geometry`], which holds the unrelated cell-coordinate
//! primitives `Point`/`Rect`/`distribute_width`.
//!
//! Deviations forced by Rust:
//!
//! - `LayoutContent` is a TS union (`Component | LayoutRenderer`). Rust has no
//!   trait upcasting from `dyn Component`, so a component child is a
//!   [`HeightConstrainedComponent`] trait object; every [`Component`] gets the
//!   (default no-op) `set_height` hook through a blanket implementation.
//! - The reference narrows a component with `"setHeight" in content` and
//!   `"routeMouse" in content`. Those duck-typed probes have no Rust
//!   counterpart, so [`is_height_constrained_component`] is true for every
//!   component child and [`is_layout_mouse_routable`] is true for every crate
//!   component (mouse arrives through [`Component::handle_mouse`]).
//! - `nativeLayoutChild` memoizes its renderer adapter on the function object.
//!   A `Box<dyn Fn>` cannot carry state, so each call returns a fresh adapter;
//!   callers that need a stable identity keep the returned box.
//! - `TspProps<"col">` / `TspSpace` are protocol strings; they travel as the
//!   crate's [`Props`] JSON object and `&'static str` tokens.

use crate::component::Component;
use crate::native::node::Props;
use crate::text_shape::{truncate_to_width, visible_width, Ellipsis};
use serde_json::{Map, Value};

/// `Number.MAX_SAFE_INTEGER`, the reference's unbounded allocation maximum.
const MAX_SAFE_INTEGER: usize = 9_007_199_254_740_991;

/// Alignment within space allocated by a layout component.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutAlignment {
    /// Flush to the start edge.
    #[default]
    Start,
    /// Centered in the spare cells.
    Center,
    /// Flush to the end edge.
    End,
}

/// Insets reserved inside a layout child's allocated rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutInsets {
    /// Rows above the content.
    pub top: Option<f64>,
    /// Columns right of the content.
    pub right: Option<f64>,
    /// Rows below the content.
    pub bottom: Option<f64>,
    /// Columns left of the content.
    pub left: Option<f64>,
}

/// Insets normalized to finite, non-negative terminal-cell counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NormalizedLayoutInsets {
    /// Rows above the content.
    pub top: usize,
    /// Columns right of the content.
    pub right: usize,
    /// Rows below the content.
    pub bottom: usize,
    /// Columns left of the content.
    pub left: usize,
}

/// Rectangle in coordinates local to a layout component.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutRect {
    /// First row.
    pub row: usize,
    /// First column.
    pub col: usize,
    /// Column count.
    pub width: usize,
    /// Row count.
    pub height: usize,
}

/// Lazy layout child, evaluated only when the child is visible.
pub type LayoutRenderer = Box<dyn Fn(usize, Option<usize>) -> Vec<String>>;

/// A component whose parent may assign a local row budget before rendering.
///
/// Deviation: the reference requires `setHeight` on the narrowed type. Rust's
/// child slot needs a default so every [`Component`] can be a layout child;
/// override it to receive the budget.
pub trait HeightConstrainedComponent: Component {
    /// Accept the local row budget.
    fn set_height(&mut self, _height: Option<usize>) {}
}

impl<T: Component + 'static> HeightConstrainedComponent for T {}

/// A concrete component or a lazy renderer used as a layout child.
pub enum LayoutContent {
    /// A concrete component.
    Component(Box<dyn HeightConstrainedComponent>),
    /// A lazy row renderer.
    Renderer(LayoutRenderer),
}

impl LayoutContent {
    /// Wrap a concrete component as layout content.
    pub fn component<C: Component + 'static>(child: C) -> Self {
        LayoutContent::Component(Box::new(child))
    }

    /// Wrap a lazy row renderer as layout content.
    pub fn renderer(render: impl Fn(usize, Option<usize>) -> Vec<String> + 'static) -> Self {
        LayoutContent::Renderer(Box::new(render))
    }
}

/// Static text or a late-bound layout decoration, useful for theme-colored
/// rules.
pub enum LayoutDecoration {
    /// Static text.
    Text(String),
    /// Late-bound text.
    LateBound(Box<dyn Fn() -> String>),
}

impl LayoutDecoration {
    /// Static decoration text.
    pub fn text(value: impl Into<String>) -> Self {
        LayoutDecoration::Text(value.into())
    }

    /// Late-bound decoration text.
    pub fn late_bound(value: impl Fn() -> String + 'static) -> Self {
        LayoutDecoration::LateBound(Box::new(value))
    }
}

/// One fixed or growing constraint consumed by [`allocate_layout_space`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutAllocation {
    /// Exact cell count; the child is not flexible when set.
    pub fixed: Option<f64>,
    /// Growth weight among flexible children.
    pub grow: Option<f64>,
    /// Minimum cell count.
    pub min: Option<f64>,
    /// Maximum cell count.
    pub max: Option<f64>,
}

/// Describe a layout child for the native engine.
///
/// Components describe themselves; a lazy row renderer becomes a stable
/// adapter component the native engine renders as a `rows` fallback (it can
/// only produce width-bound rows).
pub fn native_layout_child(content: LayoutContent) -> Box<dyn HeightConstrainedComponent> {
    match content {
        LayoutContent::Component(child) => child,
        LayoutContent::Renderer(render) => Box::new(RendererAdapter { render }),
    }
}

/// Adapter presenting a [`LayoutRenderer`] as a component.
struct RendererAdapter {
    render: LayoutRenderer,
}

impl Component for RendererAdapter {
    fn render(&mut self, width: u16) -> Vec<String> {
        (self.render)(width as usize, None)
    }
}

/// Spacing step standing in for a textual gap decoration (the terminal draws no
/// divider glyphs).
pub fn native_layout_gap(decoration: Option<&LayoutDecoration>) -> Option<&'static str> {
    let length = layout_decoration_text(decoration).encode_utf16().count();
    match length {
        0 => None,
        1 => Some("xs"),
        2 => Some("sm"),
        _ => Some("md"),
    }
}

/// Flex props for one layout slot along `axis` (`w` for rows, `h` for stacks).
pub fn native_slot_props(axis: &str, slot: &LayoutAllocation) -> Props {
    let mut props = Props::new();
    if let Some(fixed) = optional_layout_size(slot.fixed) {
        props.set_i64("grow", 0);
        props.set_i64("shrink", 0);
        insert_bound(&mut props, "min", axis, fixed);
        insert_bound(&mut props, "max", axis, fixed);
        return props;
    }
    if let Some(grow) = optional_layout_size(slot.grow) {
        props.set_i64("grow", grow as i64);
    }
    props.set_str("basis", "content");
    if let Some(min) = optional_layout_size(slot.min).filter(|min| *min != 0) {
        insert_bound(&mut props, "min", axis, min);
    }
    if let Some(max) = optional_layout_size(slot.max) {
        insert_bound(&mut props, "max", axis, max);
    }
    props
}

/// Set `key` to the axis-bound cell dimension (`{w:{cells}ch}` / `{h:{cells}lines}`).
fn insert_bound(props: &mut Props, key: &str, axis: &str, cells: usize) {
    let (axis_key, value) = if axis == "w" {
        ("w", format!("{cells}ch"))
    } else {
        ("h", format!("{cells}lines"))
    };
    let mut object = Map::new();
    object.insert(axis_key.to_string(), Value::String(value));
    props.insert(key, Value::Object(object));
}

/// Clamp an external size to a finite, non-negative integer cell count.
pub fn layout_size(value: Option<f64>, fallback: f64) -> usize {
    let resolved = match value {
        Some(value) if value.is_finite() => value,
        _ => fallback,
    };
    if resolved.is_finite() {
        resolved.max(0.0).floor() as usize
    } else {
        0
    }
}

/// Normalize an optional external size while preserving `None`.
pub fn optional_layout_size(value: Option<f64>) -> Option<usize> {
    value.map(|value| layout_size(Some(value), 0.0))
}

/// Clamp a proportional layout input to a finite, non-negative number.
pub fn layout_ratio(value: Option<f64>, fallback: f64) -> f64 {
    let resolved = match value {
        Some(value) if value.is_finite() => value,
        _ => fallback,
    };
    if resolved.is_finite() {
        resolved.max(0.0)
    } else {
        0.0
    }
}

/// Resolve static or late-bound layout decoration text.
pub fn layout_decoration_text(decoration: Option<&LayoutDecoration>) -> String {
    match decoration {
        Some(LayoutDecoration::Text(text)) => text.clone(),
        Some(LayoutDecoration::LateBound(resolve)) => resolve(),
        None => String::new(),
    }
}

/// The cell count a flexible child can still grow into.
fn allocation_maximum(item: &LayoutAllocation) -> usize {
    match item.max {
        Some(max) => layout_size(item.min, 0.0).max(layout_size(Some(max), 0.0)),
        None => MAX_SAFE_INTEGER,
    }
}

/// Allocate bounded integer cells across fixed and growing children.
///
/// When the minimums cannot fit, later children collapse first so output stays
/// bounded.
pub fn allocate_layout_space(items: &[LayoutAllocation], available: f64) -> Vec<usize> {
    let available = layout_size(Some(available), 0.0);
    let mut sizes: Vec<usize> = items
        .iter()
        .map(|item| {
            let minimum = layout_size(item.min, 0.0);
            match item.fixed {
                None => minimum,
                Some(fixed) => minimum.max(allocation_maximum(item).min(layout_size(Some(fixed), 0.0))),
            }
        })
        .collect();
    let used: usize = sizes.iter().sum();
    if used > available {
        let mut overflow = used - available;
        for size in sizes.iter_mut().rev() {
            if overflow == 0 {
                break;
            }
            let reduction = (*size).min(overflow);
            *size -= reduction;
            overflow -= reduction;
        }
        return sizes;
    }
    let mut remaining = available - used;
    let mut flexible: Vec<(usize, &LayoutAllocation)> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.fixed.is_none() && layout_size(item.grow, 0.0) > 0)
        .map(|(index, item)| (index, item))
        .collect();
    while remaining > 0 && !flexible.is_empty() {
        let total_weight: usize = flexible
            .iter()
            .map(|(_, item)| layout_size(item.grow, 0.0))
            .sum();
        let mut granted = 0usize;
        for (index, item) in &flexible {
            let capacity = allocation_maximum(item) - sizes[*index];
            if capacity == 0 {
                continue;
            }
            let proportional = (remaining * layout_size(item.grow, 0.0)) / total_weight;
            let share = capacity.min(proportional.max(1)).min(remaining - granted);
            if share == 0 {
                continue;
            }
            sizes[*index] += share;
            granted += share;
            if granted == remaining {
                break;
            }
        }
        if granted == 0 {
            break;
        }
        remaining -= granted;
        flexible.retain(|(index, item)| sizes[*index] < allocation_maximum(item));
    }
    sizes
}

/// Return concrete children once each, preserving their first layout order.
///
/// Deviation: the reference stores component object identities in a `Set`, so
/// the same object listed twice collapses. An owning [`LayoutContent`] cannot
/// repeat a component (each is a distinct `Box`), so the identity check is
/// kept for API parity but never collapses here; shared children would need
/// explicit `Rc` handling in the caller.
pub fn unique_layout_components(
    contents: &[LayoutContent],
) -> Vec<&dyn HeightConstrainedComponent> {
    let mut unique: Vec<&dyn HeightConstrainedComponent> = Vec::new();
    for content in contents {
        if let LayoutContent::Component(child) = content {
            if !unique.iter().any(|seen| std::ptr::eq(*seen, child.as_ref())) {
                unique.push(child.as_ref());
            }
        }
    }
    unique
}

/// Normalize every inset to a finite, non-negative integer cell count.
pub fn layout_insets(insets: Option<&LayoutInsets>) -> NormalizedLayoutInsets {
    NormalizedLayoutInsets {
        top: layout_size(insets.map(|insets| insets.top).flatten(), 0.0),
        right: layout_size(insets.map(|insets| insets.right).flatten(), 0.0),
        bottom: layout_size(insets.map(|insets| insets.bottom).flatten(), 0.0),
        left: layout_size(insets.map(|insets| insets.left).flatten(), 0.0),
    }
}

/// Compare optional inset objects by their normalized terminal geometry.
pub fn equal_layout_insets(left: Option<&LayoutInsets>, right: Option<&LayoutInsets>) -> bool {
    layout_insets(left) == layout_insets(right)
}

/// Offset content within spare cells according to an alignment.
pub fn layout_alignment_offset(space: usize, alignment: LayoutAlignment) -> usize {
    match alignment {
        LayoutAlignment::Start => 0,
        LayoutAlignment::End => space,
        LayoutAlignment::Center => space / 2,
    }
}

/// Narrow a concrete-or-lazy child to a concrete component.
pub fn is_layout_component(content: &LayoutContent) -> bool {
    matches!(content, LayoutContent::Component(_))
}

/// Narrow a layout child to a component accepting a row budget.
///
/// Deviation: every component child carries the budget hook, so this is true
/// for every component child.
pub fn is_height_constrained_component(content: &LayoutContent) -> bool {
    is_layout_component(content)
}

/// Narrow a layout child to a mouse-routable component.
///
/// Deviation: every crate component accepts mouse through
/// [`Component::handle_mouse`], so this is true for every component child.
pub fn is_layout_mouse_routable(content: &LayoutContent) -> bool {
    is_layout_component(content)
}

/// Return concrete children once each, preserving their first layout order.

/// Fit one ANSI-styled row to an exact terminal-cell width.
pub fn fit_layout_line(line: &str, width: f64) -> String {
    let width = layout_size(Some(width), 0.0);
    if width == 0 {
        return String::new();
    }
    let line_width = visible_width(line);
    let (clipped, clipped_width) = if line_width > width {
        let clipped = truncate_to_width(line, width, Ellipsis::Unicode);
        let clipped_width = visible_width(&clipped);
        (clipped, clipped_width)
    } else {
        (line.to_string(), line_width)
    };
    format!("{}{}", clipped, " ".repeat(width - clipped_width))
}

/// Render a concrete or lazy child with an optional local height budget.
pub fn render_layout_content(
    content: &mut LayoutContent,
    width: f64,
    height: Option<f64>,
) -> Vec<String> {
    match content {
        LayoutContent::Renderer(render) => {
            render(layout_size(Some(width), 0.0), optional_layout_size(height))
        }
        LayoutContent::Component(child) => {
            child.set_height(optional_layout_size(height));
            child.render(layout_size(Some(width), 0.0).min(u16::MAX as usize) as u16)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Text;

    fn allocation(fixed: Option<f64>, grow: Option<f64>, min: Option<f64>, max: Option<f64>) -> LayoutAllocation {
        LayoutAllocation { fixed, grow, min, max }
    }

    #[test]
    fn layout_size_clamps_to_cells() {
        let cases = [
            (None, 0.0, 0),
            (Some(3.9), 0.0, 3),
            (Some(-2.0), 0.0, 0),
            (Some(f64::NAN), 0.0, 0),
            (Some(f64::INFINITY), 0.0, 0),
            (Some(f64::NEG_INFINITY), 5.0, 5),
            (None, 7.0, 7),
            (Some(2.0), 9.0, 2),
        ];
        for (value, fallback, expected) in cases {
            assert_eq!(layout_size(value, fallback), expected, "{value:?}");
        }
    }

    #[test]
    fn optional_layout_size_preserves_none() {
        assert_eq!(optional_layout_size(None), None);
        assert_eq!(optional_layout_size(Some(4.2)), Some(4));
        assert_eq!(optional_layout_size(Some(-1.0)), Some(0));
    }

    #[test]
    fn layout_ratio_clamps() {
        assert_eq!(layout_ratio(None, 0.5), 0.5);
        assert_eq!(layout_ratio(Some(-1.0), 0.5), 0.0);
        assert_eq!(layout_ratio(Some(f64::NAN), 0.5), 0.0);
        assert_eq!(layout_ratio(Some(0.25), 0.5), 0.25);
    }

    #[test]
    fn allocates_fixed_minimum_and_growth() {
        let cases: &[(&[LayoutAllocation], f64, Vec<usize>)] = &[
            (&[], 10.0, vec![]),
            (&[allocation(None, None, Some(2.0), None)], 10.0, vec![2]),
            (
                &[
                    allocation(Some(3.0), None, None, None),
                    allocation(None, Some(1.0), Some(1.0), None),
                ],
                10.0,
                vec![3, 7],
            ),
            (
                &[
                    allocation(None, Some(1.0), Some(1.0), None),
                    allocation(None, Some(1.0), Some(1.0), None),
                ],
                9.0,
                vec![5, 4],
            ),
            (
                &[
                    allocation(None, Some(1.0), Some(2.0), Some(3.0)),
                    allocation(None, Some(1.0), Some(2.0), None),
                ],
                10.0,
                vec![3, 7],
            ),
            (
                &[
                    allocation(Some(2.0), None, None, None),
                    allocation(None, Some(1.0), Some(1.0), None),
                    allocation(None, Some(1.0), Some(1.0), None),
                ],
                4.0,
                vec![2, 1, 1],
            ),
        ];
        for (items, available, expected) in cases {
            assert_eq!(allocate_layout_space(items, *available), *expected);
        }
    }

    #[test]
    fn allocation_collapses_later_children_first() {
        let items = [
            allocation(Some(4.0), None, None, None),
            allocation(Some(4.0), None, None, None),
            allocation(Some(4.0), None, None, None),
        ];
        assert_eq!(allocate_layout_space(&items, 6.0), vec![4, 2, 0]);
        assert_eq!(allocate_layout_space(&items, 0.0), vec![0, 0, 0]);
    }

    #[test]
    fn allocation_never_exceeds_maximum() {
        let items = [allocation(None, Some(1.0), Some(1.0), Some(2.0))];
        assert_eq!(allocate_layout_space(&items, 100.0), vec![2]);
    }

    #[test]
    fn normalizes_and_compares_insets() {
        let insets = LayoutInsets {
            top: Some(1.9),
            right: Some(-3.0),
            bottom: None,
            left: Some(2.0),
        };
        assert_eq!(
            layout_insets(Some(&insets)),
            NormalizedLayoutInsets {
                top: 1,
                right: 0,
                bottom: 0,
                left: 2
            }
        );
        assert_eq!(layout_insets(None), NormalizedLayoutInsets::default());
        let shifted = LayoutInsets {
            top: Some(1.0),
            right: Some(0.0),
            bottom: Some(0.0),
            left: Some(2.0),
        };
        assert!(equal_layout_insets(Some(&insets), Some(&shifted)));
        assert!(equal_layout_insets(None, None));
        assert!(!equal_layout_insets(None, Some(&shifted)));
    }

    #[test]
    fn alignment_offsets() {
        assert_eq!(layout_alignment_offset(0, LayoutAlignment::End), 0);
        assert_eq!(layout_alignment_offset(5, LayoutAlignment::Start), 0);
        assert_eq!(layout_alignment_offset(5, LayoutAlignment::End), 5);
        assert_eq!(layout_alignment_offset(5, LayoutAlignment::Center), 2);
    }

    #[test]
    fn resolves_decoration_text() {
        assert_eq!(layout_decoration_text(None), "");
        assert_eq!(
            layout_decoration_text(Some(&LayoutDecoration::text("──"))),
            "──"
        );
        assert_eq!(
            layout_decoration_text(Some(&LayoutDecoration::late_bound(|| "─".to_string()))),
            "─"
        );
    }

    #[test]
    fn maps_gap_decoration_to_space_steps() {
        assert_eq!(native_layout_gap(None), None);
        assert_eq!(
            native_layout_gap(Some(&LayoutDecoration::text(""))),
            None
        );
        assert_eq!(
            native_layout_gap(Some(&LayoutDecoration::text(" "))),
            Some("xs")
        );
        assert_eq!(
            native_layout_gap(Some(&LayoutDecoration::text("  "))),
            Some("sm")
        );
        assert_eq!(
            native_layout_gap(Some(&LayoutDecoration::text("   "))),
            Some("md")
        );
    }

    #[test]
    fn builds_fixed_slot_props() {
        let props = native_slot_props("w", &allocation(Some(6.0), None, None, None));
        assert_eq!(props.get_i64("grow"), Some(0));
        assert_eq!(props.get_i64("shrink"), Some(0));
        assert_eq!(props.get_str("basis"), None);
        let min = props.get("min").and_then(Value::get).and_then(Value::as_str);
        assert_eq!(min, Some("6ch"));
        let max = props.get("max").and_then(Value::get).and_then(Value::as_str);
        assert_eq!(max, Some("6ch"));

        let props = native_slot_props("h", &allocation(Some(2.0), None, None, None));
        let min = props.get("min").and_then(Value::get).and_then(Value::as_str);
        assert_eq!(min, Some("2lines"));
    }

    #[test]
    fn builds_flexible_slot_props() {
        let props = native_slot_props(
            "w",
            &allocation(None, Some(2.0), Some(0.0), Some(9.0)),
        );
        assert_eq!(props.get_i64("grow"), Some(2));
        assert_eq!(props.get_str("basis"), Some("content"));
        assert_eq!(props.contains_key("min"), false);
        let max = props.get("max").and_then(Value::get).and_then(Value::as_str);
        assert_eq!(max, Some("9ch"));

        let props = native_slot_props("h", &allocation(None, None, Some(3.0), None));
        assert_eq!(props.contains_key("grow"), false);
        let min = props.get("min").and_then(Value::get).and_then(Value::as_str);
        assert_eq!(min, Some("3lines"));
        assert_eq!(props.contains_key("max"), false);
    }

    #[test]
    fn fits_lines_to_exact_width() {
        assert_eq!(fit_layout_line("", 0.0), "");
        assert_eq!(fit_layout_line("abc", 5.0), "abc  ");
        assert_eq!(fit_layout_line("abcdef", 3.0), "ab…");
        assert_eq!(fit_layout_line("abc", 3.0), "abc");
        assert_eq!(fit_layout_line("abcdef", -1.0), "");
    }

    #[test]
    fn renders_lazy_and_concrete_children() {
        let mut lazy = LayoutContent::renderer(|width, height| {
            vec![format!("{width}:{height:?}")]
        });
        assert_eq!(render_layout_content(&mut lazy, 7.0, None), vec!["7:None"]);
        assert_eq!(
            render_layout_content(&mut lazy, 7.0, Some(4.0)),
            vec!["7:Some(4)"]
        );

        let mut concrete = LayoutContent::component(Text::new("hi"));
        assert_eq!(
            render_layout_content(&mut concrete, 8.0, Some(2.0)),
            vec!["hi      "]
        );
    }

    #[test]
    fn native_adapter_renders_width_bound_rows() {
        let child = native_layout_child(LayoutContent::renderer(|width, height| {
            vec![format!("{width}:{height:?}")]
        }));
        let mut child = child;
        assert_eq!(child.render(5), vec!["5:None"]);
    }

    #[test]
    fn narrows_children() {
        let contents = vec![
            LayoutContent::component(Text::new("a")),
            LayoutContent::renderer(|_, _| vec![]),
        ];
        assert!(is_layout_component(&contents[0]));
        assert!(!is_layout_component(&contents[1]));
        assert!(is_height_constrained_component(&contents[0]));
        assert!(is_layout_mouse_routable(&contents[0]));
        assert!(!is_layout_mouse_routable(&contents[1]));

        let unique = unique_layout_components(&contents);
        assert_eq!(unique.len(), 1);
    }

    #[test]
    fn unique_components_preserve_layout_order() {
        let contents = vec![
            LayoutContent::component(Text::new("a")),
            LayoutContent::renderer(|_, _| vec![]),
            LayoutContent::component(Text::new("b")),
        ];
        let unique = unique_layout_components(&contents);
        assert_eq!(unique.len(), 2);
        assert_eq!(unique[0].render(1), vec!["a"]);
        assert_eq!(unique[1].render(1), vec!["b"]);
    }
}
