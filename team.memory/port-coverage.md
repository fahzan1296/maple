## Maple clean-room coverage map

Reference tree enumerated: **457 `.ts` files** (contract stated 455; +2 from full subtree glob — `components/{layout,composer}` and `overlays/extensions` were not in the flat listing). Rust targets: `src/**/*.rs` (32 files) + `crates/maple-tui/src/**/*.rs` (105 files). Evidence: Rust `//! Reference:/Port of l/src/...` headers, `mod.rs` re-export lists, and symbol greps. Line counts from `read path:-1` (spot-checked: `tools/bash.ts`=890, `chat/assistant-message.ts`=2192, `overlays/ask-dialog.ts`=1839, `theme/theme.ts`=870, `components/editor.ts`=5608).

### l/src (root) — 41 files, 25 029 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| vim.ts | 1366 | MISSING | — | no Rust counterpart |
| thinking.ts | 425 | MISSING | — | no Rust counterpart |
| stdin-buffer.ts | 1236 | MISSING | — | no Rust counterpart |
| space-hold.ts | 247 | MISSING | — | no Rust counterpart |
| motion-effects.ts | 27 | MISSING | — | no Rust counterpart |
| latex-block.ts | 1460 | MISSING | — | no Rust counterpart |
| kill-ring.ts | 51 | MISSING | — | no Rust counterpart |
| key-hint-format.ts | 137 | MISSING | — | no Rust counterpart |
| keybinding-matchers.ts | 136 | MISSING | — | no Rust counterpart |
| hotkeys-markdown.ts | 294 | MISSING | — | no Rust counterpart |
| editor-component.ts | 74 | MISSING | — | no Rust counterpart |
| tui.ts | 4842 | PARTIAL | `crates/maple-tui/src/tui.rs`, `screen.rs` | missing `RenderTimer`, `RenderScheduler`, `TuiPaint`, `PaintListener`, `TUIOptions`, `ViewportSize`, `HistoryBatch`, `TerminalFramePlan`, `TerminalFrameProvider`, `SEGMENT_RESET`; Rust `Tui` is a component container + `Screen` painter |
| utils.ts | 778 | PARTIAL | `text_shape.rs`, `render/render_utils.rs`, `render/width_aware_text.rs` | missing `getHangulCompatibilityJamoWidth`, `getWidthConfigEpoch`, `publishLineWidths`, `getPublishedLineWidths`, `setHangulCompatibilityJamoWidth`, `resetHangulCompatibilityJamoWidthForTests`, `TextSizingScale/VerticalAlign/HorizontalAlign` |
| tmux.ts | 183 | PARTIAL | `terminal_capabilities.rs` | missing `tmuxClientProgramsFromReport`, `tmuxSessionSpeaksTsp`, `setTmuxPaneApcTitles`, `tmuxClientRendersSixel` |
| terminal.ts | 3096 | PARTIAL | `terminal/{mod,process,virt}.rs` | missing `chunkForConPTY`, `STDOUT_BACKLOG_CLEAR_BYTES`, `StdoutStallWatchdog`, `setAltScreenActive`, `emergencyTerminalRestore`, `TerminalStartOptions`, `TerminalAppearance`, report-handler types |
| terminal-multiplexer.ts | 138 | PARTIAL | `terminal_capabilities.rs`, `multiplexers.rs` | missing `TERMINAL_MULTIPLEXER_ENV_KEYS`, `classifyTerminalMultiplexerModule`, `routeTerminalMultiplexerNotification` |
| symbols.ts | 26 | PARTIAL | `symbols.rs` | missing `BoxSymbols` (has `LineSymbols`, `TableSymbols`, `SymbolTheme`) |
| terminal-capabilities.ts | 1602 | PARTIAL | `terminal_capabilities.rs`, `kitty_graphics.rs`, `native/blobs.rs` | missing `TERMINAL_ID`/`TERMINAL` consts, `ImageDimensions`, `ImageRenderOptions`, kitty transmit/placement-line/delete helpers, iTerm2 helpers, `calculateImageRows`, `setTerminalScreenToScrollback`, `setTerminalTextSizing` |
| sixel.ts | 185 | PARTIAL | `terminal_capabilities.rs`, `render/sixel.rs` | missing `cropSixelTop` (has `ensureSixelRasterWidth`) |
| mouse.ts | 110 | PARTIAL | `mouse.rs`, `mouse_sgr.rs` | missing `routeSgrMouseInput`, `routeSelectListMouse`, `SelectListMouseTarget`, `MouseRoutable`, `SgrMouseHandler` |
| keybindings.ts | 351 | PARTIAL | `keybindings.rs` | missing `Keybindings`, `Keybinding`, `KeybindingDefinition(s)`, `KeybindingsConfig`, `TUI_KEYBINDINGS`, `KeybindingConflict`, `addKeyAliases` |
| keys.ts | 565 | PARTIAL | `keys.rs` | missing `isWindowsTerminalSession`, `matchesRawBackspace`, kitty-protocol active get/set, `ModifierName`, `KeyId`, `Key` const, `isKeyRelease`, `isKeyRepeat`, `parseKittySequence` |
| index.ts | 84 | PARTIAL | `lib.rs` | barrel only re-exports `Component`/`Key`/`Terminal`/`Tui`/`Trigger`/`text_shape`; most modules not re-exported |
| autocomplete.ts | 1375 | PARTIAL | `autocomplete.rs` | missing `ModelMentionCompletion` and model-mention/emoji/skill-token completion wiring |
| app-keybindings.ts | 715 | PARTIAL | `keybindings.rs` | missing `AppKeybinding`, `getDefaultPasteImageKeys`, `KEYBINDINGS`, `KeybindingsCreateOptions`, app `KeybindingsManager` |
| active-terminal.ts | 80 | PARTIAL | `active_terminal.rs` | missing `register_stdout_error_handler` |
| windows-input-mode.ts | 460 | PORTED | `windows_input.rs` | doc header: port of `l/src/windows-input-mode.ts` |
| windows-altgr.ts | 227 | PORTED | `windows_altgr.rs` | doc header: port |
| ttyid.ts | 84 | PORTED | `ttyid.rs` | doc header: port |
| spinner-clock.ts | 88 | PORTED | `spinner_clock.rs` | doc header: port |
| lang-from-path.ts | 253 | PORTED | `lang_from_path.rs` | doc header: port |
| latex-to-unicode.ts | 2216 | PORTED | `latex_to_unicode.rs` | doc header: port |
| kitty-graphics.ts | 181 | PORTED | `kitty_graphics.rs` | doc header: port |
| glyph-protocol.ts | 143 | PORTED | `glyph_protocol.rs` | doc header: port |
| bracketed-paste.ts | 137 | PORTED | `bracketed_paste.rs` | doc header: port |
| deccara.ts | 314 | PORTED | `deccara.rs` | doc header: port |
| desktop-notify.ts | 193 | PORTED | `desktop_notify.rs` | doc header: port |
| debug-server.ts | 414 | PORTED | `debug_server.rs` | doc header: port |
| fuzzy.ts | 526 | PORTED | `fuzzy.rs` | doc header: port |
| frame-rate.ts | 36 | PORTED | `frame_rate.rs` | doc header: port |
| loop-watchdog.ts | 174 | PORTED | `loop_watchdog.rs` | doc header: port |

