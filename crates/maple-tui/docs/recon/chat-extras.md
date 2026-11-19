## Summary

I read the full `l/src/chat/` surface (message components, builder, browser/outline, presentation, tool card/block/status-line primitives, rewind selector) plus `fuzzy.ts`, `utils.ts`, `lang-from-path.ts`, `glyph-protocol.ts`, `kitty-graphics.ts`, `sixel.ts`, `terminal-multiplexer.ts`, `tmux.ts`, `desktop-notify.ts`, `hotkeys-markdown.ts`, `key-hint-format.ts`, and the first 260 lines of `terminal-capabilities.ts`.

**Coverage gap you must close before implementing:** I did **not** capture `autocomplete.ts` provider internals, `latex-block.ts` / `latex-to-unicode.ts`, `vim.ts`, the terminal-capability **query/detection flow** (rest of `terminal-capabilities.ts` + `terminal.ts`/`active-terminal.ts`), the `multiplexers/*` modules, the native sixel encoder inputs, or the remaining chat renderers (`read-tool-group.ts` internals, `bash-execution.ts`, `eval-execution.ts`, `spotlight.ts`, `svg-*`, `table-chart.ts`, `image-loading.ts`, `late-diagnostics-message.ts`, `advisor-message.ts`, `compaction-summary-message.ts`, `skill-message.ts`, `reply-write.ts`, `served-model-marker.ts`, `cache-invalidation-marker.ts`, `todo-reminder.ts`, `extension-types.ts`, `transcript-render-helpers.ts`, `execution-shared.ts`, `fence-figure.ts`, `ttsr-notification.ts`, `eval-execution.ts`). Their exported API is noted where I captured it; the LaTeX symbol maps and vim subset in the acceptance criteria are therefore **not** in this report.

---

## 1. Chat message components

### 1.1 Persisted entry model — `l/src/chat/transcript-entry.ts`

```ts
SessionMessageEntryLike { type:"message"; id; parentId:string|null; timestamp:string; message:AgentMessage }
CustomMessageEntryLike { type:"custom_message"; id; parentId; timestamp; customType; content:CustomMessageContent; details?; display:boolean; attribution? }
```

- `transcriptEntryMessage(entry)` → `entry.message` for `message`; for `custom_message`, `customMessageEntryMessage` first requires `isCustomMessageContent(content)`, then `normalizeCustomMessagePayload`, then `createCustomMessage(customType, content, display, details, timestamp, attribution)` (lines 34-46).
- `isUserRequestEntry`: `role==="user"`, or `role==="custom" && isUserTurnInitiator(message)` (48-58).
- `recentTranscriptEntries(entries, limit=600)`: if `entries.length <= limit` return all; else scan backwards from `entries.length-limit` to 1 for a user-request entry and slice from it; otherwise return the whole array (64-72). Comment: builder drops tool results whose call was sliced away.
- `userTurnDraft`: user → `textContent(content)`; custom initiator → `titleTextFromSkillPrompt(message) ?? textContent(content)` (74-81).
- `textContent(content, separator="")`: string → itself; else concatenate `text` blocks with `separator` between blocks (83-92).
- `imageContent`: keep `type==="image"` with string `data` and string `mimeType` (94-99).
- `userMessageLabel`: `textContent(content," ").replace(/\s+/g," ").trim()` (101-103).

### 1.2 Display preferences — `l/src/chat/display-preferences.ts`

```ts
interface ChatTranscriptDisplayPreferences {
  hideToolActivity:boolean; shellVisualization:boolean; readToolResultPreview:boolean;
  showImages:boolean; cacheMissMarker:boolean; showTokenUsage:boolean; showTurnTime:boolean;
  advisorCollapsedLines:number;
}
```
Defaults: `false, true, false, true, false, false, false, 0`. `setChatTranscriptDisplayPreferences(Partial)` mutates the singleton. Consumed by the builder (`readToolResultPreview`, `showImages`, `cacheMissMarker`, `showTokenUsage`, `showTurnTime`, `hideToolActivity`).

### 1.3 Builder — `l/src/chat/chat-transcript-builder.ts`

State: `#pendingTools: Map<callId,ToolExecutionHandle>`, `#readArgs: Map<callId,args>`, `#readGroup: ReadToolGroupComponent|null`, pending usage fields, `#turnStartedAt`, `#turnUsage`, `#lastAssistantUsage`, `#servedModelTracker`, `#waitingPoll`, `#todoSnapshot`, `#expandables`, `#expanded`, `#entryComponents: Map<entryId, Component[]>`.

- `rebuild(entries)`: `reset()` then append each; at end, if `#readArgs.size===0 && #pendingTools.size===0` → `#flushPendingUsage()` (129-138).
- `append(entries)`: append each; same trailing flush condition (139-148).
- `setExpanded(expanded)`: stores and calls `setExpanded` on every tracked expandable (150-158).
- `rowForEntry(entryId)`: first `container.getChildStartRow(component)` for the entry's components (160-167).
- `reset()`: seals pending tools, clears all state, disposes container (169-191).
- `#appendEntry`: `transcriptEntryMessage`; records `container.children.slice(before)` under `entry.id` (196-203).
- `#trackExpandable`: applies current `#expanded` then pushes (205-209).
- `#mountProse(component)`: `setImagesVisible(displayPreferences.showImages)`, `setToolResultImagesVisible(!hideToolActivity)`, `setTableChartsVisible(deps.tableCharts !== false)`, track, add (211-218).
- `#resolveWaitingPoll(nextToolName?)`: removes a displaceable `wait` block if next tool is `wait` and `container.canDisplaceBlock(previous)`, always seals (220-228).
- `#resolveTodoSnapshot(nextToolName?)`: if not displaceable → drop; if `canBeDisplacedBy(nextToolName)` → remove+seal; if `nextToolName===undefined` → seal (230-246).
- `#ensureReadGroup()`: lazily creates `ReadToolGroupComponent({showContentPreview: displayPreferences.readToolResultPreview, transcriptPresenters:false})`, tracks, adds (248-262).
- `#flushPendingUsage()`: tries `#readGroup.attachUsage(callIds, usage, duration, ttft, timestamp, elapsedMs)`; if not attached, seals/drops read group and adds `createUsageRowBlock(...)`; clears pending fields (264-296).
- `#appendChatMessage` (298-423):
  - `role !== "toolResult"` → flush pending usage; `role !== assistant && !== toolResult` → seal+null read group.
  - `assistant` → `#appendAssistantMessage`; `toolResult` → `#appendToolResult`.
  - `user`/`developer`: a `user` message with `attribution === "agent"` does **not** anchor the turn; a `developer` message with `synthetic` resets `#turnUsage`, and sets `#turnStartedAt = userInitiated ? timestamp : undefined`. A `user` message resolves waiting poll and todo snapshot. `parentIrc` → `buildParentIrcMessageCard`. Non-empty `userText` → synthetic ? `CollapsedSyntheticMessageComponent(userText)` (tracked) : `UserMessageComponent(userText, {liveSteered, timestamp, images: imageContent(content)})`.
  - `bashExecution` → `BashExecutionComponent(command, ui, excludeFromContext)` + `appendOutput` + `setComplete(exitCode, cancelled, {truncation, artifactError, minimized, images, showImages})`.
  - `pythonExecution` → `EvalExecutionComponent(code, ui, excludeFromContext)` + output + `setComplete(exitCode, cancelled, {truncation, artifactError})`.
  - `hookMessage`/`custom` → if custom and `isUserTurnInitiator` set `#turnStartedAt`/reset usage, then `#appendCustomMessage`.
  - `compactionSummary`, `branchSummary` → dedicated tracked components; `fileMention` → `buildFileMentionBlock(files, 1)`.
