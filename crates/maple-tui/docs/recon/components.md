# Behavioral spec — `l/src/components/**` + helpers → `crates/maple-tui`

Coverage note: contracts below are from direct reads this session. Items marked **[unverified]** were in the archive but their bodies were elided; I did not re-read them under the request budget, so treat those as leads, not specs.

## 0. Cross-cutting contracts

**`Component`** (`l/src/tui.ts:254`): `render(width: number): readonly string[]` (component-owned, stable by reference when unchanged), optional `describe?(cx): NativeNode | null`, `debugId`, `debugKind`, `debugState?()`, `debugChildren?`, `invalidate?()`, `releaseRenderCaches?()`, `dispose?()`, `setIgnoreTight?(bool)`, `setHeight?(number)`, `handleInput?(data)`, `routeMouse?(event,line,col)`, `handleNativeEvent?(event)`.
**`Focusable`** (`tui.ts:392`): `focused: boolean`; when true the component must emit `CURSOR_MARKER`.
**`CURSOR_MARKER`** (`tui.ts:423`) = `"\x1b_pi:c\x07"` (ESC `_pi:c` BEL), zero-width, emitted before the cursor cell for IME placement.
**`Container`** (`tui.ts:537`): `children: Component[]`; `addChild/removeChild/clear/disposeChildren/invalidate/releaseRenderCaches/dispose/setIgnoreTight`; `render(width)` clamps width to ≥1, renders every child at the same width, concatenates, and memoizes by child-array reference + width. Native: `col(children)`.

Shared width helpers (from `utils`): `getPaddingX(px)` (tight-mode padding), `padding(n)` = `" ".repeat(n)`, `visibleWidth` (ANSI-aware cell width), `truncateToWidth(text,w,ellipsis?)`, `wrapTextWithAnsi(text,w)`, `replaceTabs` (tab→3 spaces), `sliceWithWidth`, `getWidthConfigEpoch`, `publishLineWidths`, `Ellipsis.Omit|Unicode`, `finiteWidth`, `singleLine`.

## 1. `box.ts` — Box
- `interface BoxBorder { chars: {topLeft,topRight,bottomLeft,bottomRight,horizontal,vertical}; color?: (s)=>s }`.
- Fields: `children: Component[]`, private `#paddingX`, `#paddingY`, `#bgFn?`, `#border?`, `#ignoreTight`.
- API: `addChild`, `removeChild`, `clear`, `setPaddingX`, `setPaddingY`, `setBgFn` (no cache invalidation; sampled at render), `setBorder`, `invalidate`, `releaseRenderCaches`, `debugState`, `describe`.
- `render(width)` (`box.ts:176`): `paddingX = ignoreTight ? px : getPaddingX(px)`; border used only if `width - 2 >= paddingX*2 + 1`; `innerWidth = border ? width-2 : width`; `contentWidth = max(1, innerWidth - paddingX*2)`. Children render at `contentWidth`. Interior rows: top `paddingY` blanks, children (left-padded by `paddingX`, right-padded to `innerWidth`), bottom `paddingY` blanks; each interior row passed through `bgFn` if set. Border: top `topLeft + horizontal.repeat(innerWidth) + topRight`, sides `vertical + row + vertical`, bottom similarly; `border.color` paints border glyphs. Empty result when no content rows. Cache keyed on width, width-config epoch, sampled `bgFn("test")`, sampled border color/chars, child line arrays (reference or snapshot) and published widths.
- Native `describe`: `col` when no border/bg; otherwise `card({role,tone,selected,inset})` (inset=true when no border).

## 2. `text.ts` — Text
- Ctor `(text, paddingX=0, paddingY=0)`; private `#styleFn?`, `#customBgFn?`, `#wrapLine?`, `#ignoreTight`.
- `setText(text): boolean` (false if unchanged), `setCustomBgFn`, `setStyleFn` (returns this), `invalidate`, `releaseRenderCaches`, `describe`.
- `render(width)` (`text.ts:176`): cache on (text,width,widthEpoch). Empty/whitespace-only text → `[]`. Normalizes `styleFn(text)` then `replaceTabs`; `contentWidth = max(1, width - paddingX*2)`; wrap via `#wrapLine` per `\n` line or `wrapTextWithAnsi`; each row = `leftMargin + line + rightMargin`, then either `applyBackgroundToLine(..., width, customBgFn)` or pad with spaces to `width`. Top/bottom `paddingY` blank rows (width-wide, bg-applied if customBgFn). Returns `result.length ? result : [""]`. Publishes exact widths when no bgFn.

## 3. `truncated-text.ts` — TruncatedText
- Ctor `(text, paddingX=0, paddingY=0)`; `debugState()` reports `textPreview` (first 120 chars), `textLength`, `previewTruncated`, `truncated` (true if source has `\n` or `cachedWidth - paddingX*2 < visibleWidth(firstLine)`).
- `render(width)`: takes text before first `\n`; `availableWidth = max(1, cachedWidth - paddingX*2)`; `truncateToWidth`; line = left pad + display + right pad (no full-width padding); then `paddingY` blank lines. Caches width/lines.

