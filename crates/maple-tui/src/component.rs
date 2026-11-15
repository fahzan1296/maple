//! The component contract every renderable element implements.

use crate::keys::Key;
use crate::mouse::MouseEvent;

/// A renderable unit. `render(width)` returns display rows; every row must fit
/// within `width` columns or the renderer will reject the frame.
pub trait Component {
    /// Produce this component's rows for the given viewport width.
    fn render(&mut self, width: u16) -> Vec<String>;

    /// Handle a decoded key event while this component has focus.
    fn handle_input(&mut self, _key: &Key) {}

    /// Handle a decoded mouse event while this component has focus.
    fn handle_mouse(&mut self, _event: &MouseEvent) {}

    /// Handle a bracketed paste payload.
    fn handle_paste(&mut self, _text: &str) {}

    /// Drop derived caches; the next `render` rebuilds from state.
    fn invalidate(&mut self) {}

    /// Release owned resources. Called when the component leaves the tree.
    fn dispose(&mut self) {}
}