- `#appendAssistantMessage` (427-575): flags `hideThinkingBlock`, `proseOnlyThinking` (default true), `expandThinkingBlocks`; `splitAssistantMessageToolTimeline(message)`; builds `AssistantMessageComponent(timeline.beforeTools, hideThinkingBlock, requestRender, [], ui.imageBudget, proseOnlyThinking, linkTargets, expandThinkingBlocks, {renderers: textRenderers ?? [], contentOffset:0})`; `pickReactionTarget(container.children)`; mount; cache-miss marker via `detectCacheInvalidation(#lastAssistantUsage, usage)` when enabled; updates `#lastAssistantUsage` if `cacheRead+cacheWrite+input > 0`; `setServedModelMismatch(tracker.check(message))`; if `assistantHasVisibleContent(message)` seal+null read group; error presentation (`resolveAssistantErrorPresentation`, full ⇒ `errorMessage`); for each `toolCall`: `getTool` → `toolRenderName`, `#resolveWaitingPoll(renderToolName)`, `afterToolSegment = timeline.afterToolCalls.get(id)`; read-collapse branch (`renderToolName==="read" && readArgsCollapseIntoGroup(args)`) handles error/group/pending args; else seal read group, `builtIn = isBuiltInTool?.(name) ?? true`, `isReplyWrite(...)===true` → `ReplyWriteComponent` (mount as prose), else `ToolExecutionComponent(name, args, {useBuiltInRenderer:builtIn, showImages: displayPreferences.showImages, transcriptPresenters:false}, tool, ui, cwd, id)` tracked+added; error stop or `#pendingTools.set(id, component)`; `appendAssistantSegment(id, afterToolSegment)` creates a second `AssistantMessageComponent(segment, …, {renderers, contentOffset: timeline.contentOffsets.get(id) ?? 0})`; finally `#turnUsage.add(message, #turnStartedAt)` → `lastAssistantComponent.setTurnUsage`, and sets pending usage when `showTokenUsage && assistantUsageIsBilled(usage)`, with `groupedReadUsageCallIds(message)` and elapsed only if `showTurnTime`.
- `#appendToolResult` (577-625): resolve canonical name; read-group result if `name==="read" && (!pending || pending instanceof ReadToolGroupComponent)` — creates group if needed, applies `#readArgs`, `updateResult(message,false,id)`, deletes pending/args; else mount `toolResultReplies(message.details)` as `AssistantMessageComponent(replyDisplayMessage(reply), false, …, true, …)`; `pending.updateResult`; `wait` → `#waitingPoll`; `todo` → `#resolveTodoSnapshot("todo")` then `#todoSnapshot = pending`.
- `#appendCustomMessage` (627-760): skips `!display`; `async-result` → `buildAsyncResultComponent`; `lsp-late-diagnostic` → `routeLateDiagnostics` then `LateDiagnosticsMessageComponent`; `collab-prompt` → `CollabPromptMessageComponent`; `skill-prompt` → `SkillMessageComponent`; `irc:incoming|autoreply|relay|workpool` → `buildIrcMessageCard`; `advisor` → `createAdvisorMessageCard`; `launch-completion` → `buildLaunchCompletionBlock`; `background-tan-dispatch` → `createBackgroundTanDispatchBlock`; handoff → `createHandoffSummaryMessageComponent`; fallback `CustomMessageComponent(message, getMessageRenderer?.(customType))`.

### 1.4 User bubble — `l/src/chat/user-message.ts`

- `setPromptZonesEnabled(enabled)` (68); `interface UserBubbleOptions` (73); `userBubbleColor(...)` (95).
- `class UserMessageComponent extends Container implements ReactionTarget, TranscriptPresentationTarget` (141); `constructor(text, options)` (166); `setReaction(emoji)` (203); `describe` (219) builds `card({role: synthetic? "omp.user.synthetic":"omp.user", tone: synthetic? "muted":"user", actions.hold: turnHoldEntries("user", {id:"copy-message",label:"Copy message",icon:"copy"})}, children)`; `handleNativeEvent` (318): `copy-message` → `runTranscriptAction({act:"copy",text})`, `rewind` → `{act:"rewind"}`, other → `runTurnAction(act,{role:"user",at:timestamp})`.
- `#badgeRow(width)` (333): `applyBackgroundToLine(\` ${marker}${padding(gap)}${emoji}\`, width, bgColor)` where `marker = liveSteered ? theme.fg("accent","*") : ""`, `emoji = reaction ?? ""`, `gap = max(0, width-2-visibleWidth(marker)-visibleWidth(emoji))`.
- `setTranscriptAllocation`, `isTranscriptBlockFinalized(): true`, `renderTranscriptBlockEmergencyRow` (337-366): if `lines.length>=3`, first non-blank interior line (`1..len-2`), else `lines[1]`, else `lines[0]`.
- `render(width)` (368-403): `super.render(width)`; if `#allocation===1` return `[emergency]`; if reaction/liveSteered replace `wrapped[0]` with `#badgeRow`; if prompt zones enabled prefix `OSC133_ZONE_START` on first and append `OSC133_ZONE_CLOSE` on last; caches by source array identity + `promptZonesEnabled`.
- Body: `Markdown(text, 1, 1, getMarkdownTheme(), {bgColor: v => theme.bg("userMessageBg", v), color: v => theme.fgOnBg("userMessageText","userMessageBg",v)})`; Container yields Markdown rows with paddingY=1 (blank top/bottom rows carrying the background).
- `tokenMarks` (405-427): maps composer tokens to spans: skill → `customMessageLabel strong`, mention → `statusLineModel strong`, reference → `accent strong` (+ file link for image/video).
- `SyntheticSummary` (428-466): always-visible dim summary row, width-truncated label, heavy body in disclosure detail.
- `CollapsedSyntheticMessageComponent` (467-…): `constructor(text, imageLinks?)`, `setExpanded`, `describe`, `setIgnoreTight`, `render`.

### 1.5 Assistant — `l/src/chat/assistant-message.ts`

`constructor(message?, hideThinkingBlock=false, onImageUpdate?, thinkingRenderers=[], imageBudget?, proseOnlyThinking=true, linkTargets?, expandThinkingBlocks=false, textRendering?)` (618-658). Children: `#contentContainer = new Container()` then `#markerSlot = new Container()`; `updateContent(message)` if present. `#createMarkdown` (1507-1511): prose kind → `new Markdown(text, 1, 0, this.#getProseTheme(), this.#getProseTextStyle(), 0)`; otherwise `new Markdown(text, 1, 0, getMarkdownTheme(), {…})`. Thinking uses `Markdown(..., prose theme)` at 2115. Spacers of 1 row separate blocks (2123, 2135, 2148). `#refreshMarkers` order: served-model marker then cache-invalidation marker. No background/padding is applied by the assistant component itself (Markdown paddingX=1, paddingY=0).

### 1.6 Thinking — `l/src/chat/thinking-display.ts`

- `canonicalizeMessage(text)`: trim; return `""` if every char is `.`, `…`, space, tab, LF, CR; else trimmed text.
- `formatThinkingForDisplay(text, proseOnly)`: drops empty HTML comment lines (`<!-- -->`, and unterminated `<!--` on the last line while streaming) outside fences; prose mode additionally elides fenced code to a trailing ellipsis. Fold cache: `DISPLAY_CACHE_SLOTS=4` MRU per mode; `MAX_RESUME_PARTIAL_BYTES=8192`; append detected by exact `startsWith`; last input line always re-folded. `resetThinkingDisplayCacheForTests()`.
- Fence handling uses `FENCE_RE` from `render/render-utils`.

