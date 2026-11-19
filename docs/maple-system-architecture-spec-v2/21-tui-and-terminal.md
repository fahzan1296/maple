# 21 — TUI Presentation Boundary and Terminal Runtime

## 1. Preservation boundary

The existing TUI is the only implementation asset intended for extraction/reuse. Its rendering and input contract is preserved where sound. It is not the owner of session state, provider logic, persistence, tools, or resource governance.

## 2. View-model architecture

Application state is projected into immutable view models:
- session header;
- transcript viewport;
- composer state;
- tool execution cards;
- approval prompt;
- peer/crew status;
- diagnostics panel;
- status/footer.

TUI sends semantic user intents to application commands. It does not mutate domain aggregates.

## 3. Component contract

A component:
- renders deterministically from supplied state and dimensions;
- handles semantic input events;
- reports invalidation;
- disposes owned ephemeral resources;
- does not perform hidden I/O during render;
- clips or wraps content within declared bounds;
- never emits raw untrusted terminal control sequences.

## 4. Render pipeline

1. receive invalidation;
2. snapshot view model;
3. compute layout;
4. render component tree to buffer;
5. diff against previous frame;
6. emit minimal terminal updates;
7. update frame metrics.

Rendering must not block on provider/network/filesystem operations.

## 5. Input pipeline

Raw terminal events are normalized into semantic events: key, text, paste, mouse, resize, focus, interrupt. Paste content is size-bounded and sanitized for display. Input routing is deterministic and testable.

## 6. Terminal ownership

Acquire raw mode and alternate screen through a terminal lease. Restore terminal on normal exit, errors, panic hook where safely possible, and cancellation. Do not assume terminal restoration is possible after SIGKILL or power loss.

## 7. Accessibility and usability

Support keyboard navigation, visible focus, readable contrast, predictable shortcuts, resize behavior, and non-color-only status cues. Long outputs must be scrollable and bounded.

## 8. Virtual terminal tests

Use a virtual terminal to test:
- exact layout at small/large dimensions;
- clipping/wrapping;
- resize;
- key routing;
- mouse events;
- paste limits;
- terminal restoration;
- escape-sequence sanitization.

## 9. Performance

Target responsive input and render under normal load. Coalesce render invalidations. Avoid full transcript re-layout for every token delta; use incremental updates and bounded viewport state.
