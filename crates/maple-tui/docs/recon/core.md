# Behavioral Spec — `maple-tui` v18.20.2 core (`l/src`)

Scope: the 17 requested modules. All contracts below are behavior/format only (no TS copied). Constants and byte sequences verified against source are given literally; anything reconstructed rather than re-read is marked `[INFERENCE]`. Line anchors are `file:line`.

---

## 1. `tui.ts` — the `TUI` class

### 1.1 Verified byte/escape constants (`tui.ts:67-115`)

| Name | Value | Meaning |
|---|---|---|
| `SEGMENT_RESET` | `"\x1b[0m"` | full-attribute reset ending a content row |
| `LINE_TERMINATOR` | `"\x1b[0m\x1b]8;;\x07"` | row terminator closing SGR **and** any OSC 8 hyperlink |
| `ERASE_LINE` | `"\x1b[2K"` | |
| `ERASE_TO_END_OF_LINE` | `"\x1b[K"` | |
| `HIDE_CURSOR` | `"\x1b[?25l"` | |
| `SYNC_OUTPUT_BEGIN` | `"\x1b[?2026h"` | DEC 2026 begin |
| `SYNC_OUTPUT_END` | `"\x1b[?2026l"` | DEC 2026 end |
| `DISABLE_AUTOWRAP` | `"\x1b[?7l"` | pending-wrap guard |
| `ENABLE_AUTOWRAP` | `"\x1b[?7h"` | |
| `PAINT_BEGIN` | `HIDE_CURSOR + SYNC_OUTPUT_BEGIN + DISABLE_AUTOWRAP` | `\x1b[?25l\x1b[?2026h\x1b[?7l` |
| `PAINT_END` | `ENABLE_AUTOWRAP + SYNC_OUTPUT_END` | `\x1b[?7h\x1b[?2026l` |
| `PAINT_BEGIN_NO_SYNC` | `HIDE_CURSOR + DISABLE_AUTOWRAP` | |
| `PAINT_END_NO_SYNC` | `ENABLE_AUTOWRAP` | |
| `SYNC_OUTPUT_RENEW_BYTES` | `16 * 1024` | renew DEC 2026 every 16 KiB during large replays (tmux expires it after 1 s) |
| `MOUSE_TRACKING_ON` | `"\x1b[?1000h\x1b[?1003h\x1b[?1006h"` | click + any-motion + SGR coords |
| `MOUSE_TRACKING_OFF` | `"\x1b[?1006l\x1b[?1003l\x1b[?1000l"` | |
| `CELL_SIZE_REPLY` | `/^\x1b\[6;(\d+);(\d+)t$/` | CSI 16 t reply |
| `ITERM2_CELL_SIZE_REPLY` | `/^\x1b\]1337;ReportCellSize=(\d+(?:\.\d+)?);(\d+(?:\.\d+)?)(?:;(\d+(?:\.\d+)?))?(?:\x07|\x1b\\)$/` | |
| `CURSOR_MARKER` | `"\x1b_pi:c\x07"` | APC cursor sentinel components emit; TUI strips it |
| `BLANK_ROW` | `""` | diff sentinel for a blank row |
| `UNKNOWN_ROW` | sentinel distinct from every row key (above the pre-viewport band) | `[INFERENCE]`: a unique symbol/number used in the screen model |
| `DEFAULT_MAX_INLINE_IMAGES` | `8` (`components/image.ts:109`) | image-budget cap |

Static timing constants (`tui.ts:977-1146`): `#INPUT_FRAME_INTERVAL_MS = 1000/30`, `#INPUT_RENDER_GRACE_MS = 1000/30`, `#MAX_ADAPTIVE_RENDER_MS = 200`, `#MAX_PENDING_OUTPUT_BYTES = STDOUT_BACKLOG_CLEAR_BYTES` (= 256 KiB, `terminal.ts:272`), `#OUTPUT_BACKLOG_RETRY_MS = 10`, `#RESIZE_VIEWPORT_SETTLE_MS = 120`, `#RESIZE_PROBE_TIMEOUT_MS = 200`, `#RESIZE_REBUILD_NOTICE_MIN_BYTES = 7*1024*1024`, `#MAX_ROW_MOVES = 3`, `#GHOSTTY_INITIAL_IMAGE_DELAY_MS = 100`, `#OSC66_MAX_SPACER_ROWS = 6`, `#NATIVE_PROBE_HOLD_MS = 300`, `#NATIVE_CONFIRM_MS = 1000`, `#NATIVE_CONFIRM_GRACE_MS = 100`.

Env flags read: `PI_HARDWARE_CURSOR` (default hardware-cursor preference), `PI_TUI_MAX_FPS` (clamped 1..1000 in `tui.ts`, default from `minFrameIntervalMs()`), `PI_TUI_RESIZE_IN_PLACE` (`1/true`, `0/false`), `PI_TUI_RESIZE_SCROLLBACK` (`append|rebuild|preserve`, default `preserve`), `PI_DEBUG_REDRAW` (append resize-anchor lines to debug log). maple-tui reads the `MAPLE_`-prefixed names through `maple_tui::env_lookup`, which still honours these legacy `PI_*` names as a fallback.

### 1.2 Public types (`tui.ts:207-539`)

```ts
interface Component {
  debugId?: string; debugKind?: string;
  debugState?(): Record<string, unknown>;
  debugChildren?: readonly Component[];
  render(width: number): readonly string[];      // component-owned; reference equality = byte-identical
  describe?(cx: DescribeContext): NativeNode | null;
  nativeOverlay?: { role?: string; head?: TspText; size?: "sm"|"md"|"lg"|"full"; anchor?: "center"|"top"|"bottom" };
  nativeSheet?(cx: DescribeContext): boolean;
  describeScreen?(cx: DescribeContext): NativeScreen;
  handleNativeEvent?(event: NativeUiEvent): void;
  nativeTrailers?(): readonly Component[];
  capturesInput?(data: string): boolean;
  handleInput?(data: string): void;
  wantsKeyRelease?: boolean;
  invalidate?(): void;
  releaseRenderCaches?(): void;
  setIgnoreTight?(ignore: boolean): any;
  dispose?(): void;
}
interface Focusable { focused: boolean; setUseTerminalCursor?(use: boolean): void }
type OverlayAnchor = "center"|"top-left"|"top-right"|"bottom-left"|"bottom-right"|"top-center"|"bottom-center"|"left-center"|"right-center";
type SizeValue = number | `${number}%`;
interface OverlayOptions {
  width?: SizeValue; minWidth?: number; maxHeight?: SizeValue;
  anchor?: OverlayAnchor; offsetX?: number; offsetY?: number;
  row?: SizeValue; col?: SizeValue; margin?: OverlayMargin | number;
  visible?: (termWidth: number, termHeight: number) => boolean;
  fullscreen?: boolean;      // borrow alternate screen
  mouseTracking?: boolean;   // default true while fullscreen
}
interface OverlayHandle { hide(): void; setHidden(hidden: boolean): void; isHidden(): boolean }
interface TUIStartOptions { clearScrollback?: boolean; deferInput?: boolean }
type ResizeScrollbackMode = "append" | "rebuild" | "preserve";
interface RenderRequestOptions { clearScrollback?: boolean }
interface TUIOptions { renderScheduler?: RenderScheduler; onPaint?: (paint: TuiPaint) => void }
interface RenderTimer { cancel(): void }
interface RenderScheduler {
  now(): number;
  scheduleImmediate(cb: () => void): void;
  scheduleRender(cb: () => void, delayMs: number): RenderTimer;
}
type InputListener = (data: string) => { consume?: boolean; data?: string } | void;
type PaintListener = (paint: TuiPaint) => void;
type StartListener = () => void;
interface TuiPaint { history: readonly string[]; viewport: readonly string[]; reset: boolean; alt: boolean; columns: number; rows: number }
type HistoryBatchKind = "incremental" | "replay";
type HistoryEntry = string | { rows: readonly string[] } | { component: Component };
interface HistoryBatch { id: number; kind: HistoryBatchKind; entries: readonly HistoryEntry[] }
interface TUIFrame { history?: HistoryBatch; viewport: readonly string[] }
interface TUIFrameProvider {
  renderFrame(req: { columns: number; rows: number }): TUIFrame | null;
  renderResizeFrame?(req: { columns: number; rows: number }): readonly string[];
  beginHistoryReplay?(): void;
  acknowledgeHistory(id: number): void;
  requestRender?(): void;
}
interface StableRowContract { rows?: (width:number)=>readonly string[]; parse?: (width:number)=>readonly string[]; wrap?: (width:number)=>readonly string[] }
```