### l/src/tools — 67 files, 28 612 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| yield.ts (234), xd-url.ts (54), xdev.ts (272), write.ts (872), web-search.ts (362), wait.ts (912), web-search-types.ts (103), vibe.ts (672), tool-errors.ts (15), url-scheme-host.ts (63), todo.ts (802), terminal-output.ts (153), think.ts (46), task.ts (2675), task-yield-assembly.ts (173), task-repair-args.ts (133), subprocess.ts (39), streaming-output.ts (1821), resolve.ts (201), result-card.ts (35), report-tool-issue.ts (19), read.ts (1019), proc-render.ts (523), output-meta.ts (469), native-view.ts (405), mcp.ts (502), memory.ts (302), match-line-format.ts (20), lsp.ts (1147), line-ranges.ts (118), list-limit.ts (43), json-tree.ts (597), job-widget.ts (1048), irc.ts (45), hashline-format.ts (64), grouped-file-output.ts (777), grep.ts (388), goal.ts (208), github.ts (964), glob.ts (379), gh-format.ts (18), fetch.ts (326), find.ts (469), edit.ts (1719), debug.ts (238), default-renderer.ts (263), daemon.ts (51), cfg-url.ts (30), conflict-detect.ts (77), cfg-render.ts (182), bash-interactive.ts (370), autoresearch.ts (435), ast-edit.ts (404), ast-grep.ts (264), ask.ts (375), agent-tree.ts (135) | 24 105 | MISSING | — | 57 files; no Rust counterpart (grep for `ToolRenderer`, `tool_renderers`, `EvalView`, `RunTrace`, etc. negative) |
| bash.ts | 890 | PARTIAL | `tools/bash_execution.rs` | missing `BashRenderContext`, `ShellRendererConfig`, `getBashEnvForDisplay`, `createShellRenderer` wiring |
| eval.ts | 1384 | PARTIAL | `tools/eval_execution.rs` | missing `EvalStatusEvent`, `EvalTableColumn`, `EvalView`, `EvalViewEntry`, `EvalCellResult`, `EvalToolDetails`, `statusEventKey`, `MAX_STATUS_EVENTS`, `EVAL_DEFAULT_PREVIEW_LINES` |
| eval-format/javascript.ts | 954 | PARTIAL | `tools/eval_execution.rs` | JS tokenizer not ported; only `format_eval_code_for_display` |
| eval-format/python.ts | 544 | PARTIAL | `tools/eval_execution.rs` | Python tokenizer not ported; only `format_eval_code_for_display` |
| shell-viz.ts | 199 | PARTIAL | `tools/shell_viz.rs` | missing `attachShellTrace`, `shellVisualization`, `readShellPresentation` (replaced by `decode_shell_presentation`) |
| shell-trace.ts | 80 | PARTIAL | `tools/shell_viz.rs` | missing `RunTrace` owner, `SHELL_VIZ_VIEWS/LAYERS`, `shellVizEventAction`, `changeShellVizPreferences` |
| renderer.ts | 226 | PARTIAL | `tools/tool_execution.rs` | missing `RenderResultOptions`, `RenderResultContextOptions`, `ToolRenderResult`, `ToolFigure`, `FirstResultViewportRepaint`, `ToolActivitySummary`, `ToolActivityContext`, `NativeToolHead`, `NativeToolView`, `ToolRenderer` |
| index.ts | 75 | PARTIAL | `tools/mod.rs` | `toolRenderers` registry not ported |
| shell-viz-render.ts | 187 | PORTED | `tools/shell_viz.rs` | `shellVizText`, `renderShellVisualization` present |
| eval-format/index.ts | 43 | PORTED | `tools/eval_execution.rs` | `formatEvalCodeForDisplay` present |