### 1.7 Tool card — `l/src/chat/tool-execution.ts` + `render/*`

- State tint (1814-1830): `stateBgKey = #isPartial || benignSkip ? "toolPendingBg" : result?.isError ? "toolErrorBg" : "toolSuccessBg"`; `stateBgFn = t => theme.bgFill(stateBgKey,t)`. `#contentText = new WidthAwareText(w => #renderDefaultCard(w), 1, 1)` (604).
- `#getTextOutput` (2270-2301): text blocks joined by `\n` after `sanitizeWithOptionalSixelPassthrough(c.text, sanitizeText)`; if image blocks and no protocol/showImages, append `imageFallback(mime,dims)` indicators joined by `\n` (linked when data present).
- `#renderDefaultCard` → `formatDefaultToolExecution({label, args, result:{output,isError,skipped}, options}, width, theme)`.
- `#isBenignSkip` (2358-2374): details `source==="interrupt_skipped"` and (`__synthetic===true` or (`__interrupted===true && execution==="started"`)).
- `#renderBenignSkipCard` (2375-…): neutral `toolPendingBg`, paddingX=1, default card.
- `formatDefaultToolExecution` — `l/src/tools/default-renderer.ts:153`: `[renderStatusLine(status), ...buildDefaultToolBody(...)].join("\n")`.
  - `buildDefaultToolStatus` (41-72): icon `running` if `isPartial && spinnerFrame!==undefined`, `pending` if `isPartial`, `info` if `result.skipped` (titleColor muted), `error` if isError, else `success`; phase `running|partial|info|error|success`.
  - `buildDefaultToolBody` (74-151): collapsed args inline when `!expanded && args && keys>0`: budget `max(20, contentWidth - width(theme.tree.last) - 2)`, line ` ${dim(tree.last)} ${dim(preview)}`. Expanded args: blank, `Args` dim, `renderJsonTreeLines(args, expanded caps)`, `…` if truncated, blank. Result: `output.trimEnd()`; empty ⇒ dim `(no output)`; `isTreeSafeJson` ⇒ tree with collapsed/expanded caps and `formatExpandHint(...,true)` when collapsed; expanded+truncated falls through to raw; otherwise `formatOutputPaneLines({lines, expanded, collapsedMaxLines:4, styleLine: clipCollapsed(styleToolOutputLine(line,theme), contentWidth, expanded), showExpandHintWhenUncapped:true})`.
  - `JSON_TREE_MAX_CHARS = 64*1024`; `UNSAFE_INTEGER_RE = /(?<![\w.])-?\d{16,}(?![\w.])/`; `isTreeSafeJson` requires `{`/`[` start, length ≤ cap, no unsafe integer. Caps `JSON_TREE_MAX_DEPTH/LINES/SCALAR_LEN_{COLLAPSED,EXPANDED}` come from `tools/json-tree`.
  - `describeDefaultToolExecution`: summary via `formatArgsInline(args, 160, {characterBudget:true})`; tone `skipped→"info"`, `isError→"error"`; empty ⇒ `noteText("(no output)")`; safe JSON ⇒ `describeJsonValue` (falls back to raw `ansi(output)` if truncated); else `ansi(output)`.
- `renderStatusLine` — `render/status-line.ts:33`: `icon` (`formatStatusIcon` or `iconOverride`) + ` ` + title (`theme.fg(titleColor ?? "accent")`); `: ` + muted description; badge wrapped in `theme.format.bracketLeft/Right` with its color; meta joined by `theme.sep.dot` in dim. `flattenForHeader` replaces `\r\n?|\n` with space.
- `formatStatusIcon` — `render/render-utils.ts:293`: success/done → `theme.styledSymbol("status.success","success")`; error → `status.error`/`error`; warning → `status.warning`/`warning`; info → `status.info`/`accent`; pending → `status.pending`/`muted`; running → `theme.spinnerFrames[frame % len]` when frame given, else `status.running`/`accent`; aborted → `status.aborted`/`error`.
- `ToolCard` — `render/tool-card.ts:137`: `variant: "framed"|"plain"`; framed cards are marked `markFramedBlockComponent`. `render` computes `defaultContentWidth = framed ? outputBlockContentWidth(width) : max(1,width-plainPadding*2)`; resolves body/sections/footer; framed ⇒ `CachedOutputBlock.render(OutputBlockOptions, theme)`; plain ⇒ header, sections (blank separator, label), footerMeta, joined with `\n`, rendered through a `Text` with `setCustomBgFn(state && applyBg!==false ? t => theme.bg(getStateBgColor(state),t) : undefined)`.
- `renderOutputBlock` — `render/output-block.ts:69`: `h=theme.boxRound.horizontal`, `v=vertical`, `cap=h.repeat(3)`, `lineWidth=max(0,width)`; border color = `options.borderColor ?? (error→"error", warning→"warning", running|pending→"accent", else "dim")`; bg = `theme.getBgAnsi(getStateBgColor(state))` with `\x1b[0m`/`\x1b[m`/`\x1b[49m` re-stamped after resets, wrapped as `bg + stabilized + \x1b[49m`. `contentPaddingLeft` default 1, right defaults to left; `contentWidth = max(0, lineWidth - width(v) - left - right - width(v))`. Rows: top bar (`topLeft + cap` … `topRight`), labelled section bars use `teeRight`/`teeLeft`, blank divider only between sections; content lines `wrapTextWithAnsi(line.trimEnd(), contentWidth)` and right-padded to contentWidth; sixel lines emitted raw; `footerMeta` fits when `visibleWidth(" "+meta+" ") <= lineWidth - width(bottomLeft+cap) - width(bottomRight)` else wrapped into body rows; bottom bar. `renderBar`: label `" "+label+" "` truncated to `maxLabelWidth`, fill with `h.repeat(fillCount)`; no label ⇒ continuous bar. `clipFrame = lineWidth < max(width(cap)+2, left+right+2)` ⇒ `truncateToWidth(line,lineWidth,Ellipsis.Omit)`. Final lines `padToWidth(..., lineWidth, bgFn)`.
- Theme colors (`theme/dark.json`): `toolPendingBg #1d2129`, `toolSuccessBg #161a1f`, `toolErrorBg #291d1d`; `userMessageBg` aliases `userMsgBg`. Light: `#e8e8f0`, `#e8f0e8`, `#f0e8e8`. `boxRound` default `╭ ╮ ╰ ╯ ─ │` (`theme/symbols.ts:436-439`).

### 1.8 Custom / hook / collab — `chrome/message-frame.ts`, `chat/custom-message.ts`, `hook-message.ts`, `collab-prompt-message.ts`