`Container implements Component`: `children: Component[]`, `render(width)` concatenates child renders (memoized by child reference equality), `addChild`/`removeChild`, `invalidate`, `dispose`.

`class TUI extends Container` public surface: `terminal: Terminal`, `overlayStack: {component, options?, preFocus, hidden, released}[]`, `onDebug?: () => void`, plus `children` inherited.

### 1.3 Constructor and lifecycle

`constructor(terminal: Terminal, showHardwareCursor?: boolean, options?: TUIOptions)` (`tui.ts:1154`):
- `renderScheduler = options.renderScheduler ?? DEFAULT_RENDER_SCHEDULER` (`now=performance.now`, `scheduleImmediate=setImmediate`, `scheduleRender=setTimeout`); `onPaint` added to `#paintListeners`; `showHardwareCursor` defaults to `$flag("PI_HARDWARE_CURSOR")`; `#watchdog = new LoopWatchdog()`; `#resizeScrollbackMode` from `PI_TUI_RESIZE_SCROLLBACK`; `#imageBudget = new ImageBudget(8, () => requestRender())`.

`start(options?: TUIStartOptions)` (`tui.ts:1547`): resets `#stopped`; subscribes appearance/background-change, private-mode-2026 reports, glyph-protocol report, sixel support, TSP hello; sets `#inputDeferred = options.deferInput && !tspExpected`; `#heldInput=[]/#heldFocus` for a TSP restart; starts watchdog; sets Ghostty image delay deadline `now + 100 ms`; registers postmortem `"tui-restore"` → `stop()`; runs start listeners; sets `#started`; `terminal.hideCursor()`; if not deferred, queries SIXEL and cell size; if TSP expected opens the native surface (optimistic, confirmed later) else holds first paint 300 ms; finally `requestRender(true, { clearScrollback: options.clearScrollback === true })`.

`stop()` (`tui.ts:2759`): sets `#stopped`/`#started`; stops watchdog; cancels render/Ghostty timers; **shell handoff**: if `#previousFrameLength > 0`, target row = `min(#providerViewportTop + #previousFrameLength, rows-1)`, move by `CSI nB`/`CSI nA` from `#hardwareCursorRow` (clamped), then write `"\r"` if target ≤ bottom else `"\r\n"`; then `terminal.showCursor(true)` (forced), forget cursor state, `terminal.stop()`.

`enableInput()` (`tui.ts:2438`): only when `#inputDeferred`; calls `terminal.enableInput()`, queries SIXEL/cell size, then continues startup (native/probes/requestRender) exactly as `start()` would have.

`releaseHeldInput()`: replays held TSP keystrokes (`#heldInput`) through the normal input path (held during `deferInput` with TSP expected).

`scheduleAfter(delayMs, cb)`: scheduler timer that no-ops if stopped.

`renderNow(options?)`: synchronous forced render (no setImmediate); same as `requestRender` minus the hop.

`resetDisplay()`: `invalidate()` + `#prepareForcedRender(true)` + `#renderRequested=false` + `#executeRender()` — the only ordinary-path destructive reset.

### 1.4 Render request & scheduling (exact)

- `requestRender(force = false, options?)` (`tui.ts:2819`):
  - `force`: `#prepareForcedRender(options?.clearScrollback === true)` (sets `#clearScrollbackOnNextRender` and `#forceViewportRepaintOnNextRender`, calls `provider.beginHistoryReplay()` when clearing, cancels any render timer and clears `#renderRequested`), then `#renderRequested=true` and `scheduleImmediate` → `#executeRender()` (guarded by `#stopped`/flag).
  - ordinary: if `#inputRenderPending && #renderTimer && !nativeLive`, cancel the timer, keep `#renderRequested`, `#scheduleRender()` (ride the 30 fps input cadence); else `#requestOrdinaryRender()`.
- `#requestOrdinaryRender`: if `#renderRequested` return; set flag; `scheduleImmediate(() => #scheduleRender())`.
- `requestComponentRender(component)`: identical to ordinary render (parameter unused; frames always recompose the bounded viewport).
- `#scheduleRender()` (`tui.ts:2951`): no-op if stopped/timer exists/flag unset. `elapsed = now - #lastRenderAt`; `frameIntervalMs = #inputRenderPending ? min(1000/30, minFrameIntervalMs()) : minFrameIntervalMs()`; `cadenceDelay = max(0, frameIntervalMs - elapsed)`; `adaptiveFloor = min(200, 2*#lastFrameCostMs)`; `adaptiveDelay = max(0, adaptiveFloor - elapsed)`; `inputGraceDelay = max(0, #inputRenderGraceUntilMs - now)`; `delay = nativeLive ? 0 : max(cadenceDelay, adaptiveDelay, inputGraceDelay)`; arm `#runScheduledRender`.
- `#runScheduledRender`: clear timer; no-op if stopped/flag unset; clear flag; `#executeRender()`; if flag got re-set, `#scheduleRender()` again.
- `#executeRender`: if `#deferRenderForOutputBacklog()` return; `#inputRenderPending=false`; `start=now`; `#lastRenderAt=start`; `#doRender()`; `#lastFrameCostMs=now-start`.
- `#deferRenderForOutputBacklog`: if `terminal.pendingOutputBytes <= 256 KiB` (or undefined) return false; re-set `#renderRequested`, arm a 10 ms retry (`??=`), return true. One-shot intents survive.

### 1.5 Render pipeline (buffer, diff, clear, cursor movement, wrapping, blanks)

Buffer model: the renderer keeps `#providerWindow: string[]` (exact normalized rows of the last painted mutable viewport), `#providerPreparedRows: PreparedLine[]`, `#providerViewportTop`, `#providerViewportPadTop`, `#previousFrameLength/Width/Height`, `#preparedLineMemo`/`Spare`. A frame composes the viewport from `render(width)` (children fallback) or `provider.renderFrame(...).viewport`, composites visible overlays, clamps to `height`, runs the image budget, then `#emitPlanFrame`.