### l/src/theme — 13 files, 5 277 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| tui-adapters.ts | 458 | MISSING | — | no Rust counterpart |
| mermaid-cache.ts | 160 | MISSING | — | no Rust counterpart |
| active-symbols.ts | 20 | MISSING | — | no Rust counterpart |
| theme.ts | 870 | PARTIAL | `theme/theme_class.rs` | missing `onTerminalBackgroundFallbackChange`, `detectTerminalBackground`, `theme` singleton, `bindTheme`, `getCurrentThemeName`, `fgOrPlain`, `ThemeChangeEvent`, `initThemeSync`, `ensureThemeSync`, `ensureTheme` |
| symbols.ts | 1560 | PARTIAL | `theme/symbols.rs` | missing `SlashCommandIconName`, `SYMBOL_PRESETS`, `SPINNER_FRAMES` |
| session-color.ts | 269 | PARTIAL | `theme/theme_class.rs` | missing `getSessionAccentHex`, `getSessionAccentAnsi` (only `SessionAccentTheme` present) |
| shimmer.ts | 319 | PARTIAL | `native/describe.rs` | missing `shimmerEnabled`, `describeShimmer`, `shimmerSegments`, `shimmerText` (only `ShimmerPalette`/`shimmer` present) |
| loader.ts | 233 | PARTIAL | `theme/loader.rs` | missing async `loadTheme`/`loadThemeSync` (sync only) |
| index.ts | 5 | PARTIAL | `theme/mod.rs` | barrel |
| theme-class.ts | 870 | PORTED | `theme/theme_class.rs` | `Theme`, symbol structs, `thinkingLevelToken` |
| schema.ts | 247 | PORTED | `theme/schema.rs` | full token/color model |
| schema-validation.ts | 128 | PORTED | `theme/schema.rs` | `validate_theme_json` |
| color.ts | 138 | PORTED | `theme/color.rs` | incl. `resolve_theme_colors`, `is_light_luma`, `is_light_surface` |

### l/src/status-line — 15 files, 7 547 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| startup.ts (165), presets.ts (109), host.ts (165), git-utils.ts (42), footer.ts (399) | 880 | MISSING | — | no Rust counterpart (grep for `STATUS_LINE_PRESETS`, `parseGitHubRepo`, `readStatusLineStartupData` negative) |
| types.ts | 304 | PARTIAL | `status_line/schema.rs`, `segments.rs` | missing `ContextUsage`, `CollabSessionState`, `CollabStatus`, `StatusLineSegmentOptions`, `StatusLineSettings`, `EffectiveStatusLineSettings`, `RGB`, `StatusResetExpiry` |
| segments.ts | 1375 | PARTIAL | `status_line/segments.rs` | missing `SEGMENTS` catalog, `ALL_SEGMENT_IDS`, `describeSegment`, `thinkingLevelWord`, `statusSegmentPriority` |
| prompt-cache.ts | 154 | PARTIAL | `status_line/segments.rs` | missing `promptCacheBrief`, `promptCacheDetails` |
| schema.ts | 58 | PARTIAL | `status_line/schema.rs` | missing `CUSTOM_STATUS_LINE_DEFAULTS` (`SegmentId`/`StatusLinePreset`/`ContextLineMode`/`SeparatorStyle` present) |
| loop.ts | 28 | PARTIAL | `status_line/segments.rs` | `LoopState`/`LoopCondition`/`LoopMode`/`LoopLimit` present; `summarizeLoopCondition` private |
| metrics.ts | 104 | PARTIAL | `status_line/segments.rs` | `billing_summary` present; `BillingSummaryOptions` folded into `SegmentContext` |
| index.ts | 7 | PARTIAL | `status_line/mod.rs` | barrel |
| context-usage.ts | 584 | PARTIAL | `status_line/segments.rs`, `chrome/context.rs` | missing `computeCompactionBoundaries`, `NonMessageTokenSource`, `SystemPromptSource(s)`, `EMPTY_STRING_PARTS` |
| component.ts | 4011 | PARTIAL | `status_line/component.rs` | missing `StatusLineNativeAction`, `StatusLinePullRequest`, `statusSegmentPriority`, generic `StatusLineComponent` |
| separators.ts | 42 | PORTED | `status_line/separators.rs` | `getSeparator`, `SeparatorDef` |