- `FramedMessageComponent`: `Box(1,1, t => theme.bg("customMessageBg",t))`, `setIgnoreTight(true)`; border `chars: theme.boxRound`, `color: t => theme.fg(borderColor ?? "borderMuted", t)`; header (unless `hideHeader`) = `theme.fg("customMessageLabel", theme.bold(`${icon} ${customType}`))` + `Spacer(1)`; body `Markdown(text, 0, 0, getMarkdownTheme(), {color: v => theme.fg("customMessageText", v)})`; `collapseAfterLines` truncates to N lines + `…` when collapsed. Native: `card({role, tone: colorTone(borderColor), head: hideHeader?undefined:[span(tag,"customMessageLabel strong")], collapsible, collapsed, preview:{lines:N}}, [md(text)])`.
- `CustomMessageComponent`: role `omp.custom` (`omp.hook` for hookMessage, `omp.custom.delegation` for live delegation), icon `theme.icon.extensionHook` for hooks else `theme.icon.package`, `hideHeader` for delegation, `borderColor:"borderAccent"` for delegation.
- `HookMessageComponent`: role `omp.hook`, `collapseAfterLines=5`.
- `CollabPromptMessageComponent`: author line `theme.fg("accent", "\x1b[1m«from»\x1b[22m ›")` with padding (1,0) and `setIgnoreTight(true)`; body `Markdown(text,1,1,getMarkdownTheme(), {bgColor: userMessageBg, color: userMessageText})`; native `card({role:"omp.user.collab", tone:"user", head:[span(\`«from» ›\`,"accent strong")]}, [md(text)])`.

### 1.9 Reaction — `chat/reaction.ts`

`ReactionTarget { setReaction(emoji) }`; `isReactionTarget`. Constants: `MAX_REACTION_UNITS=32`; `REACTION_HEAD_RE=/^(?:\p{RGI_Emoji}|\p{Extended_Pictographic}\uFE0F?)/v`; `EMOJI_EXTENSION_RE=/^(?:[\p{Emoji_Modifier}\u{E0020}-\u{E007F}]|\uFE0F|\u200D|\u20E3)/u`; `REACTION_PREFIX_RE=/^(?:[\p{Extended_Pictographic}\p{Emoji_Modifier}\p{Regional_Indicator}\u{E0020}-\u{E007F}]|\uFE0F|\u200D|\u20E3)*[ \t]*$/u`; `NEXT_WHITESPACE_RE=/^[ \t]*\r?\n|^[ \t]+/`. `splitReaction(text)`: skip leading whitespace; no head ⇒ `{body:text,pending:true}`; RGI/pictograph match and (head ≤32 units and rest is an emoji extension) ⇒ pending; else consume one following whitespace run and return `{emoji, body, pending:false}`; no match ⇒ `{body:text, pending: head.length<=32 && REACTION_PREFIX_RE.test(head)}`.

### 1.10 Other chat components

- `recap-notice.ts`: `Spacer(1)` + `Text(\`※ recap: ${recap}\`,1,0)` styled `theme.fg("dim", theme.italic(line))`; native row role `omp.recap` with `history` icon.
- `stripped-tool-calls-placeholder.ts`: label `${n} tool call(s) elided — no result on this branch`, `Text(...,1,0)` dim italic; native `text([span(label,"dim em")], {wrap:"word", role:"omp.tool.elided", hidden: !visible})`; ANSI renders `[]` when hidden.
- `skill-title-input.ts`: `skillPromptTitleInput` precedence `queueChipText` → `prompt` → `/skill:name args` → `/skill:name` → `args` → `""`; `titleTextFromSkillPrompt` only for `role==="custom" && customType==="skill-prompt" && attribution==="user"`.
- `background-tan-message.ts`: `TAN_WORK_PREVIEW_LENGTH=56`; `previewWork` collapses whitespace, truncates to 55 chars + `…`; status block parts `${theme.icon.output} Tangent dispatched` muted, `[task]` dim, jobId accent, `${theme.format.dash} ${work}` dim; native spans `Tangent dispatched · task <id>[ · work]`.

### 1.11 Transcript browser / outline / navigation

- `transcript-browser.ts`: `TranscriptBrowserFrame {header, body:{lines,anchor?}, footer}`; `TranscriptBrowserRenderContext {frameWidth, contentWidth=max(1,frameWidth-1), chromeWidth=max(1,frameWidth-2)}`; `minimumBodyRows` default 3; `followBottom` default false. `render(width)` (164-…): context; `borderRows = frameWidth>0 ? 3 : 0`; `bodyHeight = max(minimumBodyRows, height - (header+footer+borderRows))`; `layout.bodyTop = header.length + (frameWidth>0?2:0)`; sets ScrollView lines/height; on `body.anchor` `revealRange` success ⇒ followBottom=false; output = border, header (`" " + truncateToWidth(line,width-1)`), border, ScrollView rows, border, footer. Getters `atTop`, `visibleEnd`, `hasAnchored(id)`, `frameRowOffset`, `toContentPoint(frameRow,frameCol)`.
- `nearestVisibleOutlineTarget(visible, selected)` (63-73): if selected out of range or `visible[selected] !== false` return it; else first visible above, then below, else selected.
- `prepareOutline` / `composeOutline` (107-137); `renderOutlineRows` uses `OutlineRowCache` at `max(10, columnWidth-4)`.
- `transcript-outline.ts`: `OutlineTarget {entryId, turnId, isUserTurn, start, end, entries}`; `appendOutlineEntries` (90-127): appends one entry, counts usage-row blocks at the head and extends the previous target's `end`; folds `toolResult` into the previous target (updates `entryId`, pushes entry, extends `end`); skips entries rendering nothing; `outlineVisibility` (130-140) true if any row has `\S`; `describedVisibility`/`shows` for native; `stripPromptZones` removes OSC 133 spans; `outlineRule`/`outlineRows` draw dotted rules with `boxDotted.horizontal` and `┆` sides; `composeOutlineColumn` (212-262): `inner=max(10,columnWidth-4)`, unselected rows prefixed `"  "` (blank rows unprefixed), selected target outlined over its non-blank core; `positionRail` (`… ○ ◉ ○ …`, `radio.selected/unselected`); `isUserTurnEntry`, `userTurnLabel`, `userMessageHasText`.
- Rewind selector navigation (`overlays/rewind-selector.ts`): starts on latest target (`#selected = max(0,targets.length-1)`), `#startAt(entryId)` finds target containing the entry and loads full history if truncated. Keys: wheel scrolls ±3 rows; `f` opens filter (loads full history); expand toggle; up/down step selection; left/right move between sibling branch columns when present, else between user turns; `a`/`A` loads full history; Enter rewinds to outlined target; remaining keys go to `ScrollView.handleScrollKey`. Filter: matches every whitespace-separated Latin word as a whole word (case-insensitive), non-Latin scripts substring; empty query matches all; selection stays if still matching else newest match above; up/down step, left/right step user-turns only; Enter rewinds; Esc closes filter, backspace on empty closes. Frame: `#frame(contentWidth)` prepares outline; branch strip uses half-width columns (`max(24, floor((contentWidth-2)/2))`), camera slide `SLIDE_MS=160` eased `1-(1-t)^3`, dot rail when >2 columns; anchor id `rewind:main:<turnId>` / `rewind:sibling:<rootId>:<entryId>` / `rewind:filter:<query>:<turnId|index>`. `ScrollView.handleScrollKey` handles shift+up/down (±5), up/down, pageUp/pageDown, home/end.

---

## 2. Fuzzy matcher — `l/src/fuzzy.ts`

`FuzzyMatch {matches:boolean; score:number}` (lower = better); `FuzzyFilterResult<T> {item, score}`.

**Normalization** (`normalizeForSearch`, 30-37): split camel case (`([A-Z]+)([A-Z][a-z])` → `$1 $2`, then `([a-z0-9])([A-Z])` → `$1 $2`), lowercase, replace runs of non-letter/mark/number with a single space, trim, collapse whitespace.

**Search index** (44-105): `normalized`, `compact` (spaces removed), `compactWordStarts` (cumulative offsets of word starts in compact), `words: {text,index,ordinal}`. Bounded LRU `indexCache` capacity `INDEX_CACHE_MAX=4096`, only texts ≤ `MAX_CACHED_TEXT_LEN=4096`; `buildSearchIndex(text, admit=true)`; `FuzzyText`/`FuzzyCorpus` build uncached indexes directly. `FuzzyCorpus.rank` and `fuzzyRank` return score 0 for all items when query is blank; `fuzzyRank` still calls `getText` per item for pure-punctuation queries.