- **Line preparation** `#prepareLinesArray(lines, width, previous, length, markers?)` (`tui.ts:4237`): reuses previous prepared rows via a memo keyed by the stripped raw line (only stripping passes index); strips `CURSOR_MARKER` when `markers` given and records positions bottom-most first; classifies each line; fits to width.
- **Classification** `#classifyLine`: image markers (inline image protocol spans) take precedence over ANSI parsing; computes `asciiWidth`, `hasOsc8`, `isImage`; non-image rows are `coalesceAdjacentSgr` (merge adjacent SGR runs) unless `isImage`.
- **Width fitting** `#lineFitSource`: walks code units, skipping ANSI sequences (`#ansiSequenceEnd`), counts visible cells, stops at `safeWidth`; long sources (>4096 code units) use the fast path; limits `LINE_FIT_MAX_SOURCE_CODE_UNITS = 65536`, `LINE_FIT_SOURCE_WIDTH_MULTIPLIER = 64`. OSC 66 (`\x1b]66;META;TEXT\x1b\\`) counts as visible payload; a visible-payload sequence that doesn't fit is truncated at a sequence boundary.
- **Row diff / rewrite**: `#lineRewriteSequence(row, width)` returns `rewrite + "\r"`; when a row is stale/bcc-tainted it prefixes `SEGMENT_RESET + ERASE_TO_END_OF_LINE` (rewrites always start at column 1, so EL-to-end clears the row). Rows are addressed with `CSI row;1H` (1-based).
- **Row moves**: `#movedRowsSequence(width,height,startTop,newTop,history,viewport)` returns a synchronized sequence or `undefined` (falls back to plain rewrites) when: viewport taller than screen, `startTop + window.length > height`, any row is an image, width/widthEpoch/imageProtocol changed since preparation, or any line is OSC 66. It builds a screen model (`UNKNOWN_ROW` above the old viewport top), pushes history in screenfuls with `\n` on the bottom row, applies at most `#MAX_ROW_MOVES = 3` `findRowMove`/`rowMoveSequence` (DL/IL/scroll) band moves, then rewrites remaining differing rows, then erases from the first non-blank row below the content downward with one `#eraseBelowRow`. Rows bound for scrollback are first written as their history line, then pushed with line feeds on the bottom row.
- **Full clear**: only on destructive reset / replay / explicit gesture: `FULL_RESET_SEQUENCE` `[INFERENCE: "\x1b[H\x1b[J\x1b[3J"]` (home + ED2 + ED3); ordinary frames never clear scrollback. `reset: true` is reported to paint listeners when `destructiveReset || history.kind === "replay"`.
- **CSI 2026 wrapping**: every paint/move buffer is bracketed by `#paintBeginSequence`/`#paintEndSequence` (`PAINT_BEGIN/PAINT_END`, or the no-sync variants when DEC 2026 is unsupported/opted out). Enabled by default via `shouldEnableSynchronizedOutputByDefault()`, overridden by an explicit user override, and by confirmed DECRPM 2026 reports (status 0 + a multiplexer that honors 2026 is ignored; confirmed unsupported disables; unconfirmed DA1 sentinel keeps the static default).
- **Autowrap / pending-wrap guard**: `PAINT_BEGIN` includes `CSI ?7l` and `PAINT_END` includes `CSI ?7h`; every content row ends with `"\r"` and rows are separated by `"\r\n"`; exact-width rows therefore cannot trigger a terminal pending-wrap on the following cursor move. Frame buffers that embed cursor sequences are still wrapped.
- **Trailing blanks**: the mutable viewport is padded to the terminal height with `BLANK_ROW` before a provider frame, so a full provider frame emits blank rows for unused bottom rows; in the row-move/diff path no blank rows are written below the content — the first non-blank row below `newTop+rows` triggers a single `#eraseBelowRow(r, height)`. Children-fallback frames render only the composed children and erase leftover rows.
- **Resize handling**: see 1.9.
- **Image budget**: `ImageBudget(cap=8, requestRender)` (`components/image.ts`). `beginPass(normal?, alt?)` / `endPass()` loop; `limitResidentImages()` in `#emitPlanFrame; `hasPendingTransmits()` gates the Ghostty 100 ms initial-image hold (`#maybeDeferGhosttyInitialImagePaint`); images force `undefined` from `#movedRowsSequence` (plain rewrites). Screen/alt placement maps, `#nextImagePlacementId`, `#sixelCrop`.

### 1.6 Overlays (`tui.ts:1346-1545`)

- `showOverlay(component, options?)`: `setIgnoreTight(true)`; push `{component, options, preFocus: #focusedComponent, hidden:false, released:false}`; if `#isOverlayVisible(entry)` → `setFocus(component)`; `terminal.hideCursor()` + `#recordHardwareCursorHidden()`; `requestRender()`; returns handle.
- `handle.hide()`: splice; for overlays above, if their `preFocus` was an owned target of this component, replace it with this entry's `preFocus`; if this overlay (or an owned target) had focus, `setFocus(#getKeyHolderOverlay()?.component ?? entry.preFocus)`; if stack empty, hide cursor; `requestRender()`.
- `handle.setHidden(hidden)`: hiding moves focus to key-holder overlay or `preFocus`; showing restores focus to this overlay if visible; `requestRender()`.
- `hideOverlay()`: pop top; `setFocus(#getKeyHolderOverlay()?.component ?? overlay.preFocus)`; hide cursor if stack empty; `requestRender()`.
- `hasOverlay()`: any `#isOverlayVisible`.
- `#isOverlayVisible(entry)`: `!hidden && (options.visible ? options.visible(columns, rows) : true)`.
- `#getTopmostVisibleOverlay()`: last visible in stack order.
- `#getKeyHolderOverlay()`: last visible entry with `released === false`.
- `#compositeVisibleOverlays(viewport,width,height)`: pads viewport to height, then for each visible overlay resolves layout (first with height 0 to get width/maxHeight, then with overlay height), clamps lines to `maxHeight` (bottom-anchored anchors keep the tail), composites at `row`/`col`.
- `#focusFromPointer(owners, field, sheet)`: a click inside a visible overlay focuses it; a click on `field` under overlays focuses it only when every visible overlay is a `sheet`, marking all `released = true`.
- **Fullscreen overlays** (`options.fullscreen`): while the topmost visible overlay sets it, `#doRender` borrows the alternate screen (`#altActive`), paints only the modal, emits no ED3/scrollback bytes, leaves all normal-screen accounting untouched, and sets mouse tracking `"full"` (unless `options.mouseTracking === false`). `#altActive` frames diff against `#altPreviousLines`; `#altScreenExit()` restores and re-asserts the main keyboard mode. `setAltScreenActive(bool)` mirrors the terminal-level alt state (`terminal.ts:388`).
- Mouse tracking state machine `#setMouseTracking`: `"off" | "inline" | "full"`; only transitions from off emit `MOUSE_TRACKING_ON`, to off emit `MOUSE_TRACKING_OFF`; inline/full share bytes.

### 1.7 Focus dispatch order (`#handleInput`, `tui.ts:2993-3127`)

1. Data starting with `TSP_PREFIX` → native backend (or dropped).
2. CPR (`CSI row;col R`) replies while `#cprColumnTags` non-empty: strip/attribute by tagged column + geometry epoch; untagged row-1 column≥2 is left as a keystroke (modified F3 collision); unknown tagged replies are stripped and discarded.
3. If empty, return. Set `#inputRenderPending = true`.
4. Held-input gate: if `#heldInput !== undefined && #focusedComponent === #heldFocus && !#typesThroughHold(data)` → hold and return.
5. Focused overlay visibility re-check: if the focused component is an overlay that is no longer visible, redirect to topmost visible overlay or its `preFocus`.
6. `ctrl+c`/`escape` set `#inputRenderGraceUntilMs = now + 1000/30`.
7. Capture preflight: `#inputTarget()` with `handleInput`, not a release (or `wantsKeyRelease`), and `capturesInput(data) === true` → `handleInput(data)`, `requestRender()`, return.
8. `#inputListeners` in insertion order; each may `consume` or rewrite `data`; empty → return.
9. `#consumeCellSizeResponse(data)` (CSI 6;h;w t / iTerm2 OSC 1337) → return.
10. `matchesKey(data,"shift+ctrl+d") && onDebug` → `onDebug()`, return.
11. `#inputTarget().handleInput(data)` (releases filtered unless `wantsKeyRelease`), then `requestRender()`.
`#inputTarget()`: focused component if it has `handleInput`; else if the key-holder overlay's component === focused, its `preFocus`; else null. `setFocus` (`tui.ts:1287`): a key-holder overlay that does not own the target keeps the keys (records `preFocus` if it is the current focus); clears/sets `focused` flags; clears `released` when focus returns into a sheet; syncs `setUseTerminalCursor`.