## 4. `input.ts` — Input (single-line)
- Fields: `prompt = "> "`, `mask = false`, `placeholder?`, `onSubmit?`, `onEscape?`, `readonly spaceHold: SpaceHoldGesture`, `cursorOverride?`, `focused`.
- API: `getValue`, `getCursor` (UTF-16 offset), `setValue(value,{cursor?})` (default end), `setUseTerminalCursor`/`getUseTerminalCursor`, `capturesInput`, `pasteText`, `handleNativeEvent`, `applyHostEdit`, `submit`, `deleteBeforeCursor`, `setVolatileText`/`clearVolatileText`/`commitVolatileText`, `debugState`.
- `toSingleLine`: `replaceTabs(decodeReencodedPasteControls(text).replace(/\r\n/g,"").replace(/\r/g,"").replace(/\n/g,"")).normalize("NFC").replace(/[\x00-\x1F\x7F]/g,"")`.
- `handleInput(data): boolean` (`input.ts:132`): bracketed-paste first (recurses on remainder). Space-hold gesture (`getSpaceHoldText`) → `"type"` inserts text and records, `"swallow"` returns true. Then keybindings in order: `tui.select.cancel` (calls `onEscape` if set, else returns false), `tui.editor.undo`, `tui.input.submit` or `"\n"` (calls `onSubmit` if set else false), `deleteCharBackward`, `deleteCharForward`, `deleteWordBackward`, `deleteWordForward`, `deleteToLineStart`, `deleteToLineEnd`, `yank`, `yankPop`, `cursorLeft` (grapheme step), `cursorRight` (grapheme step), `cursorLineStart`, `cursorLineEnd`, `cursorWordLeft`, `cursorWordRight`, then `extractPrintableText` → insert. Returns false only for unbound keys and cancel/submit without handlers.
- Editing semantics: word typing coalesces undo; deletion/kill/yank push undo. Kill ring: `deleteToLineStart/End` and word deletes `push` with `prepend` + accumulate-on-kill; `yank` inserts `peek`; `yankPop` only after yank with >1 entries, rotates and replaces.
- `applyHostEdit`: masked fields map bullet offsets→value offsets via grapheme starts; stale `len` dropped; `resolveTextEdit` with `toSingleLine`; one undo unit.
- `render(width)` (`input.ts:578`): width truncated ≥0; `availableWidth = width - visibleWidth(prompt)`; if ≤0 return `sliceWithWidth(prompt,0,width,true).text`. Mask → `"•"` per grapheme, cursor = graphemes before cursor. At end, append `" ".repeat(override?.width ?? 1)`; `cursorColumnWindow` gives visible text + cursor index; cursor cell = `override` or inverted grapheme (`\x1b[7m…\x1b[27m`) unless `useTerminalCursor`; `CURSOR_MARKER` prepended when focused; trailing text clamped to remaining width; line = `prompt + before + marker + cursorChar + after + pad`.

## 5. `editor.ts` — Editor
- `EditorTheme` (`editor.ts:654`): `borderColor(str)`, optional `accentColor`, `surfaceColor`, `textColor`, `selectList: SelectListTheme`, `symbols: SymbolTheme`, `editorPaddingX?`, `hintStyle?`.
- Provider interfaces: `AutocompleteProvider` (seen earlier: `getSuggestions`, `applyCompletion`, optional `trySyncSlashCompletion`, `trySyncModelMentionCompletion`, `getForceFileSuggestions`, `shouldTriggerFileCompletion`, `getInlineHint`), `EditorTextAssistProvider` (`editor.ts:705`): `getWordCompletion?`, `wordCompletionFeedback?`, `tryAutocorrect?` → `EditorInlineReplacement | null | Promise`, `getWordReplacements?` → `EditorWordReplacements | null | Promise`. `PasteOptions { submitAfterPaste? }`.
- `EditorState`: `{ lines: string[], cursorLine, cursorCol }`.
- `handleInput` jump mode (`editor.ts:2070`): if `#jumpMode` set, pressing the same jump hotkey cancels; a printable char jumps to next occurrence (`#jumpToChar`) and clears; a control key clears and falls through.
- `#tryTriggerAutocomplete(explicitTab)` (`editor.ts:5061`): returns if no provider; when `explicitTab` and `shouldTriggerFileCompletion` exists and returns false, returns; else queues `{kind:"regular", explicitTab}`.
- `#acceptPrediction` (`editor.ts:5165`): inserts `#getShownPrediction().suffix`, records `#insertedPrediction = lines[0]`, fires `onPredictionAccepted(index)`.
- `#acceptWordCompletion({space})` (`editor.ts:5150`): inserts shown word completion (+trailing space if Tab), sets `#lastAction = "accept-word"` when space, feeds `wordCompletionFeedback(...,true)`.
- `#getShownPrediction` (`editor.ts:5539`): null if autocomplete open, no predictions, `lines.length !== 1`, cursor not at end, or (≥2 predictions and buffer equals `#insertedPrediction` and is one of them); else first prediction strictly longer than typed with typed as prefix → `{index, suffix}`.
- `#getInlineHint` (`editor.ts:5507`): selected autocomplete item's `hint` → prediction suffix → alternatives ` <tab> N more` → provider `getInlineHint` → word completion.
- Tab completion order (`#handleTabCompletion`): accept prediction → show alternatives → accept word completion(space) → slash-command completion / mid-prompt skill / force file completion.
- `[unverified]` full keybinding table in `#handleInputChunk` (2517–2800) and `#jumpToChar`; read the file for exact bindings.

## 6. `markdown.ts` — Markdown
- Ctor `(text, paddingX, paddingY, theme: MarkdownTheme, defaultTextStyle?, codeBlockIndent = 2)` (`markdown.ts:2573`). Normalizes OSC-8 terminators; stores `#symbols = theme.symbols ?? getSymbolTheme()`.
- `render(width)` (`markdown.ts:3234`): L1 cache on (text,width,extensionLinks). `paddingX = ignoreTight ? px : getPaddingX(px)`; `contentWidth = max(1, width - paddingX*2)`. Empty/whitespace text → `EMPTY_RENDER_LINES`. Lex tokens; wrap; content lines get `leftMargin + line + rightMargin`, then either `applyBackgroundToLine(..., width, bgFn)` or space-pad to `width`. Image lines and OSC-66 sized headings are emitted raw (no margins/bg); literal code rows emitted raw. Top/bottom `paddingY` blank width-wide lines (`#renderEmptyPaddingLines`, `markdown.ts:4059`). Result `[emptyLines, ...contentLines, ...emptyLines]` or `[""]`.
- Fast paths: append-only same-line delta splice (`#fastTail`) with hazard disarms (line-start, emphasis delimiter seam, pair seam, `$`+digit, tab/newline/ESC deltas); L2 module LRU keyed by text + width/paddingX/paddingY/codeBlockIndent/themeId/styleId/imageProtocol/hyperlinks/extensionLinks/textSizing/bgColorProbe/headingProbe/symbolsProbe.
- `MarkdownTheme` fields **[unverified — not re-read]**; probe uses `theme.heading("",1)` as representative.