### l/src/setup — 14 files, 2 826 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| wizard.ts (121), wizard-overlay.ts (572), startup-splash.ts (145), setup-version.ts (11), lazy.ts (17), scenes/updates.ts (116), scenes/types.ts (116), scenes/splash.ts (292), scenes/theme.ts (443), scenes/sign-in.ts (291), scenes/outro.ts (64), scenes/model.ts (332), scenes/glyph.ts (144), scenes/composer.ts (162) | 2826 | MISSING | — | no Rust counterpart (grep for `SetupWizard`, `runSetupWizard` negative) |

### l/src/render — 19 files, 4 590 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| tree-list.ts (227), link-hints.ts (299), file-list.ts (84), extension-links.ts (194), code-cell.ts (358) | 1162 | MISSING | — | no Rust counterpart |
| utils.ts | 109 | PARTIAL | `render/render_utils.rs` | missing `Hasher`, `RenderCache`, `buildTreePrefix`, `getTreeBranch`, `getTreeContinuePrefix`, `padToWidth` |
| tool-card.ts | 430 | PARTIAL | `render/tool_card.rs` | missing `ToolCardVariant/Content/Section/Snapshot/BuildContext/Options`, `toolCardState`, `framedToolCard` |
| status-line.ts | 55 | PARTIAL | `render/tool_card.rs` | missing `StatusLineOptions` (`StatusLine` + `render_status_line` present) |
| render-utils.ts | 1316 | PARTIAL | `render/render_utils.rs` | missing `disambiguateDisplayLabels`, `setInlineImageMaxColumns/Rows`, `resolveImageOptions`, `PREVIEW_LIMITS`, `DEFAULT_TERMINAL_PREVIEW_LINES`, `FENCE_RE`, `EMPTY_LINK_TARGETS` |
| output-block.ts | 390 | PARTIAL | `render/output_block.rs` | missing `markFramedBlockComponent`, `isFramedBlockComponent`, `outputStateTone`, `outputStateStatus`, `NativeOutputBlockSection/Options` |
| output-pane.ts | 385 | PARTIAL | `render/render_utils.rs` | missing `OutputPane` component, `NativeOutputOptions`, `describeOutputLines` |
| hyperlink.ts | 308 | PARTIAL | `render/hyperlink.rs` | missing `urlLinkSpan`, `FileUriOptions` |
| index.ts | 19 | PARTIAL | `render/mod.rs` | barrel |
| hanging-wrap.ts | 58 | PARTIAL | `render/width_aware_text.rs` | missing `wrapTreeLine`, `wrapGuidedLine` |
| width-aware-text.ts | 97 | PORTED | `render/width_aware_text.rs` | `WidthAwareText` |
| types.ts | 19 | PORTED | `render/types.rs` + `render_utils.rs` | `TreeContext`, `State` |
| sixel.ts | 69 | PORTED | `render/sixel.rs` | doc header: port |
| row-moves.ts | 109 | PORTED | `render/row_moves.rs` | doc header: reference; file not declared in `render/mod.rs` |
| composite.ts | 64 | PORTED | `render/render_utils.rs` | `composite_line_at` |

### l/src/prompt — 31 files, 8 278 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| word-completion.ts (256), video.ts (37), welcome.ts (673), usage-amounts.ts (99), tools-markdown.ts (29), typing-word.ts (59), queued-messages.ts (110), skill-tokens.ts (38), queue-input.ts (138), prompt-action-autocomplete.ts (380), prose-gate.ts (90), model-mention-autocomplete.ts (173), model-mention-syntax.ts (50), markdown-prose.ts (245), macos-spelling.ts (375), magic-keywords.ts (157), interactive-context-helpers.ts (165), internal-url-autocomplete.ts (156), image-source.ts (53), image-references.ts (179), image-format.ts (28), gradient-highlight.ts (102), emoji-autocomplete.ts (283), github-ref-autocomplete.ts (75), editor-top-gap.ts (36), custom-editor.ts (2019), composer.ts (1310), composer-cache.ts (272), composer-hints.ts (75), composer-attachments.ts (351), attachment-chips.ts (265) | 8278 | MISSING | — | no Rust counterpart |