### 1.8 Cursor management

- Hardware cursor target: `#targetHardwareCursorState(cursorPos,totalLines)` clamps row to `[0,totalLines-1]`, col ≥ 0, `visible = #showHardwareCursor`; `null` when no marker or no lines.
- Paint writes `CSI row+1;col+1H` + `CSI ?25h`/`?25l` when a marker was found; otherwise parks the hidden cursor with `CSI ?25l` + `CSI mutableTop+1;1H` and records `#parkedViewportOffset`. `#paintEndSequence` closes the frame.
- `#recordHardwareCursorState/Hidden/Forget` maintain `#hardwareCursorRow` and `#hardwareCursorState` for dedupe and shell handoff.
- `stop()` forces `terminal.showCursor(true)`.

### 1.9 Resize model (`tui.ts:1923-2440`)

Two paths, chosen by `#resizeRepaintsInPlace()` (env override; Warp in-place; multiplexers/hostOwnsGridOnResize take the in-place path) and `#resizeAvoidsAltBuffer()` (also avoids the borrow for tmux `rebuild` + synchronized output).

- **Alt borrow**: `#beginResizeAltPaint(restartingProbe?)` tracks the burst, enters `#resizeAltActive`, `setAltScreenActive(true)`, snapshots `#resizeProbeWindow/Offset`, blanks the live viewport (`\x1b[?25l` + erase) so only committed rows can scroll, and holds `#pendingAltEnter = erase + \x1b[?1049h + keyboardEnhancementEnter` for the first resize frame's synchronized write. Settle after 120 ms: `#settleResizeAltPaint()` — if `rebuild` and not clearing, prepare the replay and fuse the exit into the destructive frame (`#resizeAltExitFused`); else write `pendingAltEnter + exitSequence`, `setAltScreenActive(false)`, then CPR anchor probe.
- **In place**: `#beginResizeInPlacePaint()` tracks burst, sets `#resizeInPlaceActive`, erases the live viewport (height-shrink uses `#reflowedRowCount` bound; otherwise cursor-relative), waits 120 ms, prepares the replay (if rebuild), then probes.
- **Anchor probe**: `#beginResizeAnchorProbe(retry)` parks a distinct column `CSI colG` in `[17, 17+span)` (span `min(30, columns-16)`, needs ≥4) followed by `CSI 6n` and `CSI 1G`; columns 1–16 are never tags (column 1 indistinguishable; modified F3 collides on 2–16). `hostOwnsGridOnResize` without a multiplexer (and no `PI_TUI_RESIZE_IN_PLACE=1`) resolves immediately with no DSR. Retry once for multiplexers/grows. Timeout 200 ms.
- **Anchor resolution** `#resolveResizeAnchor(reportedRow)`: multiplexer: trust CPR (`max(0,reportedTop)`); CPR-less shrink models the clip (`discardedBelow = min(shrink, prevHeight-1-parkedRow)`, `pushed = shrink - discardedBelow`); CPR-less grow/reversed burst anchors at `providerViewportTop + #resizeBurstPull`. Direct: `fallbackTop = (reportedRow===undefined && burstGrew) ? providerViewportTop + pull : reportedTop`; `top = max(0, min(fallbackTop, height - staleRows))`. Then `providerViewportTop = min(top, max(0,height-1))`, pad 0, force repaint, `requestRender(true)`.
- Warp alt-toggle echo: a height-only ±1 SIGWINCH against the toggle baseline, single-shot; swallowed without restarting the borrow.
- `#reflowedRowCount(window,start,end,width)`: `Σ max(1, ceil(visibleWidth(row)/max(1,width)))`.
- `#prepareResizeReplay(width,height)` chooses `append|rebuild|preserve`; `#renderResizeAltFrame` renders the full semantic tail on the borrowed buffer.

### 1.10 Debug handler / server hooks

- `onDebug?: () => void` (`tui.ts:958`) fired on `shift+ctrl+d` before input forwarding.
- `getDebugPaint()`: last painted lines, window top, alt flag, cursor.
- `OMP_TUI_DEBUG=<socket path>` starts `TuiDebugServer` (`debug-server.ts`); `injectDebugInput(data)` feeds the normal input path.
- `getNativeDocument/getNativeFrames/nativeFallbackCount/nativeRendering/closeNative` are TSP-specific.

---

## 2. `terminal.ts` — `Terminal` interface and `ProcessTerminal`

### 2.1 `Terminal` interface (`terminal.ts:579-782`)

```ts
interface Terminal {
  start(onInput:(data:string)=>void, onResize:()=>void, onDisconnect?:()=>void, options?:TerminalStartOptions): void;
  enableInput?(): void;
  stop(): void;
  drainInput(maxMs?: number, idleMs?: number): Promise<void>;   // defaults 1000 / 50
  write(data: string): void;
  get columns(): number;
  get rows(): number;
  readonly pendingOutputBytes?: number;
  readonly hostOwnsGridOnResize?: boolean;                      // ConPTY ⇒ true
  get kittyProtocolActive(): boolean;
  get kittyEnableSequence(): string | null;                     // "\x1b[>5u" / "\x1b[>7u" / null
  readonly keyboardEnhancementEnterSequence?: string | null;
  readonly keyboardEnhancementExitSequence?: string | null;
  readonly keyboardEnhancementRestoreSequence?: string | null;
  moveBy(lines: number): void;
  hideCursor(force?: boolean): void;
  showCursor(force?: boolean): void;
  setCursorShape?(shape: CursorShape): void;                    // DECSCUSR
  clearLine(): void; clearFromCursor(): void; clearScreen(): void;
  setTitle(title: string): void;
  setProgress(active: boolean): void;
  onAppearanceChange(cb:(appearance, requestToken?)=>void): void;
  onAppearanceReport?(cb): (()=>void)|void;
  refreshAppearance?(token?): token|void;
  get appearance(): TerminalAppearance | undefined;
  onPrivateModeReport?(cb: PrivateModeReportHandler): void;
  onSixelSupport?(cb:(supported:boolean)=>void): void;
  onProgramStatusSupport?(cb): ()=>void;
  onGlyphProtocolReport?(cb: GlyphProtocolReportHandler): void;
  onTspHello?(cb: TspHelloHandler): void;
  readonly tspProbePending?: boolean;
  readonly tspExpected?: boolean;
}
```
`TerminalStartOptions { deferInput?: boolean; isLoopStalled?: () => boolean }`. `PrivateModeReportHandler(mode, supported, confirmed?, status?)`; DECRPM status 0=unrecognized, 1/2=set/reset, 3=permanently set, 4=permanently reset. `CursorShape = "default"|"block"|"underline"|"bar"`, `CURSOR_SHAPE_CODES = {default:0, block:2, underline:4, bar:6}` (DECSCUSR `CSI <n> SP q`).