## 7. `select-list.ts` — SelectList (+ SelectItem, SelectListTheme, layout)
- `SelectItem` (`select-list.ts:29`): `value`, `label`, `description?`, `icon?`, `iconName?`, `nativeDetail?`, `state?`, `hint?`, `disabled?`, `confirmation?`, `searchText?`.
- `SelectListTheme` (`:53`): `selectedPrefix`, `selectedText`, `description`, `scrollInfo`, `noMatch`, `symbols: SymbolTheme`, optional `icon`, `hovered`.
- `SelectListLayoutOptions` (`:83`): `minPrimaryColumnWidth?`, `maxPrimaryColumnWidth?`, `truncatePrimary?`, `overflowSearch?`, `search?: "overflow"|"always"|"never"`, `filterItems?`, `renderItem?`, `measureItem?`, `emptyText?`, `noMatchText?`, `statusText?`, `wrapNavigation?` (default true), `wrapDescription?` (default false), `maxDescriptionRows?`.
- Ctor `(items, maxVisible, theme, layout = {})`; `maxVisible = max(1, trunc)`. Constants: `DEFAULT_PRIMARY_COLUMN_WIDTH=32`, `PRIMARY_COLUMN_GAP=2`, `MIN_DESCRIPTION_WIDTH=10`, `DEFAULT_CURSOR_SYMBOL=">"`.
- API: `debugState`, `setMaxVisible`, `setFilter`, `setItems`, `setSelectedIndex`, `setSelectedValue`, `getFilter`, `pickerView`, `hitTest(line)`, `setHoverIndex`, `handleWheel`, `clickItem`, `routeMouse`, `describe`, `setNativeMark`, `handleNativeEvent`, `invalidate`, `render`, `handleInput`, `getSelectedItem`.
- `handleInput` (`:464`): `tui.select.cancel` → cancel pending confirmation else `onCancel`; printable text first (filter) then navigation: `up`/`down` (wrap per `wrapNavigation`), `pageUp`/`pageDown` (±maxVisible), `home`/`end`, `confirm` or `"\n"` → activate; anything else goes to search field.
- Search policy: `search ?? (overflowSearch===false ? "never" : "overflow")`; editable when `always`, or `overflow` and `items.length > maxVisible`. Empty query ignores whitespace and backspace.
- `render(width)` (`:412`): empty → optional status + `noMatch("  "+message)`. Computes primary column width clamped to `[min,max]` (default 32) from widest label+gap; icon column width = widest icon. Per-item row counts at `width-1`; `getMenuWindow` centers selection in `maxVisible` visual rows; overflow reserves 1 scrollbar column. Selected row: prefix `cursor + " "` else spaces; icon cell padded to icon column+1; description layout only when `width > 40` and `remainingWidth > MIN_DESCRIPTION_WIDTH`; primary truncated with `Ellipsis.Omit`; description truncated/wrapped. Rows passed through a `ScrollView` (height=rows.length, totalRows=window.totalRows, theme track=scrollInfo, thumb=selectedPrefix), offset `rowOffset`; optional status line. `#hitRows` maps output line → item index.
- Native `describe`: `list` node keyed by item value with `selected`, `filter`, `empty`, `max:{lines:maxVisible}`; items described with `mark` prefix for `setNativeMark`, `icon`, `label`, `detail`, `value`, `disabled`, `tone`.

## 8. `settings-list.ts` — SettingsList
- `SettingItem` (`:24`): `id`, `label`, `description?`, `warning?`, `currentValue`, `values?`, `submenu?: SettingEditor`, `menu?: SettingEditor`, `changed?`, `defaultLabel?`, `heading?`. `SettingEditor = (currentValue, done(selectedValue?)) => Component`.
- `SettingsListTheme` (`:112`): `label(text,selected,changed)`, `value(...)`, `description`, `warning?`, `warningMark?`, `cursor`, `hint`, `heading?`, `section?`, `hovered?`.
- `SettingsListOptions` (`:140`): `layout?: "auto"|"flat"`, `typeToSearch?` (default true), `emptyText?`, `hint?`, `sidebarWidth?`.
- Ctor `(items, maxVisible, theme, onChange, onCancel, options = {})`; `MenuSelection` with `getKey:id`, `getSearchText:getSettingItemFilterText`, `isDisabled: heading`, `filter: fuzzyFilter` over non-headings.
- API: `getSelectedItem`, `selectItem(id)`, `sectionFocused`, `hasSectionFocusTargets`, `toggleSectionFocus`, `hasOpenSubmenu`, `openSubmenu`, `prefsSections`, `prefsFocus`, `prefsEditing`, `applyValue`, `openSubmenuFor`, `setMaxVisible` (min 3), `handleWheel`, `handleWheelAt`, `setHoverItem`, `hitTest`, `hoverTest`, `routeSubmenuMouse`, `getSearchQuery`, `clearSearch`, `updateValue`, `setItems`, `invalidate`, `render`, `describe`, `handleNativeEvent`, `handleInput`.
- `handleInput` (`:1011`): if submenu open, delegate. `cancel` clears search → exits section focus → `onCancel`. Search input first. `up`/`down` (section focus jumps sections), `pageUp`/`pageDown` jump sections (or page when <2 sections), `confirm`/`" "`/`"\n"` clears section focus or activates.
- `#activateItem`: submenu → open editor; `values` → cycle `(indexOf(current)+1) % len`, mutate `currentValue`, call `onChange`.
- Submenu lifetime: `#openSubmenu` sets component, `done` applies value + onChange then closes; `#closeSubmenu` restores selection by id, clamps if gone.
- `render(width)` (`:631`): submenu replaces list, padded to `#stableHeight() = maxVisible + 4 + (typeToSearch?1:0) + (hint!==""?2:0)`. Main list: empty → hint; no matches → status + "No matching settings" + blank + "⌫ to edit search · esc to cancel". Split layout when `layout!=="flat" && !query && sections>=2 && paneWidth>=60`: sidebar (width `min(22,nameWidth)+4`, `"│ "` separator), active section highlighted, others dimmed; right pane whole list scrollable. Flat: window centered on selection, `maxLabelWidth = min(30, widest label+mark)`, ScrollView with totalRows, then blank + 3 description rows (warning first, wrapped at `width-4`, clamped to 3 with `…`), then search status, then blank+hint. Hint default: `"<confirm>/<space> to change · <pageUp><pageDown> to jump sections · Type to search · <cancel> to cancel"`.