**Character scoring** (`scoreCharacters`, 121-156): greedy subsequence; consecutive run of length k subtracts `k*5`; gaps add `(i-lastMatch-1)*2`; adds `i*0.1`; span = last-first+1.

**Token/word scoring** (`scoreTokenAgainstWord`, 216-239):
- exact word: `-200`
- word startsWith token: `-170 + (wordLen-tokenLen)*0.5`
- token startsWith word and `tokenLen-wordLen<=2`: `-150 + tokenLen - wordLen`
- substring: `-20 + substringIndex`
- character match: reject if `span > max(tokenLen+2, ceil(tokenLen*1.8))`; else `-40 + charScore`
All add `withPosition(score, word.index) = score + index*0.01`.

**Acronym** (`scoreAcronym`, 241-266): token length 2..4; match first letters in order across words; reject if `wordSpan > tokenLen+2`; score `-30 + wordSpan*4 - tokenLen*2` + position of first word.

**Token direct** (301-322): compact word-start hit `-140` (only if the occurrence starts a word; `findCompactWordStart` rescans past a shadowing non-word occurrence, but only when `needle.length >= MIN_SHADOW_RESCAN_LENGTH=3`), then best of per-word matches and acronym.

**Alphanumeric swap** (`buildAlphanumericSwapQueries`, 158-170): for each adjacent `a-z`/`0-9` pair in either order, swap; `scoreToken` adds `ALPHANUMERIC_SWAP_PENALTY=5`.

**Whole query** (`fuzzyMatchCore`, 355-382): empty query ⇒ `{matches:true,score:0}`; empty index ⇒ no match. `findWordBoundaryPhrase(normalized, normalizedQuery)` ⇒ `-PHRASE_BONUS (1000) + index*0.01`; `findCompactWordStart(index, compactQuery)` ⇒ `-COMPACT_PHRASE_BONUS (1200) + index*0.01`; every token must match (else whole query fails); total = phrase bonuses + token scores. `findWordBoundaryPhrase` (184-214) only rescans for shadowing when the first hit is mid-word and `phrase.length >= 3`.

`fuzzyRank` sorts ascending by score (stable), filters non-matches; `fuzzyFilter` maps to items; `FuzzyQuery`/`FuzzyText` pre-prepare.

---

## 3. Autocomplete — `l/src/autocomplete.ts` (API captured, internals not read)

Exports: `findLeadingSlashCommandStart(text)`, `findTrailingSlashCommandStart(text)`, `extractAtFilePrefix(text)`, `isDirectoryCompletionValue(value)`, `subsequenceMatch(query,target)`, `atCompletionMatches(token,value)`, `subsequenceScore(query,target)`, `interface AutocompleteItem` (215), `type ModelMentionCompletion` (244), `interface SlashCommand` (249). Provider filtering/insertion semantics and slash/file/@-mention ranking were not captured.

---

## 4. LaTeX → Unicode / ANSI — `l/src/latex-block.ts`, `l/src/latex-to-unicode.ts`

Exports captured: `latexToBlock(src): string[]` (latex-block.ts:1449), `describeLatex(src, display=true): NativeNode` (1438), `MATH_FONT_COMMANDS: ReadonlySet<string>` (365), `latexColorScope(model, spec)` (1347), `latexToUnicode(src)` (2026), `isBareMathEnvironment(env)` (2072), `renderMathInText(text)` (2154). **Supported constructs, unicode-vs-ascii output forms, block rendering rules, and the symbol maps were not read** — this is the largest remaining gap.

---

## 5. Vim — `l/src/vim.ts` (API captured, command table not read)

`VimMode = "insert"|"normal"|"visual"|"visual-line"|"replace"`; `VimOperator = "d"|"y"|"c"|">"|"<"`; `VimPosition`, `VimBuffer`, `VimCommand` (34), `VimInsertRecord`; helpers `nextGraphemeStart`, `prevGraphemeStart`, `lastGraphemeStart`, `countGraphemes`. **The motion/operator/editing subset, registers, marks, and plug-in points were not read.**

---

## 6. Terminal graphics / capabilities

### 6.1 Glyph protocol — `l/src/glyph-protocol.ts`