### 2.2 `ProcessTerminal` (`terminal.ts:876`)

Construction-time overrides: `{ conpty?: boolean; nativeWindowsConsole?: boolean }`; `#headless = isTerminalHeadless()` (tests suppress all side effects and `#safeWrite`).

`start()` (`terminal.ts:1171`): stores handlers; `#cursorVisible = undefined`; headless → return; registers postmortem restore; `setActiveTerminal(this)`; resets background RGB; `terminalEverStarted = true`; creates the off-thread `TtyWriter(1)` pump on non-Windows TTYs (not under `bun test`); `suppressTerminalStderr()`; installs the `process.stdout` `"resize"` listener (clears cursor visibility, reconciles in-band geometry, calls `#resizeHandler`); on POSIX sends itself `SIGWINCH` to refresh dimensions; if `deferInput`, set flag and return; else `#attachInput()`.

`#attachInput()` (`terminal.ts:1256`):
- capture `#wasRaw = stdin.isRaw`; `setRawMode(true)` (failure → `#markTerminalDisconnected("stdin raw mode setup failed")`); `#cookedTypeahead = !wasRaw`.
- `stdin.setEncoding("utf8")`; listeners `end`/`close`/`error` → `#markTerminalDisconnected(...)`; `stdin.resume()`.
- write `CSI ?2004h` (bracketed paste); set `#rearmBracketedPaste` for multiplexers that gate paste markers.
- write `CSI ?1l` + `ESC >` (rmkx: normal cursor keys / numeric keypad).
- `#enableWindowsVTInput()` (Windows: `ENABLE_VIRTUAL_TERMINAL_INPUT` on stdin console mode; restore captured).
- `PROBE_GUARD_BEGIN = "\x1b7\x1b[?7l\x1b[8m"` (DECSC + autowrap off + conceal) then:
  - `#queryAndEnableKittyProtocol()`: push DA1 sentinel owner `keyboard`; write `CSI ?u` + `CSI c`; arm a 150 ms timeout to `#enableModifyOtherKeysFallback()`.
  - `#queryBackgroundColor()` (OSC 11 + DA1 sentinel), `#queryOsc99Support()`, `#queryProgramStatusSupport()`, `#queryGlyphProtocolSupport()`, `#queryTspSupport()`.
  - `CSI ?2031h` (appearance-change notifications).
  - DECRQM probes: `CSI ?2026$p`, `CSI ?2048$p`, `CSI ?2031$p`, `CSI ?2004$p`, and `CSI ?1010$p`, `CSI ?1011$p` — each fused with `CSI c` as sentinel; results resolve first-wins, drive `#enableInBandResize()` (2048), Windows appearance polling (2031), `#rearmBracketedPaste` + `stdinBuffer.setRawPasteStallProbe` (2004), and disable xterm scroll-to-bottom modes if set.
  - `PROBE_GUARD_END = "\x1b[28m\x1b[?7h\x1b8"`.

Kitty enable flags (`terminal.ts:2376`, reply handler): on `CSI ?<flags> u`, set `kittyProtocolActive`, clear a modifyOtherKeys fallback, disable win32 mode, then:
- ConPTY: `flags = (reported & 2) ? 3 : 1` (flag 4 dropped — ConPTY loses Shift+letter);
- non-ConPTY with event reporting already on: `7`;
- otherwise `5`.
Push `CSI >flags u`. `keyboardEnhancementEnterSequence` = `CSI >flags u`; `ExitSequence` = `CSI <u`; `RestoreSequence` = `CSI =flags;1u` (repairs Windows Terminal 1.25–1.27 losing flags on `?1049l`). `modifyOtherKeys` fallback: enter `CSI >4;2m`, exit `CSI >4;0m`; skipped when kitty active, under SSH for unknown terminals, and replaced by win32-input-mode (`CSI ?9001h`) on ConPTY.

Input path: `stdin data` → (cooked typeahead `restoreCookedEnter`) → reassert `CSI ?2004h` when `#active && #rearmBracketedPaste && !pasting` → `Win32PasteMarkerNormalizer` (if present) else `StdinBuffer.process`. `StdinBuffer` emits `data` sequences and `paste` events; paste is re-wrapped `\x1b[200~<content>\x1b[201~<enter?>` (win32 `decodePaste` first). The `#stdinSequenceHandler` reassembles split probe replies (private CSI, DEC 2048, OSC 11/99/7501, TSP APC/OSC 877, glyph APC), splits a CSI from an embedded ESC, holds a flushed `CSI [` introducer 150 ms, and finally applies AltGr recovery (`translateWindowsAltGrSequence`) before `#inputHandler`.

`write(data)` (`terminal.ts:2830`): if active and `#rearmBracketedPaste`, prefix `CSI ?2004h`; `#safeWrite`; optional `PI_TUI_WRITE_LOG` append (OSC 52 payloads replaced by `<N bytes>`). `#safeWrite`: headless/dead/`!stdout.isTTY` → no-op; register stdout error handler; `#trackCursorVisibility`; with pump → `pump.write`, detect dead pump, update `#pumpBacklogBound`, `#trackStdoutBacklog`; without pump → `pushLoopPhase("ui.terminal-write")`, Windows `ensureWindowsConsoleUtf8()`, ConPTY chunking (`chunkForConPTY`, max 16 KiB UTF-8, cut at `\n`/`\r` boundaries, never splitting a surrogate pair or `\r\n`), then `process.stdout.write`, `#trackStdoutBacklog`, `popLoopPhase`.

Dimensions: `columns = (#inBandResizeActive && #reportedColumns) ? reported : process.stdout.columns || COLUMNS || 80`; `rows` analogous with `LINES || 24`. `pendingOutputBytes`: pump bound re-read only when above 256 KiB, else `process.stdout.writableLength ?? 0`.

`drainInput(maxMs=1000, idleMs=50)` (`terminal.ts:2530`): pop kitty (`CSI <u`), disable modifyOtherKeys, disable win32 input, detach the input handler, drain stdin until `maxMs` or `idleMs` of silence; for an active/pending TSP session send `CSI c` and wait for its DA1 sentinel so late acks are swallowed; restore handler and clear CSI/introducer state.

`stop()` (`terminal.ts:2610`): clear active/deferred; release active-terminal ownership and background RGB; headless → return; `restoreTerminalStderr()`; clear progress; write `CSI ?2026l` + `CSI ?7h`; `CSI ?1l` + `ESC >`; `CSI ?2004l`; `CSI ?5522l`; restore cursor shape to default; mouse off `CSI ?1006l\x1b[?1003l\x1b[?1000l`; `CSI ?2031l`; restore xterm scroll-to-bottom modes; `CSI ?2048l`; clear timers/callbacks/probe buffers; pop kitty; disable modifyOtherKeys; disable win32; restore Windows VT input; destroy StdinBuffer; remove stdin/stdout listeners; stop pump (1000 ms); `stdin.pause()`; restore raw mode (`setRawMode(#wasRaw)`, throwing only when not dead); clear cursor tracking.