## 9. `scroll-view.ts` — ScrollView
- Ctor `(lines|child, {height, scrollbar?: "auto"|"always"|"never", totalRows?, followTail?, theme})`.
- API: `setLines`, `setHeight`, `getVisibleRange`, `setActiveRow`, `scroll`, `page` (step `max(1,height-1)`), `scrollToTop/Bottom`, `handleScrollKey`, `invalidate`, `releaseRenderCaches`, `dispose`, `describe`, `render`, `revealRange`, `getMaxScrollOffset`, `logicalRowAt`, `getScrollOffset`, `hasRevealedRange`.
- `handleScrollKey` (`:380`): shift+up/down ±`#fastScrollLines`, up/down ±1, pageUp/Down, home/end.
- `render(width)` (`:461`): disposed or height 0 → `[]`. Child rendered at `width - (scrollbar?1:0)` (two-pass when totalRows unknown). `scrollOffset` clamped; `contentWidth = width - (scrollbar?1:0)`; each row truncated to contentWidth, padded, then track/thumb char. Thumb range from `scrollbarThumbRange(height,rowCount,offset)`. Native: `col([child|rowsText], {max:{h:`${height}lines`}, hidden: height===0})`.
- `revealRange(anchor,width)` (`:301`): validates finite `start>=0`, `end>start`; mode default `"selection"`, alignment `"nearest"`, margin 1, oversized `"nearest"`; `top = max(0, trunc(start)-margin)`, `bottom = min(total, trunc(end)+margin)`; alignment start/end/center set active row; nearest: if `top < visible.start` or (`oversized==="start"` and `bottom-top >= height`) → start; else if `bottom > visible.end` → end; else no change. Dedupes by anchor id + params + geometry; stores snapshot.
- Anchoring: `#reconcileBounds` keeps `#activeRow` in view via `scrollOffsetForRow`; else if `anchor==="end"` preserves trailing rows; else `followTail` snaps to bottom if previously at tail; else clamp.

## 10. `scroll-viewport.ts`
- Exports `viewportRange`, `centeredViewportRange`, `scrollbarThumbRange`, `cursorColumnWindow`, `scrollOffsetForRow`, `clampScrollOffset`, `maxScrollOffset`, `viewportOverflows`, `normalizedRows`, `type ViewportAlignment`. **[unverified — bodies not re-read this session]**; semantics inferred from call sites above.

## 11. `tree-view.ts` — TreeView
- Types: `TreeKey = string|number`; `TreeAncestor {key,depth,isLast,siblingCount}`; `TreeRow {item,key,parentKey,depth,index,siblingIndex,siblingCount,isLast,ancestors}`; `FlattenTreeOptions` (`roots,getKey,getChildren,isExpanded?,rootDepth?,getChildDepth?,compactSameDepthAncestors?,maxItems?`); `TreePrefix {first,continuation}`; `TreeViewRowContext`; `TreeViewRowContent = string | readonly string[] | Component`; `TreeViewOptions` extends flatten with `theme: Theme, renderRow, filter?, selectedKey?, maxRows?, maxLines?, centerSelection?, renderPrefix?, renderLeading?, styleSelected?, truncateRows? (default true), scrollbar?, scrollbarTheme?`; `TreeViewRenderResult {lines,truncated,windowStart,windowEnd,totalRows}`.
- `projectTree`: depth-first, roots pushed reversed; `isLast = siblingIndex === siblingCount-1`; ancestors lazy from a shared chain; `compactSameDepthAncestors` reuses parent chain when child depth unchanged and only child.
- `treeRowPrefix`: ancestor gutter `"   "` if ancestor last else `vertical+"  "`; connector `theme.tree.last` or `theme.tree.branch` + `" "`; continuation `"   "` or `vertical+"  "`; styles dim.
- API: `invalidate`, `dispose`, `setFilter`, getters `allRows`, `rows`, `selectedKey`, `selectedItem`, `selectionIndex`, `setSelectedKey` (walks to nearest visible ancestor), `setSelectionIndex`, `moveSelection(delta,wrap=false)`, `moveSelectionWhere`, `render`, `renderWithState`, `describe`.
- `renderWithState` (`:376`): cache on (width,version). `maxRows` default all; `centerSelection` defaults to `selectedKey !== undefined`; window via `centeredViewportRange` or `viewportRange`. Per row: prefix, `renderLeading`, `contentWidth = safeWidth - leading - prefix.first`, `renderRow`; string → one line, Component → `render(contentWidth)` and tracked in `#childByKey`, array → lines. First line uses `prefix.first`, rest `prefix.continuation`; `replaceTabs`; `styleSelected` on all lines of selected row; truncate to width unless `truncateRows===false`. `truncated` if projection truncated, window clipped, or `maxLines` hit. Optional ScrollView scrollbar with `totalRows`.
- Native `describe`: `tree` of nested nodes keyed by row key, filtered-out ancestors lifted, selected row marked with `mark`; returns null if any row body is a component.