- APC framing: `ESC _ 25a1 ; <verb> [; k=v]* [; payload] ESC \` (`APC="\x1b_"`, `ST="\x1b\\"`, id `"25a1"`).
- Verbs: `s` support query, `q` coverage, `r` register, `c` clear.
- `encodeGlyphSupportQuery()` → `"\x1b_25a1;s\x1b\\"`.
- `encodeGlyphCoverageQuery(cp)` → `"\x1b_25a1;q;cp=<hex>\x1b\\"`.
- `encodeGlyphRegistration(cp, entry, metrics)` → `\x1b_25a1;r;cp=<hex>;reply=0;upm=<upm>;aw=<aw>;lh=<lh>;size=<stretch|contain>;<base64 glyf>\x1b\\`; throws unless `isPrivateUseCodepoint`.
- `encodeBundledGlyphRegistrations(metrics)` concatenates registrations for every `glyphs` key (hex → codepoint).
- `isPrivateUseCodepoint`: `0xE000–0xF8FF`, `0xF0000–0xFFFFD`, `0x100000–0x10FFFD`.
- `parseGlyphProtocolReply`: body split on `;`; `s`→`{verb,formats:fmt.split(",")}`; `q`→`{cp:parseInt(cp,16),coverage:status.split(",")}`; `r`→`{cp,status:parseInt(10),reason?}`; `c`→`{status,reason?}`; unknown verb/malformed ⇒ null; empty list values mean none.
- `GLYPH_CONFIRMATION_CODEPOINT = 0xf0d57` (`icon.maple`); `GLYPH_BUNDLE` from `theme/glyph-bundle.json` with `upm`, `aw`, `lh`, `glyphs: Record<hex,{glyf,stretch?}>`.

### 6.2 Kitty Unicode placeholders — `l/src/kitty-graphics.ts`

- `KITTY_PLACEHOLDER = "\u{10eeee}"`; `ROWCOLUMN_DIACRITICS` = 297 codepoints (listed at lines 30-54; e.g. starts `0x305,0x30d,0x30e,0x310,0x312,0x33d,…`, ends `0x1d242,0x1d243,0x1d244`); `KITTY_PLACEHOLDER_MAX_CELLS = 297`.
- `kittyPlaceholdersFit(columns,rows)`: both ≥1 and ≤297.
- `encodeKittyVirtualPlacement({imageId,placementId?,columns,rows})` → `\x1b_Ga=p,U=1,q=2,i=<id>[,p=<placementId>],c=<columns>,r=<rows>\x1b\\` (wrapped via tmux passthrough when inside tmux).
- `encodeKittyPlaceholderGrid`: per row, prefix `fg = \x1b[38;2;R;G;Bm` (imageId as 24-bit RGB), optional underline `\x1b[58:2::R:G:Bm` (placementId), then for each column `U+10EEEE + diacritic(row) + diacritic(col)`, then `\x1b[39;59m`. Exactly `rows` strings.
- `renderKittyPlaceholderLines`: prefixes line 0 with the virtual placement APC.
- Detection: `detectKittyUnicodePlaceholdersSupport(terminalId, env)`: `PI_NO_KITTY_PLACEHOLDERS` truthy ⇒ false; `PI_KITTY_PLACEHOLDERS` truthy ⇒ true, falsy ⇒ false; inside multiplexer with `PI_FORCE_IMAGE_PROTOCOL=kitty` ⇒ true; if any session multiplexer has `imagesRequireOverride` ⇒ false; else true for ids `kitty|ghostty|monstar|otty|rio|tern`. Default `unicodePlaceholders:false` until capabilities seeds it.

### 6.3 Sixel — `l/src/sixel.ts`

- `ensureSixelRasterWidth(sequence)`: regex `^(\x1bP[0-9;]*q"\d+;\d+;(\d+);\d+(?:#…|\$)*)(?=-)`; inserts `!${width}?` before the first `-` so an empty first band has an explicit transparent width (Zellij).
- `cropSixelTop(sequence, pixels, maxHeightPixels?)`: `pixels<=0` ⇒ unchanged; requires `\x1bP…q` header and trailing `\x1b\\`; grammar `[?-~]|!\d+[?-~]|#\d+(?:;\d+)*|"\d+(?:;\d+)*|[-$]`; one raster declaration `"pan;pad;width;height"` (square pixels only, positive dims); palette defs `#n;1|2;r;g;b` only before drawing; `#n` selects register; `$` carriage return; `-` newline (band advance 6px); run char = `63+bits`, `!count` compression (kept when count>3). Computes `width=max(declared,dataWidth)`, `height=max(declared,paintedHeight)`; `croppedHeight = max(0,height-pixels)`, capped to `floor(maxHeightPixels/6)*6`; returns `""` if 0. Output = header + `"${pan};${pad};${width};${croppedHeight}` + palette + bands shifted by `pixels%6` with mask `(1<<min(6,croppedHeight-i*6))-1`, inserting `$` + next band shifted by `shift-6` when shift≠0, `-` between bands, then `\x1b\\` and `ensureSixelRasterWidth`. Malformed input throws.

### 6.4 Capabilities — `l/src/terminal-capabilities.ts` (first 260 lines)

- `ImageProtocol`: `Kitty="\x1b_G"`, `Iterm2="\x1b]1337;File="`, `Sixel="\x1bPq"`. `NotifyProtocol`: `Bell="\x07"`, `Osc99="\x1b]99;;"`, `Osc9="\x1b]9;"`.
- `TerminalId` union: kitty, ghostty, wezterm, iterm2, vscode, alacritty, warp, orca, otty, rio, tern, monstar, rex, base, trueColor.
- `TerminalInfo` fields: `id, imageProtocol, trueColor, hyperlinks, notifyProtocol=Bell, deccara=false, supportsScreenToScrollback=false, supportsTextSizing=false, hangulJamoWidth="platform"`; `clone()`.
- `hasImageMarkerAt(line,start)`: sixel requires `ESC P`, then digits/`;`, then `q` within 128 chars; other protocols match the literal prefix within 512 chars, or the UTF-16 surrogate pair of `U+10EEEE`.
- `isImageLine` scans the same windows.
- `formatNotification`: Bell ⇒ `"\x07"`; structured message with Osc99 + `osc99CapabilitiesConfirmed` ⇒ `formatOsc99Notification`; else `<protocol><notificationToLine(message)>\x1b\\`; plain string ⇒ `<protocol><message>\x1b\\`.
- `sendNotification`: suppressed/headless ⇒ return; `routeTerminalMultiplexerNotification` first, then `writeTerminalSequence(formatted)`; Bell + `shouldDeliverDesktopNotification(id,true)` ⇒ `sendDesktopNotification`.
- `isSshSession`: any of `SSH_CONNECTION|SSH_TTY|SSH_CLIENT`.
- `isNotificationSuppressed`: `PI_NOTIFICATIONS` ∈ {off,0,false}.
- `getForcedImageProtocol`: `PI_FORCE_IMAGE_PROTOCOL` (now `MAPLE_FORCE_IMAGE_PROTOCOL`) → kitty/iterm2|iterm/sixel, or off|none|0|false ⇒ null, unknown ⇒ null; `isImageProtocolForced()` true when defined (including off).
- `getTerminalBackgroundColor` / `setTerminalBackgroundColor`: trusted OSC 11 value else `detectTerminalBackground()==="light" ? 0xffffff : 0x000000`.
- `isWindowsTerminalPreviewSixelSupported`: win32 + `WT_SESSION` + `TERM_PROGRAM=windows_terminal` (version gating continues past line 260).

---

## 7. Multiplexer / tmux / notify / lang / hotkeys / key hints / utils

### 7.1 Multiplexer detection — `l/src/terminal-multiplexer.ts`

Registry order: `herdr, tmux, screen, zellij, cmux, wmux, orca`. Each module has `id`, `sessionEnvKeys`, `termPrefix?`, `precedence: "session"|"outerApp"`, `ownsScreenGrid`, `backgroundQueryBrokenOn?`, `imagesRequireOverride?`, `isInside(env)`, optional `notifier {tier:"pane"|"surface"|"inBand", send(request)}`.
- `TERMINAL_MULTIPLEXER_ENV_KEYS` = union of `sessionEnvKeys` plus `"TERM"`.
- `hasTerminalMultiplexerSession(id,env)` = module `isInside` (TERM excluded).
- `terminalMultiplexerSessions(env)` = all `isInside` in registry order.
- `isTerminalBackgroundQueryTrusted(env,platform)` = no active session whose `backgroundQueryBrokenOn` includes platform.
- `terminalMultiplexerForTerm(env)` = first module whose `termPrefix` is a prefix of lowercase `TERM`.
- `classifyTerminalMultiplexerModule` = session tier `isInside` → TERM prefix → outerApp tier `isInside`; `classifyTerminalMultiplexer` returns id or null; `isInsideTerminalMultiplexer` = `ownsScreenGrid` of the classified module.
- `routeTerminalMultiplexerNotification` sorts notifiers by tier `["pane","surface","inBand"]` (stable registry tiebreak) and returns true on first successful send.

### 7.2 tmux — `l/src/tmux.ts`

- `wrapTmuxPassthroughIfNeeded(payload,env)` = wrap only when `tmuxMultiplexer.isInside(env)`.
- `CLIENT_TERMINAL_TIMEOUT_MS=2000`, `TMUX_COMMAND_TIMEOUT_MS=500`; `tmuxClientProgramsFromReport(report)` splits on the first tab → `[program from client_termtype, program from client_termname]`; `CLIENT_TERMNAME_PROGRAMS`: `xterm-kitty→kitty`, `xterm-ghostty→ghostty`, `xterm-rex→rex`, `wezterm→wezterm`.
- `queryTmuxClientTerminalNames` runs `tmux display-message -p "#{client_termtype}\t#{client_termname}"`; `resolveTmuxClientTerminalNames` caches once per process, `[]` outside tmux/test runtime.
- `tmuxSessionSpeaksTsp` = `tmux show-options -qv @tern-tsp` non-empty.
- `setTmuxPaneApcTitles(on)`: `set-option -pu|-p -t $TMUX_PANE allow-set-title [off]`.
- `tmuxClientRendersSixel`: `tmux display-message -p "#{client_name}"`, then `show-messages -T -t <client>` matched against `/^\s*\d+: Sxl: \(flag\) true$/m`; cached per `$TMUX`.
- `tmuxPaneAllowsPassthrough`: `tmux show-options -Apqv [-t $TMUX_PANE] allow-passthrough`; empty/`on`/`all` ⇒ true (tmux <3.3 always forwards); cached per `TMUX + TMUX_PANE`.

### 7.3 Desktop notification — `l/src/desktop-notify.ts`

- `APP_NAME="omp"`.
- `hasLinuxDesktopSession`: linux and (`DBUS_SESSION_BUS_ADDRESS` or `$XDG_RUNTIME_DIR/bus` exists).
- `shouldDeliverDesktopNotification(terminalId, notifyProtocolIsBell)`: requires Bell, a Linux desktop session, and `PI_NO_DESKTOP_NOTIFY !== "1"`.
- `resolveDesktopNotifier`: `$which("notify-send")` else `$which("gdbus")`, cached; `{kind,path}`.
- Fields: string ⇒ `{title:"omp", body:message, urgency:"normal"}`; object ⇒ title = trimmed `title` or `"omp"`, body = `body ?? ""`, urgency = critical/low else normal.
- `notify-send` argv: `[path, "--app-name","omp", "--urgency=<low|normal|critical>", "--expire-time=5000", title, body]`.
- `gdbus` argv: `[path,"call","--session","--dest","org.freedesktop.Notifications","--object-path","/org/freedesktop/Notifications","--method","org.freedesktop.Notifications.Notify","omp","0","",title,body,"[]",{"urgency": <byte 0|1|2>},"5000"]`.
- `sendDesktopNotification`: spawn with ignored stdio and `child.unref()`; never throws.

### 7.4 lang-from-path — `l/src/lang-from-path.ts`

`getLanguageFromPath` basename rules first: `.env.*` → `env`; `dockerfile`, `dockerfile.*`, `containerfile` → `dockerfile`; `.emacs` → `emacs-lisp`; `justfile` → `just`; `cmakelists.txt` → `cmake`. Then `EXTENSION_LANG[themeExtensionKey]` (substring after last `.`, lowercased; whole lowercase path if no dot) → `[0]`. `detectLanguageId` basename rules: dockerfile/containerfile → `dockerfile`; `.emacs` → `emacs-lisp`; `makefile`/`gnumakefile` → `makefile`; `justfile` → `just`; `cmakelists.txt` or ext `cmake` → `cmake`; else `EXTENSION_LANG[lspExtensionKey]?.[1] ?? "plaintext"`. `isMarkdownPath` = language `markdown`.

Complete `EXTENSION_LANG` (`ext: [highlight, lsp]`):

```
ts,cts,mts: typescript/typescript      tsx: tsx/typescriptreact
js,jsx,mjs,cjs: javascript/javascript  jsx: javascript/javascriptreact
rs: rust/rust   go: go/go
c,h: c/c        cpp,cc,cxx,hh,hpp,hxx,cu,cuh,ino: cpp/cpp
zig: zig/zig
py,pyi: python/python   rb,rbw,gemspec: ruby/ruby   erb: erb/erb
lua: lua/lua
sh,bash,zsh,ksh,bats,tmux,cgi,fcgi,command,tool: bash/shellscript
fish: fish/fish   pl,pm,perl: perl/perl   php: php/php
java: java/java   kt,ktm,kts: kotlin/kotlin   scala,sc,sbt: scala/scala
groovy: groovy/groovy   clj,cljc,cljs,edn: clojure/clojure   el: emacs-lisp/emacs-lisp
cs: csharp/csharp   fs: fsharp/fsharp   vb: vb/vb
html,htm,xhtml: html/html   css: css/css   scss: scss/scss   sass: sass/sass
less: less/less   vue: vue/vue   svelte: svelte/svelte   astro: astro/astro
json: json/json   jsonc: jsonc/jsonc   yaml,yml: yaml/yaml   toml: toml/toml
xml,xsl,xslt,svg,plist: xml/xml   ini: ini/ini
md,markdown,mdx,mdc,mkd,mdown: markdown/markdown
rst: restructuredtext/restructuredtext   adoc: asciidoc/asciidoc   tex: latex/latex
sql: sql/sql   graphql,gql: graphql/graphql   proto: protobuf/protobuf
dockerfile,containerfile: dockerfile/dockerfile
tf: hcl/terraform   hcl,tfvars: hcl/hcl   nix: nix/nix
ex,exs: elixir/elixir   erl,hrl: erlang/erlang   hs: haskell/haskell
ml,mli: ocaml/ocaml   swift: swift/swift   r: r/r   jl: julia/julia
dart: dart/dart   elm: elm/elm   v: verilog/v   nim: nim/nim   cr: crystal/crystal
d: d/d   pas,pp: pascal/pascal   lisp,lsp: lisp/lisp   rkt: racket/racket   scm: scheme/scheme
ps1,psm1: powershell/powershell   bat,cmd: bat/bat
tla,tlaplus: tlaplus/tlaplus
m,mm: objc/plaintext   sol: solidity/plaintext   odin: odin/plaintext
star,bzl: starlark/plaintext   sv,svh,vh: verilog/plaintext   vim: vim/plaintext
ipynb: ipynb/plaintext   hbs,hsb,handlebars: handlebars/plaintext
diff,patch: diff/plaintext   makefile,mk,mak: make/plaintext   cmake: cmake/cmake
justfile: just/plaintext   txt,text: text/plaintext   log: log/plaintext
csv: csv/plaintext   tsv: tsv/plaintext   cfg,conf,config,properties,env: conf/conf
gitignore,gitattributes,gitmodules,editorconfig,npmrc,prettierrc,eslintrc,prettierignore,eslintignore: conf/plaintext
```

### 7.5 Hotkeys markdown — `l/src/hotkeys-markdown.ts`

`buildHotkeysMarkdown(bindings)` emits `**Group**`, `| Key | Action |`, `|-----|--------|`, rows, blank line between groups. Groups: Navigation (arrows; alt+left/right; `ctrl+a`/`home`[`super+left` on mac]; `ctrl+e`/`end`[`super+right`]), Editing (enter; shift+enter/alt+enter; ctrl+w/alt+backspace; ctrl+u; ctrl+k; copyLine; copyPrompt), Other (tab; interrupt; clear; exit rows split into delete-char-forward-with-draft vs exit; suspend; display.reset; thinking.cycle; model.cycleForward/Backward/selectTemporary/select; plan.toggle; history.search; tools.expand; tools.toggleVisibility; links.hint; ask.hide; ask.note; approval.approve; thinking.toggle; editor.external; local.open; retry; clipboard.pasteImage; stt.pushToTalk; live.toggle; agents.hub / session.observe / double-tap left; tokens `#<number>`, `#`/`#<text>`, `/`, `!`, `!!`, `$`, `$$`). `markdownPart`: text literal; token wrapped in backticks; action label = `getDisplayString(action) || "Disabled"` wrapped in backticks, or key hints joined by `/`. Native `keySpans`: ids as `key` spans (keycaps), tokens `code`, prose dim.

### 7.6 Key hints — `l/src/key-hint-format.ts`

- Platform seam `setKeyHintPlatform`/`keyHintPlatform` (default `process.platform`).
- `MODIFIER_SYMBOLS`: ctrl `[key.ctrl, key.ctrlMac]`, alt `[key.alt, key.altMac]`, shift `[key.shift, key.shift]`, super `[key.super, key.superMac]`.
- `KEY_SYMBOLS`: esc/escape→`key.esc`, enter/return→`key.enter`, space, tab, backspace, delete, up, down, left, right.
- `KEY_WORDS`: home→`Home`, end→`End`, pageup→`PgUp`, pagedown→`PgDn`, insert→`Ins`.
- `formatKeyHint` = `formatKey(key, activeThemeSymbol)`. Parsing: a trailing `+` is the plus key itself. Modifiers render in canonical order ctrl, alt, shift, super; a glyph label (no `[A-Za-z0-9]`) is followed by `key.joiner`, a word label by `+`. Bare modifier ⇒ its symbol. Base: symbol if mapped; else word; else single char stays lowercase if no modifiers, uppercase in a chord; else capitalize first letter. `formatTooltipKey`: `escape` → `"esc"`, otherwise unicode preset symbols.
- `formatKeyHints(keys)` joins with `/`; `formatDoubleTap` doubles the label separated by a space (word labels) or `key.joiner`.

### 7.7 utils — `l/src/utils.ts`

- `Ellipsis` and `DEFAULT_TAB_WIDTH` are re-exported by `l/src/utils.ts`.
- Hangul: `HangulCompatibilityJamoWidth = "platform"|"unicode"|1|2`; `setHangulCompatibilityJamoWidth` bumps `widthConfigEpoch` on change and forwards wire code (`unicode`→3, number→itself, else 0); `getWidthConfigEpoch()`; `publishLineWidths`/`getPublishedLineWidths` weak sidecar keyed by exact lines array, validated element-wise and epoch-stamped; `resetHangulCompatibilityJamoWidthForTests`.
- `encodeTextSizing(text,{scale,widthCells,verticalAlign,horizontalAlign})`: metadata `s=<1|2|3>`, `w=<trunc≥0>`, `v=<0|1|2>`, `h=<0|1|2>` joined by `:`; unsafe control chars (`\x00-\x1f\x7f-\x9f`) replaced with spaces; output `\x1b]66;<meta>;<safeText>\x1b\\`.
- `truncateToWidth(text,maxWidth,ellipsisKind?,pad?)`: `maxWidth=max(0,maxWidth|0)`; fast path returns text unchanged when `!pad && !text.includes("\x1b]66") && text.length*3 <= maxWidth`; otherwise native `truncateToWidth(text,maxWidth, ellipsisKind (string ⇒ Ellipsis.Omit, default Unicode), pad??false, DEFAULT_TAB_WIDTH)`.
- `sliceWithWidth(line,startCol,length,strict?)` → native with `DEFAULT_TAB_WIDTH`.
- `wrapTextWithAnsi(text,width)` → native with `DEFAULT_TAB_WIDTH`.
- `wrapLiteralLine(line,width)`: `columns=max(1,width)`; zero-width ⇒ `[""]`; slice with `strict=true`, fall back to non-strict when width 0; advance by `max(1, slice.width)`.
- `extractSegments(line,beforeEnd,afterStart,afterLen,strictAfter)`.
- `replaceTabs` (8 spaces), `singleLine` (replace tabs, `[\r\n]+`→space), `finiteWidth`, `padding` (512-space buffer), `centerLine`, `getSegmenter` (`Intl.Segmenter` grapheme).
- `visibleWidth(str)`: empty ⇒ 0; cache (≤512 chars, ≤2048 entries, epoch-cleared); pure printable ASCII `[\u0020-\u007e]*` ⇒ `str.length`; long strings without tab/jamo/APC/OSC66 ⇒ `Bun.stringWidth(str,{countAnsiEscapeCodes:false,ambiguousIsNarrow:true})`; else counts tabs, ESC, Compatibility Jamo `U+3131–U+318E` and filler `U+3164`, strips OSC 66 spans (adds payload width scaled by `s=`), strips APC spans (zero cells), and applies the Hangul correction: target width 1 ⇒ subtract one cell per jamo; target 2/unset ⇒ subtract 2 cells per filler only (platform target is 1 on darwin, else unicode/uncorrected).

---

## Architecture / wiring

`ChatTranscriptBuilder` is the single composition point: it owns a `TranscriptContainer`, maps persisted `TranscriptEntry` → message component, and is reused verbatim by the live transcript and by the fullscreen selectors (`rewind-selector`, `copy-selector`, `agent-transcript-viewer`, `link-browser`) through `appendOutlineEntries`. `TranscriptBrowser` owns the framed viewport, `ScrollView`, `DynamicBorder`, outline row cache, scroll/anchor state, and delegates content to a controller `frame(context)`. `transcript-outline.ts` provides target mapping/visibility/composition. Tool rendering has two paths: bespoke `ToolExecutionComponent` content boxes (custom/extension renderers, state-tinted) and the generic `formatDefaultToolExecution` card; framed cards share `renderOutputBlock`'s border/background engine. Native (TSP) counterparts are built in `describe()` methods (`card`, `md`, `span`, `text`, `noteText`) and are distinct from the ANSI paths.

## Files

- `l/src/chat/transcript-entry.ts` — persisted entry restore, user-request detection, tail pagination, text/image extraction.
- `l/src/chat/chat-transcript-builder.ts:76-760` — builder deps/state and all role dispatch rules.
- `l/src/chat/user-message.ts:68-560` — user bubble options, color, render, badge, emergency row, synthetic collapse, token marks.
- `l/src/chat/assistant-message.ts:600-700,1480-1560,2080-2170` — constructor, container, markdown creation, block spacing.
- `l/src/chat/thinking-display.ts` — thinking fold/comment-noise algorithm.
- `l/src/chat/tool-execution.ts:245-620,1814-1935,2270-2380` — tool lifecycle, tints, content box, default card, benign skip.
- `l/src/tools/default-renderer.ts:41-260` — generic card status/body, JSON tree safety, native describe.
- `l/src/render/tool-card.ts:24-277`, `l/src/render/output-block.ts:1-300`, `l/src/render/status-line.ts`, `l/src/render/render-utils.ts:293-340` — card/block/status/icon primitives and border/background constants.
- `l/src/chrome/message-frame.ts`, `l/src/chat/custom-message.ts`, `hook-message.ts`, `collab-prompt-message.ts` — framed custom/hook/collab styling.
- `l/src/chat/reaction.ts`, `recap-notice.ts`, `stripped-tool-calls-placeholder.ts`, `skill-title-input.ts`, `background-tan-message.ts`, `transcript-actions.ts` — small message contracts.
- `l/src/chat/transcript-browser.ts`, `transcript-outline.ts`, `l/src/overlays/rewind-selector.ts:207-620,960-1160`, `l/src/components/scroll-view.ts:354-410` — browser layout/navigation/selection/jump.
- `l/src/fuzzy.ts:1-526` — complete matcher/ranker.
- `l/src/utils.ts:1-400` — width/truncate/wrap/padding/OSC66/APC/Hangul rules.
- `l/src/lang-from-path.ts` — complete extension table.
- `l/src/glyph-protocol.ts`, `kitty-graphics.ts`, `sixel.ts`, `terminal-capabilities.ts:1-260` — graphics protocols.
- `l/src/terminal-multiplexer.ts`, `tmux.ts`, `desktop-notify.ts`, `hotkeys-markdown.ts:1-235`, `key-hint-format.ts` — environment/notification/formatting.

## Checks for the implementing agent

Read before writing Rust: `l/src/autocomplete.ts` (providers, insertion, `subsequenceScore`), `l/src/latex-to-unicode.ts` + `latex-block.ts` (symbol tables and block rules), `l/src/vim.ts` (command/motion/operator/register/mark tables and integration points), `l/src/terminal-capabilities.ts:260-end` + `l/src/terminal.ts`/`active-terminal.ts` (query flow, caching, `TERMINAL` construction), `l/src/multiplexers/*.ts` (per-multiplexer env keys, precedence, notifier payloads), and the remaining `chat/` renderers listed in the coverage gap. The native width/sixel primitives are referenced by `l/src/utils.ts`/`l/src/terminal-capabilities.ts` and are not defined in this repo.