### l/src/overlays — 82 files, 53 702 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| usage-row.ts (268), usage-dashboard.ts (1827), usage-display.ts (126), tree-selector.ts (1794), thinking-selector.ts (76), theme-selector.ts (97), spotlight-sheet.ts (564), stats-notice.ts (31), snapcompact-shape-preview.ts (243), show-images-selector.ts (62), settings-selector.ts (2016), settings-defs.ts (365), session-selector.ts (2357), session-observer-registry.ts (297), session-account-selector.ts (104), session-info-overlay.ts (398), running-subagent-badge.ts (30), rewind-selector.ts (1199), report-panel.ts (195), reset-usage-selector.ts (408), queue-mode-selector.ts (83), plugin-selector.ts (133), plugin-settings.ts (1036), plan-toc.ts (139), plan-save-overlay.ts (121), pause-screen.ts (440), plan-review-overlay.ts (1943), omfg-panel.ts (223), oauth-selector.ts (699), model-selector.ts (107), move-overlay.ts (260), model-picker.ts (505), model-hub.ts (4214), model-browser.ts (2377), logout-account-selector.ts (222), mcp-add-wizard.ts (1912), link-browser.ts (295), login-dialog.ts (544), jobs-panel.ts (511), job-tree.ts (177), hook-selector.ts (935), hub-frame.ts (361), hook-input.ts (194), hook-editor.ts (502), history-search.ts (524), error-banner.ts (94), copy-selector.ts (1192), copy-targets.ts (189), context-viewer.ts (729), context-viewer-format.ts (173), context-viewer-overview.ts (381), composer-shape-preview.ts (164), composer-shape-registry.ts (82), cleanse-panel.ts (260), codex-reset-fireworks.ts (437), btw-history.ts (109), btw-inject.ts (91), btw-panel.ts (283), bordered-loader.ts (59), btw-history-panel.ts (1227), approval-dialog.ts (506), ask-dialog.ts (1839), annotation-overlay.ts (2040), annotation-types.ts (104), agent-transcript-viewer.ts (870), agents-hub.ts (1862), agent-hub.ts (2693), agent-hub-projection.ts (438), agent-hub-renderer.ts (418), agent-hub-types.ts (80), advisor-config.ts (1678), agent-activity.ts (113), extensions/types.ts (208), extensions/state-manager.ts (377), extensions/live-tool-session.ts (67), extensions/mcp-runtime.ts (289), extensions/inspector-panel.ts (1037), extensions/extension-list.ts (931), extensions/inspector-model.ts (484), extensions/display-text.ts (39), extensions/extension-dashboard.ts (901) | 53 358 | MISSING | — | 81 files; no Rust counterpart (grep for `SessionSelector`, `ModelHub`, `AskDialog`, `AnnotationOverlay`, `ExtensionDashboard` negative) |
| overlay-box.ts | 344 | PORTED | `chrome/overlay_box.rs` | doc header: port; `PanelRows`/`PanelDivider` present |

### l/src/native — 17 files, 4 916 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| tone.ts (101), quiet.ts (105), overlay.ts (255), picker.ts (333), icons.ts (184), measure.ts (185), backend.ts (983) | 2146 | MISSING | — | no Rust counterpart (`NativeBackend`, `estimateRows`, `actionHint`, `picker*`, `normalizeIcon*` absent) |
| state.ts | 66 | PARTIAL | `native/state.rs` | missing `isNativeRendering`/`setNativeRendering`, `nativeButtons`/`setNativeButtons`, `nativePacing`/`setNativePacing`, `onNativeRenderingChange` |
| reconcile.ts | 1131 | PARTIAL | `native/state.rs` | missing `NativeRegions`, `NativeTarget`, `nativeComponentId` |
| settle.ts | 24 | PARTIAL | `native/state.rs` | missing `settleNative`, `isNativeSettled` (`NativeSettle` present) |
| memo.ts | 64 | PARTIAL | `native/state.rs` | missing `OwnerMemo` |
| node.ts | 259 | PARTIAL | `native/node.rs` | missing `NativeUiEvent`, `NativeTextEdit` |
| blobs.ts | 170 | PARTIAL | `native/blobs.rs` | missing `registerNativeBlob`, `nativeBlob`, `withBlob`, `getNativeBlob`, `NativeImageCache` |
| spans.ts | 348 | PORTED | `native/spans.rs` | all 8 exports present |
| describe.ts | 146 | PORTED | `native/describe.rs` | builders present |
| encode.ts | 285 | PORTED | `native/encode.rs` | renamed (`encode_message`, `split_message`, `Hello`/`Incoming`) |
| apply.ts | 277 | PORTED | `native/state.rs` | `TspDocument`, `ApplyError` |

### l/src/multiplexers — 9 files, 347 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| wmux.ts (16), herdr.ts (64), orca.ts (16) | 96 | MISSING | — | no `Multiplexer` variant |
| zellij.ts | 24 | PARTIAL | `multiplexers.rs` | `Multiplexer::Zellij` only; no `zellijMultiplexer` object |
| screen.ts | 15 | PARTIAL | `multiplexers.rs` | `Multiplexer::Screen` only |
| tmux.ts | 43 | PARTIAL | `terminal_capabilities.rs` | missing `tmuxClientProgramsFromReport`, `tmuxSessionSpeaksTsp`, `setTmuxPaneApcTitles`, `tmuxClientRendersSixel` |
| types.ts | 116 | PARTIAL | `multiplexers.rs`, `terminal_capabilities.rs` | missing `TerminalMultiplexerPrecedence`, `NotificationTier`, `NotificationRequest`, `Notifier`, `Hyperlinks`, `BackgroundColorCache`, `Module` |
| notify.ts | 24 | PARTIAL | `desktop_notify.rs` | missing `DEFAULT_NOTIFICATION_TITLE`, `notificationTitleAndBody`, `spawnNotifier` (only `DesktopNotifier`) |
| cmux.ts | 29 | PARTIAL | `multiplexers.rs` | `Multiplexer::Cmux` only |