## 12. `disclosure.ts` — Disclosure
- `DisclosureSlot = Component | (() => Component)`; `DisclosureOptions {summary?, collapsedBody?, body: () => Component, expanded?, paddingX?, maxCollapsedRows?, collapsedHint?(hiddenRows,width)}`.
- Ctor lazy-materializes non-function slots; body factory runs only on first expanded render. `expanded` getter, `setExpanded` (clears caches), `setIgnoreTight`, `invalidate`, `releaseRenderCaches`, `dispose`, `debugState`, `debugChildren`, `describe`, `handleNativeEvent` (native `toggle` with `key===""` mirrors state).
- `render(width)`: width ≥1; `paddingX = clamp(configuredPadding, 0, floor((width-1)/2))`; `innerWidth = width - paddingX*2`; summary + (expanded ? body : collapsedBody) rendered at innerWidth; collapsed rows bounded by `maxCollapsedRows` (hint replaces last kept row when provided); every row wrapped as `pad + row + pad`; cache on width/expanded/row snapshots.

## 13. `table.ts` — Table
- `TableCell {text, style?}`; `TableColumn {width, minWidth?, align:"left"|"right", overflow:"allow"|"truncate", priority?, style?}`; `TableOptions {gap?, indent?, fit?}`.
- `renderTableRow(cells, columns, maxWidth?, options)`: count = min(cells,columns); `fitWidths` shrinks truncating columns by priority (lower first, rightmost tie) down to `minWidth ?? 1`; `alignCell` single-lines, truncates only when `overflow==="truncate"`, pads, right-aligns, applies cell/column style.
- `Table`: ctor `(rows, columns, options={})`; `setRows`, `invalidate`, `releaseRenderCaches`, `describe` (native `table` with `c{index}` columns and `r{index}` rows), `render(width)` caches on (width,epoch), maps rows through `renderTableRow` then truncates to width with `Ellipsis.Omit`.

## 14. `progress-bar.ts` — ProgressBar
- `ProgressBarStyle {filled, empty, indeterminate?, styleFilled?, styleEmpty?, styleIndeterminate?, styleBar?}`; `ProgressBarOptions {min?, max?, minWidth?, maxWidth?, prefix?, suffix?, showPercentage?, percentageSeparator?, formatPercentage?, style}`.
- `progressFraction(value,min=0,max=1)`: undefined → undefined; `max<=min` → `value>=max?1:0`; else clamp `(value-min)/(max-min)`.
- `renderProgressBar(value,width,options)`: bar width clamped `[minWidth,maxWidth]`; indeterminate uses `indeterminate ?? empty`; determinate `filledWidth = round(fraction*barWidth)`, `fillCells` repeats glyph and space-pads to width; prefix/suffix single-lined; percentage appended with separator when `showPercentage`.
- `ProgressBar`: ctor `(value, options)`; `setValue` (false if `Object.is` equal), `invalidate`, `describe` (native `progress` with `value: fraction??null`, label = prefix/suffix/percentage with frame punctuation stripped), `render(width)` computes chrome width, bar width = `renderWidth - chrome`, delegates to `renderProgressBar` with `minWidth:0`, truncates to width.

## 15. `metric.ts` — MetricRow
- `MetricSpec {value: string|undefined, leading?, separator?, priority?, style?}`; `MetricOverflow = "allow"|"drop"|"truncate"|"wrap"`; `MetricRowOptions {separator?, overflow?, maxWidth?, paddingX?, paddingY?, style?}`.
- `formatMetric`: undefined → undefined; `leading + (separator ?? " ") + value`; style applied.
- `formatMetricRow`: joins with separator; `allow`/`wrap` ignore maxWidth; `truncate` truncates; `drop` removes lowest priority (rightmost on ties) while over maxWidth.
- `MetricRow`: ctor `(metrics, options)`; wraps a `Text` with padding and style; `setMetrics`, `invalidate`, `describe` (`status` of `seg`s for `drop`, else `text` wrap word/none), `render(width)` computes `maxWidth = width - paddingX*2`, formats with `wrap`→`allow`, delegates to Text, then truncates any over-width line.

## 16. `key-value-list.ts` — KeyValueList
- `KeyValueRow {label, value, labelStyle?, valueStyle?}`; `KeyValueListOptions {indent?, labelWidth, gap?, minValueWidth?, labelOverflow?: "allow"|"truncate"}`.
- Ctor `(rows, options)`; `setRows`, `invalidate`, `releaseRenderCaches`, `describe` (native `kv` grid), `render(width)`: `constrainedLabelWidth = max(0, width - indent - gap - minValueWidth)`; `renderedLabelWidth = min(labelWidth, constrained)`; truncate label when `labelOverflow==="truncate"` or constrained < labelWidth; label cell padded; value wrapped at `width - prefixWidth`; continuation lines indented by prefix width; all lines truncated to width.

## 17. `section.ts` — Section
- `SectionOptions {title, body: Component|readonly string[], titleStyle?, ruleStyle?, ruleGlyph, ruleWidth: number|"fill", blankAfter?}`.
- `render(width)`: title single-lined/styled/truncated; rule = `fillCells(ruleGlyph, min(width, ruleWidth==="fill"?width:ruleWidth))` styled/truncated; child lines truncated; optional trailing blank. Cache on (width,epoch,child ref).
- `describe`: native `section` with `head` styled title; body component or `rowsText`.