`emergencyTerminalRestore()` (`terminal.ts:471`): if an active terminal exists, pop the alt-screen keyboard frame (`ExitSequence ?? (kittyEnableSequence ? "\x1b[<u" : "")`) + `CSI ?1049l`, `stop()`, `showCursor(true)`. Otherwise blind restore (only when `terminalEverStarted && !headless`): `CSI ?2026l`, `CSI ?7h`, `CSI ?1l ESC >`, `CSI ?2004l`, `CSI ?2031l`, `CSI ?2048l`, `CSI ?5522l`, `CSI <u`, `CSI >4;0m`, `CSI ?9001l`, `CSI ?1006l\x1b[?1003l\x1b[?1000l`, and if alt active `CSI ?1049l\x1b[?1l\x1b> \x1b[<u`, then `CSI 0 SP q` (default shape), `CSI ?25h`, `setRawMode(false)`.

Other: `moveBy(lines)` → `CSI nB` / `CSI nA` (0 = no write); `hideCursor/showCursor` dedupe against the last written visibility (`force` bypasses); `setCursorShape` writes `CSI <code> SP q` on change and `stop()` restores default; `clearLine` = `CSI K`; `clearFromCursor` = `CSI J`; `clearScreen` = `CSI H` + `CSI 0J`; `setTitle` = `OSC 0;<title> BEL`; `setProgress` = `OSC 9;4;3 BEL` / `OSC 9;4;0; BEL` with a 1 s keepalive for Ghostty/Monstar.

`StdoutStallWatchdog` (`terminal.ts:310`): arm at `MAX_STDOUT_BACKLOG_BYTES = 64 MiB`, clear at `STDOUT_BACKLOG_CLEAR_BYTES = 256 KiB`, poll 250 ms, timeout 60 s; an episode ends when the backlog drains below the clear level. Disconnect path `#markTerminalDisconnected`: declare terminal dead, disarm watchdog, log, invoke `onDisconnect` (TUI `stop()`), then `SIGHUP` (129) or `postmortem.quit(129)` on Windows.

`active-terminal.ts`: module-level `activeTerminal`; `get/setActiveTerminal`; shared `process.stdout` `"error"` listener with `registerStdoutErrorHandler`; `writeThroughActiveTerminal(data)` → `activeTerminal.write`; `writeTerminalSequence(data)` → main thread only, routes through the active terminal, else direct best-effort stdout.

---

## 3. `stdin-buffer.ts` and `bracketed-paste.ts`

### 3.1 `StdinBuffer` (`stdin-buffer.ts`)

Options/defaults: `timeout` 75 ms, `partialHoldTimeout` 150 ms, `pasteTimeout` 1000 ms, `pasteByteLimit` 64 MiB. Events: `data: [string]`, `paste: [content, enter?]`. `pasting` getter; `flush(): string[]`; `clear()`; `getBuffer()`; `destroy()`.