### l/src/html-form — 5 files, 3 085 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| paginate.ts (99), parse.ts (582), dialog.ts (2041), media.ts (105), data.ts (258) | 3085 | MISSING | — | no Rust counterpart |

### l/src/components — 41 files, 21 391 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| image.ts (974), cancellable-loader.ts (47), box.ts (302), layout/geometry.ts (246), composer/rule.ts (55), composer/types.ts (113), composer/rail.ts (49), composer/registry.ts (58), composer/index.ts (10), composer/pi.ts (37), composer/claude.ts (39), composer/field.ts (48), composer/borderless.ts (37), composer/box.ts (85), composer/band.ts (46) | 2146 | MISSING | — | no `image.rs`/`box.rs`/`cancellable_loader.rs`; `layout/geometry.rs` is `Point`/`Rect`/`distribute_width`, unrelated to TS layout API; no composer module |
| wizard-step.ts | 238 | PARTIAL | `components/wizard_step.rs` | missing `WizardStepSlot`, `WizardStepOptions` |
| tree-view.ts | 576 | PARTIAL | `components/tree_view.rs` | missing `TreeKey`, `TreeAncestor`, `FlattenTreeOptions`, `TreePrefix`, `TreePrefixStyle`, `treeRowPrefix`, `TreeViewRowContext`, `TreeViewRowContent`, `TreeViewOptions` |
| table.ts | 157 | PARTIAL | `components/table.rs` | missing `TableCell`, `TableOptions`, `renderTableRow` |
| spacer.ts | 53 | PARTIAL | `components/spacer.rs` | missing `spaceForRows` |
| sparkline.ts | 89 | PARTIAL | `components/sparkline.rs` | missing `SparklineOptions`, `renderSparkline`, `peakBuckets` |
| select-list.ts | 833 | PARTIAL | `components/select_list.rs` | missing `SelectListTruncatePrimaryContext`, `SelectListRenderItemContext`, `SelectListStatusContext`, `SelectListLayoutOptions` |
| settings-list.ts | 1147 | PARTIAL | `components/settings_list.rs` | missing `SettingEditor`, `PrefsEditing`, `PrefsOptionSource`, `PrefsSectionRef`, `prefsSectionId`, `settingPrefsRow`, `SettingsListOptions`, `getSettingItemFilterText` |
| section.ts | 90 | PARTIAL | `components/section.rs` | missing `SectionOptions` |
| progress-bar.ts | 168 | PARTIAL | `components/progress_bar.rs` | missing `ProgressBarStyle`, `ProgressBarOptions`, `renderProgressBar` |
| scroll-view.ts | 569 | PARTIAL | `components/scroll_view.rs` | missing `ScrollViewTheme`, `ScrollViewOptions` |
| metric.ts | 171 | PARTIAL | `components/metric_row.rs` | missing `MetricSpec`, `MetricRowOptions`, `formatMetric`, `formatMetricRow` |
| markdown.ts | 5253 | PARTIAL | `components/markdown.rs` | missing `streamingMarkdownSource`, `TableLocks`, `endsInsideOpenFence`, `holdPartialTableRow`, `speculativeInlineClose`, `speculativeStreamingSource`, `fastLineStartHazard`, `lexDocument`, `fencedCode`, `MarkdownLink` |
| menu-selection.ts | 288 | PARTIAL | `components/menu_selection.rs` | missing `MenuSelectionAdapter`, `MenuSelection` class |
| loader.ts | 434 | PARTIAL | `components/loader.rs` | missing `WorkingRowVariant`, `WorkingRowSpec`, `describeWorkingRow`, `LoaderMessageColorFn` |
| key-value-list.ts | 110 | PARTIAL | `components/key_value_list.rs` | missing `KeyValueListOptions` |
| editor.ts | 5608 | PARTIAL | `components/editor.rs` | missing `NativeEditorLayout`, `EditorInlineReplacement`, `EditorWordReplacements`, `EditorTextDecorationContext`, `EditorTextAssistProvider`, `PasteOptions` |
| form.ts | 658 | PARTIAL | `components/form.rs` | missing `FormFieldTheme`, `FormControl`, `FormFieldOptions`, `TextFormFieldOptions`, `TextFormField`, `SelectFormFieldOptions`, `SelectFormField`, `FormOptions`, `EmptyInputBehavior` |
| disclosure.ts | 276 | PARTIAL | `components/disclosure.rs` | missing `DisclosureSlot`, `DisclosureOptions` |
| layout/stack.ts | 295 | PARTIAL | `components/layout/stack.rs` | missing `StackChild`, `StackOptions`, `StackHit`, `MouseRoutable` |
| layout/row.ts | 320 | PARTIAL | `components/layout/row.rs` | missing `RowChild`, `RowOptions`, `RowHit`, `MouseRoutable` |
| layout/split-pane.ts | 414 | PARTIAL | `components/layout/split_pane.rs` | missing `PaneSide`, `SplitPaneSize`, `SplitPaneOptions`, `SplitPaneGeometry`, `SplitPaneHit`, `MouseRoutable` |
| truncated-text.ts | 102 | PORTED | `components/truncated_text.rs` | `TruncatedText` |
| tab-bar.ts | 354 | PORTED | `components/tab_bar.rs` | `Tab`, `TabBarTheme`, `TabBar` |
| text.ts | 224 | PORTED | `components/text.rs` | `Text` |
| scroll-viewport.ts | 154 | PORTED | `components/scroll_viewport.rs` | all geometry helpers present |
| input.ts | 664 | PORTED | `components/input.rs` | `Input` |