## 18. `tab-bar.ts` — TabBar
- `Tab {id, label, short?, muted?}`; `TabBarTheme {label, activeTab, inactiveTab, hint, mutedTab?, hoverTab?}`.
- Ctor `(label, tabs, theme, initialIndex=0)`; `showHint = true`; `onTabChange?`.
- API: `getActiveTab`, `getActiveIndex`, `setActiveIndex` (clamped, fires on change), `setTabs(tabs, activeId?)` (preserves by id, no event), `setActiveById`, `selectTab` (ignores muted), `nextTab`/`prevTab` (skip muted, wrap), `invalidate`, `describe`, `handleNativeEvent`, `handleInput`, `render`, `tabAt(line,col)`, `setHoverTab`.
- `handleInput`: `tab`/`right` → next, `shift+tab`/`left` → prev.
- `render(width)`: chunks = optional `label:` + `"  "` + per tab `" " + label + " "` styled (muted→mutedTab??inactive, active→activeTab, hover→hoverTab??inactive) + `"  "` separators + optional hint `"  (tab to cycle)"`. If total > width, collapse tabs to `short` starting farthest from active. Then greedy line wrap; chunks wider than width are truncated and pushed alone; hit zones record `{line,start,end,index}`; `tabAt` scans zones.

## 19. `sparkline.ts`
- `SPARK_LEVELS = ["▁","▂","▃","▄","▅","▆","▇","█"]`; `SparklineOptions {min?=0, max?, style?(glyph,value,fraction), gap?=" "}`.
- `sparkGlyph(fraction)`: clamp 0..1, index `min(7, floor(f*8))`.
- `renderSparkline(values, options)`: max defaults to max finite value; span = max-min; fraction = span>0 ? (v-min)/span : (v>min?1:0); undefined/non-finite → gap.
- `fitSparkValues(values, cells)`: bucket means, newest last; shorter unchanged.
- `peakBuckets(points, sinceMs, nowMs, cells)`: max per bucket, empty buckets undefined.

## 20. `wizard-step.ts` — WizardStep
- `WizardStepKind = "input"|"choice"|"confirm"|"async"|"custom"`; `WizardStepSlot {component, optional?}`; `WizardStepOptions {kind, content, heading?, intro?, preview?, status?, footer?, gap?=1, minContentLines?=1, fitContent?(maxLines)}`.
- Ctor stores slots; `setKind`, `setHeading`, `setIntro`, `setPreview`, `setContent`, `setStatus`, `setFooter`, `setMaxHeight`, `invalidate`, `dispose`, `handleInput` (delegates to content), `routeMouse` (translates by `#contentRowStart`; wheel passes through regardless), `describe` (col of slots, gap `spaceForRows(gap)`).
- `render(width)`: renders heading/intro/preview/status/footer; optional preview dropped when `maxHeight` set and `requiredRows + previewRows + gap > maxHeight`; `contentBudget = maxHeight - surroundingRows - gapRows`; calls `fitContent(budget)`; content rendered at full width then sliced to budget; slots joined with `gap` blank rows; result sliced to maxHeight; cached by reference.

## 21. `form.ts` — FormField / TextFormField / SelectFormField / SettingsFormField / Form
- `FormFieldTheme {label, description, error, hint}`; `FormControl = Component & Partial<Focusable> & {handleInput, pasteText?}`.
- `FormFieldOptions {theme, label?, description?, details?, previewLabel?, preview?, hint?, summary?, footer?, leadingSpace?, spaceBeforeControl?, spaceAfterControl?}`.
- `FormField`: builds before-control (optional Spacer(1), label, Spacer, description, details, optional preview+label, Spacer unless `spaceBeforeControl===false`) and after-control (Spacer unless `spaceAfterControl===false`, error, summary, hint, optional Spacer+footer). `render` concatenates and memoizes by child refs; exposes `controlLineAt`, `routeMouse`, `setError` (single-line, tabs→spaces, newlines→space, trim, truncate 100), `focused` setter propagates to control, `setUseTerminalCursor`, `hasError`, `debugChildren`, `setIgnoreTight`, `invalidate`, `dispose`. Native: fixed `col` role `omp.form.field`, gap `sm`.
- `TextFormFieldOptions`: `initialValue?`, `secret?`, `prompt?`, `empty?: "submit"|"cancel"|"reject"`, `emptyError?`, `validate?`, `onSubmit`, `onCancel`, `requestRender?`. `submit()`: empty trim → cancel/reject(error)/continue; `validate` error shown; sync/async `onSubmit` errors shown; success clears error.
- `SelectFormFieldOptions`: `items`, `currentValue?`, `maxVisible?=10`, `selectTheme`, `getPreview?`, `onSelectionChange?`, `onSubmit`, `onCancel`, `requestRender?`, `picker?`. Builds `SelectList` (maxVisible = min(items.length||1, maxVisible??10)), selects `currentValue`; preview `StyledText`; `onSelect` → onSubmit(value) with async error handling; `onSelectionChange` → async preview refresh with request-id guard; `itemCount > 12` and `cx.supports("picker")` → describes a `SelectListSheet`; `handleNativeEvent` → sheet.
- `SettingsFormFieldOptions`: wraps `SettingsList` in `FormField` sharing label/description/hint/focus/input.
- `FormOptions {fields, onCancel, isCancel?}`; `Form`: focus traversal by Tab/Shift+Tab (wraps), cancel via `isCancel` or `tui.select.cancel`; `routeMouse` selects field on left click and forwards local row; `render` concatenates fields and records bounds; native `col` role `omp.form`, gap `md`.

## 22. `menu-selection.ts` — MenuSelection
- `MenuSelectionAdapter<T> {getKey, getSearchText, isDisabled?, requiresConfirmation?, filter?}`; `MenuActivation = empty|disabled|pending|confirmed`; `MenuWindow {startIndex,endIndex,rowOffset,totalRows}`; `getMenuWindow(rowCounts, selectedIndex, rowBudget, {alignment?, currentStartIndex?})`.
- `getMenuWindow`: center mode targets `floor((budget - min(selectedRows,budget))/2)` rows before selection, expands forward then backward; nearest mode keeps `currentStartIndex` and shrinks forward until selection fits; `rowOffset` = rows before start.
- `MenuSelection`: `setItems(items, selectedKey=this.selectedKey)` (retains key, clears pending), `setQuery(query, retainSelection=true)`, `setSelectedIndex(index, direction=1)` (coerces disabled), `setSelectedKey`, `move(delta, wrap=false)` (skips disabled), `moveToBoundary`, `cancelConfirmation`, `requestActivation` (empty/disabled/pending/confirmed), `isDisabled`, `isPending`, getters `items`, `visibleItems`, `query`, `selectedIndex`, `selectedItem`, `selectedKey`, `pendingKey`. Default filter = `fuzzyFilter` over `getSearchText`.