Splitting rules:
- A `Buffer` of one byte >127 is converted to `ESC + (byte-128)`.
- `extractCompleteSequences` walks by code point; non-ESC scalars are emitted individually; ESC-led sequences resolved by `resolveEscapeEnd`:
  - CSI: final byte `0x40–0x7E`; X10 mouse `ESC [ M` + 3 bytes; SGR mouse requires `SGR_MOUSE_COMPLETE = /^<\d+;\d+;\d+[Mm]$/`; caps `MAX_CSI_BYTES = 4096` (cap overflow → flush raw CSI bytes).
  - OSC/DCS/APC: `resolveStringEnd` scans for ST (`ESC \`) or BEL (OSC), rejects control bytes and non-base64 OSC 5522 payloads, caps `MAX_STRING_SEQ_BYTES = 16 MiB`; impossible byte → `STRING_ABORTED`, cap overflow → drop head + enter string-discard.
  - SS3: 3 bytes; ESC+1 char = meta chord.
  - `ESC ESC`: held for the flush window; with a third byte not `[`/`O`, emits one ESC and restarts (legacy Alt chord); with `[`/`O`, resolves the inner escape (meta-CSI / held Esc); `ESC ESC [ < …` is split into a bare ESC plus the SGR report (alt-modified mouse carries modifiers in the button bits).
- `#shouldHoldPartial`: `SGR_MOUSE_PARTIAL = /^\x1b\[<[\d;]*$/` or kitty active → hold up to 150 ms past the 75 ms timeout.
- String heads: `#bufferIsStringHead` (kitty active → any `ESC ]/P/_`; otherwise must match `LEGACY_STRING_REPLY_OPENER`, e.g. `OSC 11;`/`5522;`, `DCS 1$r`/`1+r`/`>|`/`!|`, `APC _G`/`_tsp;`/`_25a1;`); tracked with 5 s abandonment + 10 s dormancy; typed lone printable keys during a stall are remembered and replayed if the head is abandoned.
- Bracketed paste: on `ESC [200~`, emit keys before it, enter paste mode; paste content is joined; `ESC [201~` finishes, `paste` event, optional leading Enter (`\r`, `\n`, or kitty `CSI 13(;1)?u`) rides along; a lost end marker aborts via 1 s inactivity or 64 MiB cap; APC/TSP reports inside paste are routed to `data`, only an undecided escape suffix stays buffered.
- Raw (unbracketed) paste classification: the first ESC-free read containing CR/LF starts a fixed 10 ms window; two logical breaks (`isRawMultilineBurst`, CRLF counts once) classify a paste; `endsWithTypedEnter` (final CR with no earlier CR) emits `paste(content minus CR, "\r")`; a stall probe (`setRawPasteStallProbe`, set when DECRPM 2004 confirms) replays the burst as keys instead.
- Kitty printable dedup: a bare char identical to an unmodified kitty CSI-u printable within 25 ms is dropped.

### 3.2 `BracketedPasteHandler` (`bracketed-paste.ts`)

Constants `PASTE_START = "\x1b[200~"`, `PASTE_END = "\x1b[201~"`; default byte limit 64 MiB. `process(data)` returns `{handled:false}` or `{handled:true, pasteContent?, remaining}`; strips nested starts, aborts on byte cap delivering accumulated content; `decodeReencodedPasteControls` reverses tmux re-encodings `ESC [ <cp>;5u` and `ESC [27;5;<cp>~` for `cp` in `a–z/A–Z` to `0x01–0x1A` before control stripping.

---

## 4. `keys.ts` — canonical keys and matching

- Key names: letters `a–z`, digits `0–9`, symbols `` ` - = [ ] \ ; ' , . / ! @ # $ % ^ & * ( ) _ + | ~ { } : < > ? ``, specials `escape|esc`, `enter|return`, `tab`, `space`, `backspace`, `delete`, `insert`, `clear`, `home`, `end`, `pageUp`, `pageDown`, `up`, `down`, `left`, `right`, `f1–f12`. Modifiers `ctrl|shift|alt|super`; canonical chord order `ctrl+shift+alt+super+base` (`canonicalKeyId`).
- `Key` helper maps names to literals and builds typed chords (`Key.ctrl("c") === "ctrl+c"`, `Key.ctrlShift`, `Key.ctrlAlt`, `Key.shiftAlt`, `Key.ctrlSuper`, `Key.ctrlShiftAlt`, `Key.ctrlShiftSuper`, and reversed-order aliases).
- `parseKey(data)`: `matchesRawBackspace(data,4)` → `"ctrl+backspace"`; else `decodeKittyKeypadText(data) ?? parseKeyNative(data, kittyProtocolActive)`. `matchesKey(data,keyId)`: backspace heuristic, keypad fast path, else native matcher with the global kitty flag.
- `matchesRawBackspace`: `"\x7f"` → modifier 0; `"\x08"` → modifier 4 only when `PI_TUI_RAW_BACKSPACE_IS_CTRL=1` or direct Windows Terminal (not SSH, not in a multiplexer); otherwise modifier 0.
- Kitty: `KITTY_RELEASE_PATTERN = /^\x1b\[[\d:;]*:3[u~ABCDHF]$/`, `KITTY_REPEAT_PATTERN` with `:2`; `isKeyRelease/isKeyRepeat` false when kitty inactive or data contains `ESC [200~`.
- CSI-u decode (`decodeKittyPrintable`): modifier = value−1, lock mask `64|128` cleared; unsupported modifier bits or alt/ctrl/super ⇒ no text; text field codepoints `≥32` and `≠127` win; keypad operators `57410–57415` → `/ * - + =`; numpad `57399–57409` → `0–9 .` when unmodified; shift+`shiftedKey`; PUA `0xE000–0xF8FF` rejected; `mod:3` rejected.
- `extractPrintableText` = `decodePrintableKey` (kitty CSI-u, then xterm `CSI 27;<mod>;<cp>~`) else the raw string if it has no control chars (`<32`, `0x7F`, `0x80–0x9F`).
- `setKittyProtocolActive`/`isKittyProtocolActive` global state, set by `ProcessTerminal`.

---

## 5. Keybindings

### 5.1 `keybindings.ts`

- `TUI_KEYBINDINGS` defaults (ids → keys) as listed in source, e.g. `tui.editor.cursorLeft: ["left","ctrl+b"]`, `cursorWordLeft: ["alt+left","ctrl+left","alt+b"]`, `deleteWordBackward: ["ctrl+w","alt+backspace","ctrl+backspace","super+alt+backspace"]`, `input.newLine: ["shift+enter","ctrl+j"]`, `input.copy: "ctrl+c"`, `select.cancel: ["escape","ctrl+c"]`.
- `canonicalKeyId`: strips modifiers in `ctrl,shift,alt,super` order (case-insensitive, `+` required), lowercases base, `esc→escape`, `return→enter`, adds implicit `shift` for a single ASCII uppercase base without shift, sorts modifiers by the order above, joins `mod+…+base`.
- `addKeyAliases`: adds canonical, plus `shift+<symbol>` for the 20 shifted symbols (`! @ # $ % ^ & * ( ) _ + { } | : < > ? ~`).
- `KeybindingsManager(definitions, userBindings)`: rebuilds maps; user keys override defaults per id (`undefined` = default, `[]` = unbound); conflicts = user keys claimed by >1 binding (report only); `matches(data,id)` = `parseKey` → `canonicalKeyId` → set lookup; `matchesCanonical`; `getKeys`, `getDefinition`, `getConflicts`, `setUserBindings`, `getUserBindings`, `getResolvedBindings` (single key as scalar). Global `setKeybindings`/`getKeybindings` (defaults to `TUI_KEYBINDINGS`).

### 5.2 `app-keybindings.ts`

- `KEYBINDINGS` = TUI + app ids (`app.interrupt` escape, `app.clear` ctrl+c, `app.exit` ctrl+d, `app.suspend` ctrl+z, `app.display.reset` alt+l, `app.thinking.cycle` shift+tab, `app.thinking.toggle` ctrl+t, `app.model.cycleForward` ctrl+p, `cycleBackward` shift+ctrl+p, `select` alt+m, `selectTemporary` alt+p, `tools.expand` ctrl+o, `tools.toggleVisibility` ctrl+shift+o, `ask.hide` alt+h, `ask.note` ["n","alt+n"], `approval.approve` ["ctrl+enter","ctrl+q"], `editor.external` ctrl+g, `local.open` alt+g, `followUp` ["ctrl+q","ctrl+enter"], `retry` ["f5","alt+r"], `dequeue` ["alt+up","shift+up"], `btw.inject` alt+i, clipboard defaults platform-dependent (`ctrl+v` everywhere; `alt+v` win32, `super+v` darwin; pasteTextRaw `["ctrl+shift+v","alt+shift+v"]`, copyLine alt+shift+l, copyPrompt alt+shift+c), sessions (`new/tree/fork/resume` unbound, `observe` ctrl+s, `togglePath` ctrl+p, `toggleSort` ctrl+s, `rename` ctrl+r, `delete` ["backspace","delete"], `deleteNoninvasive` ctrl+backspace, `pin` alt+k, `unpin` alt+shift+k, `tree.previousBranch/nextBranch` unbound, `plan.toggle` alt+shift+p, `history.search` ctrl+r, `links.hint` alt+o, `stt.pushToTalk` space, `stt.toggle` unbound, `live.toggle` ctrl+l).
- Config format: `Record<string, KeyId | KeyId[] | undefined>` in `keybindings.yml`/`.yaml` (legacy `keybindings.json` migrated on load, write-back to YAML); profile config merged over the default-profile agent dir (inherited read-only). Legacy names migrated (`interrupt→app.interrupt`, `cursorUp→tui.editor.cursorUp`, …) and config reordered to `KEYBINDINGS` key order.
- `KeybindingsManager.create/inMemory/reload`; `getKeys` removes a fallback key claimed by another binding (`followUp`→`ctrl+q`, `dequeue`→`shift+up`); `getEffectiveConfig`; `getDisplayString`.

### 5.3 `keybinding-matchers.ts`

`matchesAppInterrupt` (fallback escape/esc), `matchesAppToolsExpand`/`appToolsExpandKey` (fallback ctrl+o), `matchesSelectCancel/Up/Down/PageUp/PageDown`, `appExternalEditorKey`/`matchesAppExternalEditor` (fallback ctrl+g), `matchesAppFollowUp`/`appFollowUpKeys` (fallback `["ctrl+enter","ctrl+q"]`), `appApproveKeys`/`matchesAppApprove`, `appBtwInjectKey`/`matchesAppBtwInject` (no fallback). `matchesEffectiveKey` additionally treats `data` starting with LF (`charCodeAt(0)===10`) and length>1 as ctrl+enter/ctrl+return.

---

## 6. `mouse.ts`

`parseSgrMouse(data)`: `/^\x1b\[<(\d+);(\d+);(\d+)([Mm])$/`; `col = c-1`, `row = r-1` (0-based), `release = suffix "m"`; `wheel = (button & 64) && !(button & 2) ? (button & 1 ? 1 : -1) : null`; `motion = (button & 32) && wheel===null`; `leftClick = !release && wheel===null && !motion && (button & 3) === 0`. `routeSgrMouseInput` requires the `\x1b[<` prefix; `routeSelectListMouse` handles wheel, hover (motion), left click via `hitTest`. Buttons: 0 left, 1 middle, 2 right, 32 drag, 35 move, 64/65 wheel up/down, 66/67 horizontal; modifier bits 4/8/16 in the low byte.

---

## 7. `deccara.ts`

`DECSACE_RECT = "\x1b[2*x"`, `DECSACE_DEFAULT = "\x1b[*x"`; `encodeDeccara(top,left,bottom,right,sgr) = "\x1b[top;left;bottom;right;sgr$r"` (1-based inclusive). `analyzeBgFillLine(line,width)`: only full-width rows consisting of printable text and CSI-SGR (`m`) sequences; tracks the last non-space glyph, the trailing space run's background, and consistency; returns `{cut,leftCol,bg}` or `null` for OSC/APC, non-SGR CSI, partial rows, default/mixed backgrounds, or malformed escapes. `nextBackground` understands 0/49 reset, 40–47/100–107, `48;5;n`, `48;2;r;g;b`, skips `38;5/38;2` foreground, bails on colon forms/malformed. `planDeccaraFills(lines,width,firstScreenRow=0)`: shortens each fillable row (`cut===0 → ""`, else prefix + `SEGMENT_RESET`), coalesces vertically adjacent identical `(left,right,bg)` groups into one rectangle, keeps a group only when removed bytes > rectangle bytes, and emits nothing unless `removedTotal - rectBytesTotal > DECSACE_WRAPPER_BYTES`; returns `{texts, sequence}` where `sequence = DECSACE_RECT + rects… + DECSACE_DEFAULT`.

---

## 8. `frame-rate.ts` and `loop-watchdog.ts`

- `DEFAULT_MAX_FPS = 30`, range 1–120; `minFrameIntervalMs()`, `setMaxFps()`, `parseMaxFps(value)` (integer; numeric strings accepted; throws otherwise). The TUI's input cadence is pinned at 30 fps regardless of this ceiling.
- `LoopWatchdog` (`loop-watchdog.ts`): defaults `intervalMs=250`, `thresholdMs=250`, `sleepMs=60_000`, `CPU_BUSY_RATIO=0.01`; each tick scheduled 250 ms ahead with a deadline; a tick >250 ms late is a block unless `blockedMs > sleepMs && cpuMs < blockedMs*0.01` (suspension). Logs `ui.loop-blocked` once per rising edge with `blockedMs`, `cpuMs`, phase label and `phaseMs` (only if the phase outweighs unlabeled time); timer is `unref`'d, `stop()` cancels and bumps a generation guard; `isStalled()` true while the armed tick is overdue or a block ended within `thresholdMs`. TUI arms it in `start()` and disarms in `stop()`.

---

## 9. `debug-server.ts` and `ttyid.ts`

`TuiDebugServer(tui, path)` listens with `IpcServer.listen(path, {framing:"lines"})`; NDJSON one request per line, one JSON response per line; malformed JSON → `{ok:false,error}`. Ops: `text` (last paint lines as plain text + `window_top`/`alt_screen`/`cursor`), `frame` (debug document rows), `doc` (native TSP document + fallback count), `tsp` (`{native, frames}`, optional `n`), `tree` (component tree + overlay bands), `values` (`debugState` map keyed `kind#id`/`kind[path]`), `info` (columns, rows, pid, overlays, focused kind, alt_screen, cursor), `keys` (token encoding: `C-`/`A-`/`M-`/`S-` prefixes, special-key table incl. `enter→\r`, `tab→\t`, `esc→\x1b`, `space→" "`, `backspace→\x7f`, `delete→\x1b[3~`, arrows/Home/End/PgUp/PgDn/Insert/F-keys; modified forms via `CSI 1;<mod><final>`, `CSI n;<mod>~`, `CSI <cp>;<mod>u`; quoted literals expand per character), `bytes` (raw), `paste` (`\x1b[200~…\x1b[201~`), `mouse` (`click`/`right-click`/`middle-click`/`move`(35)/`drag`(32)/`release`/`wheel-up`(64)/`wheel-down`(65), coords 0-based → `CSI <btn>;<x+1>;<y+1>M|m`), `quit` (stops the TUI and `process.exit(0)`). A live peer/unusable path disables the socket without affecting the app.

`getTerminalId()` (`ttyid.ts`): `/proc/self/fd/0` readlink on Linux (or `ttyname(0)` FFI elsewhere) → strip `/dev/`, `/`→`-` (e.g. `pts-3`); else env precedence `ZELLIJ_PANE_ID` (+ `ZELLIJ_SESSION_NAME`, separators normalized) → `zellij-<session>-<pane>`, `TMUX_PANE` → `tmux-<pane>`, `CMUX_SURFACE_ID` → `cmux-<id>`, `KITTY_WINDOW_ID` → `kitty-<id>`, `WEZTERM_PANE` → `wezterm-<pane>`, `TERM_SESSION_ID` → `apple-<id>`, `WT_SESSION` → `wt-<id>`; null when piped.

---

## 10. Windows input mode and AltGr

### 10.1 `windows-input-mode.ts`

- Record grammar `ESC [ <Vk> ; <Sc> ; <Uc> ; <Kd> ; <Cs> ; <Rc> _` (`W32IM_PATTERN`); releases and bare modifier VKs produce no input; `Vk=0`/`VK_PACKET` are relayed text.
- Encode rules: unmodified/legacy-expressible keys emit legacy bytes (`\r`, `\t`, `\x7f`, `ESC`, space, `CSI A/B/C/D/H/F`, `SS3 P/Q/R/S`, tilde keys); chords legacy encoding loses emit kitty CSI-u (`CSI 13;<mod>u`, `CSI 9;<mod>u` / `CSI Z` for shift+tab, `CSI 127;<mod>u`, `CSI 27;<mod>u`, `CSI 32;<mod>u`, `CSI <base>;<mod>u` for ctrl+shift / ctrl+J with Uc=LF). Dead-key composition: `COMPOSABLE_FIXED_VKS` (back/tab/return/esc/space) carrying `Uc>0x20` yield that text. AltGr (Right Alt + Left Ctrl) with printable `Uc` yields the text.
- `Win32InputModeDecoder`: joins UTF-16 surrogate halves; reassembles relayed VT (`vtState`: partial/complete/invalid/split, `ESC ESC [`/`O` stays grouped, `ESC ESC d` splits); `hasPendingSequence`, `hasPendingString` (OSC/DCS/APC head), `flushPendingSequence`; `decodePaste` replaces records in paste content with their text; `Win32PasteMarkerNormalizer` reconstructs `ESC [200~`/`201~` from per-key records before `StdinBuffer`, with a 75 ms expiry.

### 10.2 `windows-altgr.ts`

`translateWindowsAltGrSequence(data, host)`: only kitty CSI-u ending in `u`, not event type 3, modifier has Alt and only Alt/Ctrl/Shift, codepoint `≥0x20` and outside the kitty PUA; looks up `"<lowercased base>:<shift 0|1>"` in the active layout's AltGr table (built once per `HKL` via `ToUnicodeEx` with `TOUNICODE_NO_STATE_CHANGE`, caching by foreground-window thread layout) and returns the produced text only when Right Alt is held (latched `RIGHT_ALT_LATCH_MS = 30`). Empty table (US/UK layouts) short-circuits. `readAltGrLayer(hkl)` test seam; `createRightAltLatch`.

---

## Cross-file architecture

`ProcessTerminal.start` owns stdin/stdout and emits `data`/`paste` sequences; `TUI` consumes them through `#handleInput` (TUI-wide listeners → debug → focused component, with overlay/capture precedence), composes rows from a `TUIFrameProvider` or `Container.render`, diffs against `#providerWindow` (or the alt frame), and writes through `terminal.write` (bracketed by DEC 2026, autowrap discipline, cursor parking). `active-terminal` makes all out-of-band escape writes ordered behind the same output path. `StdinBuffer` + `BracketedPasteHandler` + win32 decoder normalize partial reads and pastes; `keys`/`keybindings` normalize raw bytes into canonical ids; `mouse` and `deccara` are pure decoders/optimizers used by the renderer; `frame-rate`/`loop-watchdog` pace and observe the loop; `debug-server`/`ttyid` expose inspection and terminal identity; resize is a two-path (alt-borrow vs in-place) transaction with CPR-anchored recovery.

Checks to run after implementation: `l/test/stdin-buffer.test.ts`, `windows-input-mode.test.ts`, `windows-altgr.test.ts`, `ttyid.test.ts`, `resize-settle-fused-exit.test.ts`, `unsynchronized-row-moves.test.ts`, `resize-anchor-recovery` behavior, and the keybinding/key-hint tests.