### l/src/chrome — 32 files, 5 584 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| transcript-notice.ts (83), transcript-status.ts (58), visual-truncate.ts (70), transcript-container.ts (2032), status-notice.ts (100), streaming-panel.ts (96), tool-activity.ts (64), shared.ts (80), selector-helpers.ts (103), segment-track.ts (112), select-list-mouse-routing.ts (11), qrcode.ts (567), live-board.ts (123), local-date.ts (20), keybinding-hints.ts (80), error-block.ts (63), form-theme.ts (10), format.ts (78), dynamic-border.ts (69), countdown-timer.ts (97), chat-block.ts (110), collab-qrcode.ts (99) | 4125 | MISSING | — | no Rust counterpart |
| message-frame.ts | 214 | PARTIAL | `chrome/message.rs` | missing `FramedMessage`, `FramedRenderer`, `FramedMessageOptions`, `FramedMessageComponent` |
| message-notice.ts | 174 | PARTIAL | `chrome/message.rs` | missing `MessageNoticeContext`, `MessageNoticePresentation`, `MessageNoticeNativePresentation`, `MessageNoticeOptions` |
| message-divider.ts | 83 | PARTIAL | `chrome/message.rs` | missing `MessageDividerOptions` |
| index.ts | 22 | PARTIAL | `chrome/mod.rs` | barrel |
| diff.ts | 330 | PARTIAL | `chrome/diff.rs` | missing `nativeDiff` (NativeNode), `RenderDiffOptions` |
| context-thresholds.ts | 119 | PARTIAL | `chrome/context.rs` | missing `getContextUsageTone`, `getContextUsageThemeColor` |
| context-view-model.ts | 83 | PARTIAL | `chrome/context.rs` | missing `CONTEXT_CATEGORIES`, `ContextCategoryId`, `contextCategoryDescriptor`, `ContextThinkingGroup`, `ContextViewSourceMarker`, `ContextViewEntry`, `ContextViewCategory`, `ContextViewData` |
| context-map.ts | 62 | PARTIAL | `chrome/context.rs` | missing `ContextMapSegment` |
| context-presentation.ts | 28 | PARTIAL | `chrome/context.rs` | missing `CONTEXT_PRESENTATION`, `ContextPresentationId` |
| overlay-box.ts | 344 | PORTED | `chrome/overlay_box.rs` | doc header: port; all helpers + `PanelRows`/`PanelDivider` |

### l/src/chat — 42 files, 15 043 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| user-message.ts (576), transcript-render-helpers.ts (419), ttsr-notification.ts (166), transcript-outline.ts (287), transcript-presentation.ts (642), transcript-actions.ts (100), transcript-browser.ts (279), transcript-entry.ts (121), thinking-display.ts (374), todo-reminder.ts (63), svg-source.ts (191), table-chart.ts (284), spotlight.ts (514), stripped-tool-calls-placeholder.ts (43), svg-figure.ts (459), served-model-marker.ts (99), skill-message.ts (287), skill-title-input.ts (41), read-tool-group.ts (1449), recap-notice.ts (30), reply-write.ts (127), late-diagnostics-message.ts (175), messages.ts (312), reaction.ts (65), figure-markdown.ts (211), hook-message.ts (21), image-loading.ts (256), extension-types.ts (153), fence-figure.ts (85), custom-message.ts (25), display-preferences.ts (30), chat-transcript-builder.ts (694), collab-prompt-message.ts (46), compaction-summary-message.ts (344), background-tan-message.ts (39), cache-invalidation-marker.ts (111), advisor-message.ts (312), assistant-message.ts (2192) | 11 622 | MISSING | — | 38 files; no Rust counterpart |
| tool-execution.ts | 2377 | PARTIAL | `tools/tool_execution.rs` | missing tool-card component/renderers; `ToolExecutionState` present |
| bash-execution.ts | 503 | PARTIAL | `tools/bash_execution.rs` | missing `BashExecutionComponent`, `bashPtyViewport` |
| eval-execution.ts | 236 | PARTIAL | `tools/eval_execution.rs` | missing `EvalExecutionComponent` |
| execution-shared.ts | 305 | PARTIAL | `tools/bash_execution.rs`, `eval_execution.rs` | missing `PREVIEW_LINES`, `MAX_DISPLAY_LINE_CHARS`, `clampDisplayLine`, `ExecutionStatus`, `ExecutionColorKey`, `buildExecutionFrame`, `executionStatusLabel`, `buildStatusFooter`, `ExecutionCardInput`, `describeExecutionCard` |