## 23. `transcript-browser.ts` — TranscriptBrowser
- `TranscriptBrowserBody {lines, anchor?: ScrollRangeAnchor}`; `TranscriptBrowserFrame {header, body, footer}`; `TranscriptBrowserRenderContext {frameWidth, contentWidth = max(1,frameWidth-1), chromeWidth = max(1,frameWidth-2)}`; `TranscriptBrowserOptions {getHeight, frame, minimumBodyRows?=3, followBottom?}`.
- Ctor builds `ScrollView([], {height:0, scrollbar:"auto", followTail, theme track=dim, thumb=accent})` and `DynamicBorder`.
- API: `invalidate`, `renderOutlineRows`, `prepareOutline`, `composeOutline`, `scroll`, `handleScrollKey`, `scrollToTop/Bottom`, `atTop`, `visibleEnd`, `hasAnchored`, `frameRowOffset`, `toContentPoint`, `render`.
- `render(width)`: `contentWidth = frameWidth-1`, `chromeWidth = frameWidth-2`; `borderRows = frameWidth>0?3:0`; `bodyHeight = max(minimumBodyRows, height - header - footer - borderRows)`; `bodyTop = header.length + (frameWidth>0?2:0)`; sets ScrollView lines/height; `revealRange` success disables follow; output = border + header lines (inset by one space, truncated to width-1) + border + ScrollView.render(frameWidth) + footer + border; cached by item equality.
- `nearestVisibleOutlineTarget(visible, selected)`: selected if visible, else nearest above, then below, else first visible, else selected.

## 24. `image.ts` — Image
- `ImageTheme {fallbackColor}`; `ImageOptions {maxWidthCells?, maxHeightCells?, filename?, sourcePath?, budget?: ImageBudget, imageKey?, requestRender?}`.
- `ImageBudget(cap = DEFAULT_MAX_INLINE_IMAGES=8, requestRender = ()=>{})`: per-surface (`screen`/`alt`) splits with `onTerminal`, `planned`, `lastTotal`, `slots`; `cap<=0` disables budgeting; `beginPass`, `observe`, `endPass`, `release`, `forgetTransmitted`, `limitResidentImages`, `pendingTransmits` **[unverified — only head read]**.
- Module helpers: `renderForScrollback` (synchronous SIXEL encode when rows are committed to scrollback), `measureRender` (budgeted images claim no display slot; returns `{value, images}`), `imageMaxColumns(width) = max(1, width-2)`, `RESERVED_IMAGE_ROW = "\x1b[0m"`.
- `Image` ctor `(base64Data, mimeType, theme, options={})`; `#renderedGraphicRows` keeps fallback height stable; `#fallbackLines()` returns `[fallback]` or `renderedGraphicRows-1` reserved rows + fallback. **`render` body [unverified — only tail read]**: uses `imageFallback`, `linkedImageText`, `transcriptImageLink`, budget observation, protocol selection (kitty/sixel/placeholders), and cached geometry.

## 25. `loader.ts` / `cancellable-loader.ts`
- `Loader extends Text`; **`[unverified]`** ctor/theme fields not re-read. Known: `start()` syncs spinner tick/frame, `#syncText()` sets `"<frame> <message>"`, schedules ticks via `spinnerTickDelay(#tickIntervalMs())`; shimmer (`messageColorFn.animated===true`) → 30 Hz (`RENDER_INTE…`); retry variant repaints countdown via `#startNativeCountdown` (`RETRY_METER_REPAINT_MS`); native rendering delegates spinner/timer to TSP. `CancellableLoader` **[unverified]**.

## 26. `spacer.ts`
- `spaceForRows(rows)`: ≤0 `"none"`, ≤1 `"sm"`, ≤2 `"md"`, else `"lg"`.
- `Spacer(lines=1)`: `setLines`, `debugState`, `invalidate`, `describe` (`spacer` size), `render` returns cached `lines` empty strings.

## 27. `layout/geometry.ts`
- Types: `LayoutAlignment = "start"|"center"|"end"`; `LayoutInsets {top?,right?,bottom?,left?}`; `NormalizedLayoutInsets`; `LayoutRect {row,col,width,height}`; `LayoutRenderer = (width,height?)=>readonly string[]`; `LayoutContent = Component | LayoutRenderer`; `LayoutDecoration = string | (()=>string)`; `HeightConstrainedComponent`; `LayoutAllocation {fixed?,grow?,min?,max?}`.
- `layoutSize(v,fallback=0)`: finite ? `max(0,floor(v))` : fallback/0. `optionalLayoutSize`, `layoutRatio(v,fallback)`.
- `allocateLayoutSpace(items, available)`: initial sizes = fixed clamped to `[min,max]` else min; if over budget, shrink from the last child backward; else distribute remaining to `fixed===undefined && grow>0` proportionally by `floor(remaining*grow/totalWeight)`, min 1, capped by `max`, repeating until no grant; `allocationMaximum` = `max ?? MAX_SAFE_INTEGER` floored.
- `layoutInsets`, `equalLayoutInsets`, `layoutAlignmentOffset(space,align)` (start 0, end space, center `floor(space/2)`), `isLayoutComponent`, `isHeightConstrainedComponent`, `isLayoutMouseRoutable`, `uniqueLayoutComponents` (first occurrence order), `fitLayoutLine(line,width)` (truncate + space-pad), `renderLayoutContent` (function → `content(size(w), optionalSize(h))`; component with `setHeight` → `setHeight(optionalSize(h))`; then `render(size(w))`).
- `nativeLayoutChild` (lazy renderer → stable adapter component), `nativeLayoutGap` (decoration length 0→undefined, 1→"xs", 2→"sm", else "md"), `nativeSlotProps(axis, slot)` (fixed → `grow:0,shrink:0,min,max`; else `grow`, `basis:"content"`, min/max; min 0 → undefined).

## 28. `layout/stack.ts`, `layout/row.ts`, `layout/split-pane.ts`
- **Stack** **[unverified — not re-read]**: vertical layout; uses geometry allocation/insets/alignment; `setChildren`, decorations, `render`.
- **Row** (`row.ts:300` tail verified): `render(width)` lays out children at allocated widths; per output line, each child's visible rows are offset by `layoutAlignmentOffset(viewportHeight - visibleHeight, child.align ?? this.#align)`; content = `padding(inset.left) + fitLayoutLine(content, contentWidth) + padding(inset.right)`; gap between children; suffix right-aligned; row truncated to width; memo on width/height/prefix/gap/suffix/widths/child lines.
- **SplitPane** (`split-pane.ts:300` tail verified): describes a `row` of left `col` (fixed or grow/min/max with `basis: layoutRatio(size.ratio,0.5)`) and right `col` (`grow:1,min:rightMinWidth`), gap from divider, optional narrow pane; `render` delegates to `#row`, records `#lastGeometry` `{mode:"split",left,right,dividerCol}` or `{mode:"narrow",left|right}`; narrow mode swaps row child. **[unverified: ctor fields/defaults]**.

## 29. `composer/*`
- `types.ts`: `ComposerBox = SymbolTheme["boxRound"]`; `BUILTIN_EDITOR_BORDER_STYLES = ["box","band","claude","pi","borderless","rule","field","rail"]`; `EditorTopBorder {content,width,revision?}`; `ComposerChromeContext {width,paddingX,borderColor,accentColor,surfaceColor,box,topBorder?}`; `ComposerRowContext extends … {text,pad,gutter,isLastRow,cursorOverflow,imeSafeCursorTail,scrollbarThumb}`; `ComposerStyle {id, filledSurface?, sideBorders, verticalChrome: 0|1|2, statusAttachment: "top-border"|"top-band"|"top-rule-chip"|"none", bottomBar: "none"|"left"|"full", bottomBarGap, defaultPromptGutter?, defaultPaddingX(themePaddingX), sideChromeWidth(paddingX), renderTop(ctx), renderRow(ctx): string[], renderBottom(ctx)}`.
- `registry.ts`: built-in map of the 8 styles; `isBuiltinComposerStyle`, `isFilledComposerStyle` (default `filledSurface ?? !builtin`), `registerComposerStyle` (rejects empty/whitespace-padded/duplicate/builtin ids; returns disposer), `getComposerStyle(id)` falls back to `box`.
- `rail.ts`: `railComposerStyle` — `id:"rail"`, `filledSurface:true`, `sideBorders:true`, `verticalChrome:0`, `statusAttachment:"none"`, `bottomBar:"full"`, `bottomBarGap:true`, `defaultPaddingX()=1`, `sideChromeWidth(px)=px+1`, `renderTop/renderBottom → undefined`; `renderRow` = `accentColor("▎") + surfaceColor(paddingX + gutter + text + pad + padding(rightFillCells))`, with IME-safe tail and scrollbar thumb (`accentColor("█")`) variants.
- `band.ts`, `borderless.ts`, `box.ts`, `claude.ts`, `field.ts`, `pi.ts`, `rule.ts` **[unverified — not re-read]**: each exports a `ComposerStyle` with the same shape; `index.ts` re-exports all.

## 30. `symbols.ts`
- `BoxSymbols {topLeft,topRight,bottomLeft,bottomRight,horizontal,vertical,teeDown,teeUp,teeLeft,teeRight,cross}`.
- `SymbolTheme {cursor, inputCursor, boxRound: Omit<BoxSymbols,"teeDown"|"teeUp"|"teeLeft"|"teeRight"|"cross">, boxSharp: BoxSymbols, table: BoxSymbols, quoteBorder, hrChar, colorSwatch?, spinnerFrames: string[]}`. Default theme via `getSymbolTheme()` **[unverified]**.

## 31. Helpers
- `spinner-clock.ts`, `motion-effects.ts`, `kill-ring.ts`, `space-hold.ts`, `editor-component.ts` **[unverified this session — not re-read under budget]**. Known from call sites: `sharedSpinnerTick`, `sharedSpinnerFrame`, `spinnerTickDelay`, `RENDER_INTE…` (30 Hz shimmer), `RETRY_METER_REPAINT_MS`; `KillRing.push(text,{prepend,accumulate})`, `peek`, `rotate`, `length`; `SpaceHoldGesture` with `shouldRoute`, `process(canonical,len) → "type"|"swallow"|…`, `recordTyped`, `getSpaceHoldText(data,canonical)`; `editor-component.ts` is the host-side wrapper around `Editor` (focus/them

---

**Gaps to close before implementation** (in priority order): `editor.ts` full `#handleInputChunk` binding table + `#jumpToChar`; `markdown.ts` `MarkdownTheme` field list; `composer/{band,borderless,box,claude,field,pi,rule}.ts`; `stack.ts`/`row.ts`/`split-pane.ts` ctor+fields; `loader.ts`/`cancellable-loader.ts`; `scroll-view.ts` ctor/fields + `scroll-viewport.ts` bodies; `truncated-text.ts` ctor tail; `image.ts` render; `spinner-clock.ts`/`motion-effects.ts`/`kill-ring.ts`/`space-hold.ts`/`editor-component.ts`; `symbols.ts` `getSymbolTheme` defaults.