### l/src/charts — 3 files, 11 266 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| chart-svg.ts (4654), table-data.ts (1567), chart-plan.ts (5045) | 11 266 | MISSING | — | no Rust counterpart |

### l/src/apps — 26 files, 11 800 lines

| reference path | lines | status | rust counterpart | notes |
|---|---|---|---|---|
| shell-visualizer.ts (167), standalone-picker.ts (165), ps-top.ts (854), session-picker.ts (107), setup-model-picker.ts (25), if-bench-board.ts (324), live-visualizer.ts (304), cleanse-board.ts (559), cleanse-picker.ts (65), autoresearch-dashboard.ts (1282), git/state.ts (125), git/help.ts (139), git/sidebar.ts (1703), git/diff-pane.ts (1590), git/git-tui.ts (1306), debug/raw-sse.ts (499), debug/protocol-probe.ts (258), debug/raw-sse-buffer.ts (457) | 9929 | MISSING | — | 18 files; no Rust counterpart |
| ps-data.ts | 116 | PARTIAL | `src/apps/ps_data.rs` | missing `PsTarget`, `TERMINAL_STATES`, `formatCommand`, `collapseCommand`, `daemonLabel`, `stateCell`, `flagsCell` |
| autoresearch-data.ts | 64 | PARTIAL | `src/apps/autoresearch_data.rs` | missing `findBaselineResult`, `findBaselineMetric`, `findBaselineRunNumber`, `findBaselineSecondary` |
| git/colors.ts | 98 | PARTIAL | `src/apps/git/colors.rs` | missing `isDark`, `canvasHex`, `textHex` (replaced by `GitPalette`) |
| debug/log-viewer.ts | 1219 | PARTIAL | `src/apps/debug/log_viewer.rs` | missing `LogViewerDeps`, `DebugLogViewerComponent` |
| git/avatar.ts | 32 | PORTED | `src/apps/git/avatar.rs` | doc header: port |
| debug/viewer-frame.ts | 174 | PORTED | `src/apps/debug/viewer_frame.rs` | doc header: port |
| debug/terminal-info.ts | 115 | PORTED | `src/apps/debug/terminal_info.rs` | doc header: port |
| debug/log-formatting.ts | 53 | PORTED | `src/apps/debug/log_formatting.rs` | doc header: port |

## Per-directory summary

| directory | files | lines | PORTED | PARTIAL | MISSING |
|---|---|---|---|---|---|
| l/src (root) | 41 | 25 029 | 15 | 15 | 11 |
| l/src/tools | 67 | 28 612 | 2 | 7 | 58 |
| l/src/theme | 13 | 5 277 | 4 | 6 | 3 |
| l/src/status-line | 15 | 7 547 | 1 | 9 | 5 |
| l/src/setup | 14 | 2 826 | 0 | 0 | 14 |
| l/src/render | 19 | 4 590 | 5 | 9 | 5 |
| l/src/prompt | 31 | 8 278 | 0 | 0 | 31 |
| l/src/overlays | 82 | 53 702 | 1 | 0 | 81 |
| l/src/native | 17 | 4 916 | 4 | 6 | 7 |
| l/src/multiplexers | 9 | 347 | 0 | 6 | 3 |
| l/src/html-form | 5 | 3 085 | 0 | 0 | 5 |
| l/src/components | 41 | 21 391 | 5 | 21 | 15 |
| l/src/chrome | 32 | 5 584 | 1 | 9 | 22 |
| l/src/chat | 42 | 15 043 | 0 | 4 | 38 |
| l/src/charts | 3 | 11 266 | 0 | 0 | 3 |
| l/src/apps | 26 | 11 800 | 4 | 4 | 18 |
| **total** | **457** | **209 293** | **42** | **96** | **319** |

Key gaps: entire `prompt/`, `setup/`, `charts/`, `html-form/`, `overlays/extensions/`, `components/composer/`, `components/{image,box,cancellable-loader}`, `render/{tree-list,link-hints,file-list,extension-links,code-cell}`, `native/{backend,icons,measure,overlay,picker,quiet,tone}`, and 57 `tools/` modules have no Rust counterpart. `crates/maple-tui/src/render/{row_moves,types}.rs` exist but are not declared in `render/mod.rs`.
