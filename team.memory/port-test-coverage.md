# Reference test → Rust port coverage map

Read-only reconnaissance. Scope: every `*.test.ts` under `l/test/` vs the Rust workspace
(`crates/maple-tui/src/**` + `src/**`).

## 1. Totals

| Classification | Count |
|---|---|
| **DONE** (subject ported + Rust tests exercise same behavior) | **71** |
| **PARTIAL** (subject ported, but behavior untested / partially ported / sibling module missing) | **88** |
| **MISSING** (subject module not ported at all) | **195** |
| **TOTAL** | **354** |

Exact total classified: **354** = 335 root `l/test/*.test.ts` + 19 `l/test/native/*.test.ts`.
(The task brief said 349; the tree actually contains 354. Root breakdown: a–d 67, e–h 42,
i–l 44, m–p 40, q–t 122, u–z 20. Native 19: backend, blobs, buttons-native, decor-blob,
encode, held-input, host-edit, host-focus, host-send, icons, optimistic, probe,
quiet-motion, reconcile, spotlight-notice, table-chart-native, transcript-compact-runs,
transcript-native, transcript-redesign-native.)

Non-test helpers were skipped: `chat-simple.ts`, `virtual-render-scheduler.ts`,
`virtual-terminal.ts`, `width-replay-provider.ts`, `test-theme-colors.ts`,
`test-themes.ts`, `render-stress-scheduler.ts`, `process-terminal-render-harness.ts`,
`key-tester.ts`, `gc.ts`, `native/tsp-harness.ts`, `helpers/*`, `fixtures/*`.

## 2. Method and classification rules

1. Listed all 354 test files; read each file's import lines to identify the subject
   module(s) in `l/src/`. `maple-tui/<sub>/<file>` → `l/src/<sub>/<file>.ts`;
   bare `maple-tui` → `l/src/index.ts` re-exports; `../src/...` → `l/src/...`.
2. Built the ported-module set from every `Reference:` / `Ported from` / `port of` header
   in `crates/maple-tui/src` and `src` (63 headers), plus the `#[cfg(test)]` inventory
   (90 Rust test modules).
3. **MISSING** = the subject module has no Rust counterpart (verified against the
   `l/src` module listing and the Rust file listing).
4. **PARTIAL** = the subject module exists in Rust but (a) the Rust module has no
   `#[cfg(test)]` block, (b) the port header explicitly omits the tested behavior, or
   (c) the test also depends on a sibling module that is not ported.
5. **DONE** = subject ported and the Rust module has a `#[cfg(test)]` block covering the
   same functions/values (per the SOP, assertions are ported, so name correspondence is
   not relied on).

Rust test anchors are cited as `file:line` of the `#[cfg(test)]` attribute.

## 3. DONE — 71 files

### render (15)
| File | Evidence |
|---|---|
| colorful-markdown | components/markdown.rs:1069 + theme/loader.rs:332 |
| deccara | deccara.rs:287 |
| sixel-crop | render/sixel.rs:97 |
| truncated-text | components/truncated_text.rs:58 |
| hyperlink | render/hyperlink.rs:415 |
| markdown, markdown-bplus-fast-path, markdown-incremental-lex, markdown-math, markdown-prose, markdown-table-align, markdown-table-wide-cell, markdown-tree-wrap | components/markdown.rs:1069 |
| width-aware-text | render/width_aware_text.rs:126 |
| unsynchronized-row-moves | render/row_moves.rs:178 |

### misc (28)
| File | Evidence |
|---|---|
| autocomplete | autocomplete.rs:1740 |
| debug-server | debug_server.rs:173 |
| desktop-notify | desktop_notify.rs:69 |
| fuzzy, fuzzy-cache, fuzzy-shadowed-occurrence | fuzzy.rs:805 |
| editor, editor-atoms, editor-text-assist, editor-top-border-provider | components/editor.rs:861 |
| input | components/input.rs:330 |
| keybindings, keybindings-migration, keys, issue-879-repro | keybindings.rs:245 (ports l/src/keys.ts + keybindings.ts) |
| lang-from-path | lang_from_path.rs:296 |
| latex-to-unicode | latex_to_unicode.rs:3079 |
| loader | components/loader.rs:115 |
| menu-window-nearest | components/menu_selection.rs:1045 |
| scroll-view | components/scroll_view.rs:629 |
| select-filter-breadcrumb, select-list | components/select_list.rs:729 |
| settings-list, settings-list-theme | components/settings_list.rs:938 |
| tab-bar | components/tab_bar.rs:447 |
| text | components/text.rs:103 |
| tree-view-deep, tree-view-linear-history | components/tree_view.rs:585 |

### status-line (8)
| File | Evidence |
|---|---|
| context-map | chrome/context.rs:683 (ports context-map.ts) |
| status-line-cache-hit | status_line/segments.rs:2044 (`CacheHit`) |
| status-line-cost-segment | status_line/segments.rs:2044 (`Cost`) |
| status-line-model | status_line/segments.rs:2044 (`Model`, `Stream`) |
| status-line-newline-guard | render/tool_card.rs:468 (`render_status_line`, ports render/status-line.ts) |
| status-line-path | status_line/segments.rs:2044 (`Path`) |
| status-line-profile | status_line/segments.rs:2044 (`Brand`) |
| status-line-vim | status_line/segments.rs:2044 (`Vim`) |

### terminal (7)
| File | Evidence |
|---|---|
| bracketed-paste | bracketed_paste.rs:110 |
| glyph-protocol | glyph_protocol.rs:180 |
| kitty-graphics, kitty-image-conversion | kitty_graphics.rs:238 |
| loop-watchdog | loop_watchdog.rs:95 |
| terminal-capabilities | terminal_capabilities.rs:1226,1619 |
| ttyid | ttyid.rs:59 |

### apps (3)
| File | Evidence |
|---|---|
| log-formatting | src/apps/debug/log_formatting.rs:75 |
| log-viewer | src/apps/debug/log_viewer.rs:654 |
| terminal-info | src/apps/debug/terminal_info.rs:157 |

### tools (2)
| File | Evidence |
|---|---|
| shell-viz, shell-viz-playback | tools/shell_viz.rs:739 |

### theme (2)
| File | Evidence |
|---|---|
| theme-color-mode | theme/color.rs:435 |
| theme-nerd-symbols | theme/symbols.rs:192 |

### windows (2)
| File | Evidence |
|---|---|
| windows-altgr | windows_altgr.rs:264 |
| windows-input-mode | windows_input.rs:457 |

### native (4)
| File | Evidence |
|---|---|
| blobs | native/blobs.rs:416 |
| encode | native/encode.rs:939 |
| host-edit | components/editor.rs:861 + components/input.rs:330 + native/node.rs:1267 |
| host-focus | components/editor.rs:861 + native/describe.rs:377 + native/node.rs:1267 |

## 4. MISSING — 195 files

### transcript (20)
| File | Subject module(s) not ported |
|---|---|
| background-tan-message | chat/background-tan-message.ts |
| cache-invalidation-marker | chat/cache-invalidation-marker.ts |
| chat-block | chrome/chat-block.ts |
| compaction-divider | chat/compaction-summary-message.ts |
| late-diagnostics-message | chat/late-diagnostics-message.ts |
| live-board | chrome/live-board.ts |
| reply-write | chat/reply-write.ts |
| served-model-marker | chat/served-model-marker.ts |
| snapcompact-shape-preview | overlays/snapcompact-shape-preview.ts |
| transcript-committed-release | chrome/transcript-container.ts + chat/* |
| transcript-compact-packing | chrome/transcript-container.ts |
| transcript-container | chrome/transcript-container.ts |
| transcript-image-links | chrome/transcript-container.ts + chat/* |
| transcript-live-entries-cache | chrome/transcript-container.ts |
| transcript-outline-row-cache | chrome/transcript-container.ts |
| transcript-render-helpers | chat/transcript-render-helpers.ts |
| transcript-seam-loss | chrome/transcript-container.ts |
| native/transcript-compact-runs | chrome/transcript-container.ts + native/{describe,node,reconcile,state}.ts |
| native/transcript-native | chat/* (assistant-message, bash-execution, eval-execution, …) |
| native/transcript-redesign-native | chat/{assistant-message,transcript-actions,user-message}.ts + chrome/status-notice.ts |

### tools (48)
| File | Subject module(s) not ported |
|---|---|
| apply-patch-preview-render | tools/edit.ts |
| ast-edit-render | tools/ast-edit.ts |
| ast-grep-control-path, ast-grep-render | tools/ast-grep.ts |
| code-frame-highlight | tools/grouped-file-output.ts + theme/theme.ts `highlightCode` |
| expanded-reveals-text | tools/{debug,default-renderer,lsp,mcp,task,web-search,yield}.ts |
| find-render | tools/find.ts |
| glob-render | tools/glob.ts |
| grep-render | tools/grep.ts |
| grouped-file-output-render | tools/grouped-file-output.ts |
| job-widget | tools/job-widget.ts |
| json-tree-render | tools/json-tree.ts |
| line-ranges | tools/line-ranges.ts |
| lsp-locations, lsp-status-rows, lsp-symbol-tree | tools/lsp.ts |
| memory-render | tools/memory.ts |
| path-renderer-invalid-args-render | tools/{edit,read,write}.ts |
| read-pending-path-render, read-render | tools/read.ts |
| task-artifact-rows-render, task-call-render, task-nested-live-render, task-progress-render, task-render | tools/task.ts |
| task-yield-shape-render | tools/task.ts + tools/yield.ts |
| think-render | tools/think.ts |
| tool-execution-custom-repaint, tool-execution-memoization, tool-execution-preview-coalesce, tool-execution-write-repaint | chat/tool-execution.ts |
| tool-execution-xdev-render | chat/tool-execution.ts + tools/xdev.ts |
| tool-result-control-sequences | chat/read-tool-group.ts + chat/tool-execution.ts + tools/read.ts |
| tools-core-native | tools/* core renderers |
| tools-native-edit | tools/edit.ts |
| tools-native-extra | tools/{todo,debug,ask,…}.ts |
| tools-native-github | tools/github.ts |
| tools-native-search | tools/web-search.ts |
| tools-native-web | tools/web-search.ts |
| vibe-render | tools/vibe.ts |
| wait-render | tools/wait.ts + tools/proc-render.ts |
| web-search-render | tools/web-search.ts |
| write-figure, write-pending-path-render, write-streaming-incremental-render | tools/write.ts |
| xd-url | tools/xd-url.ts |

### render (12)
| File | Subject module(s) not ported |
|---|---|
| countdown-timer | chrome/countdown-timer.ts |
| dynamic-border | chrome/dynamic-border.ts |
| extension-links | render/extension-links.ts |
| image-budget, image-clip, image-render | components/image.ts |
| latex-block | latex-block.ts (latex_to_unicode.rs ports only latex-to-unicode.ts) |
| output-pane-memo | render/output-pane.ts |
| qrcode | chrome/qrcode.ts |
| segment-track | chrome/segment-track.ts |
| tool-hanging-wrap | render/hanging-wrap.ts |
| visual-truncate | chrome/visual-truncate.ts |

### native (7)
| File | Subject module(s) not ported |
|---|---|
| native/buttons-native | native/overlay.ts |
| native/decor-blob | prompt/composer-attachments.ts + prompt/custom-editor.ts + theme/theme.ts |
| native/quiet-motion | native/{apply,quiet,reconcile}.ts + motion-effects.ts |
| native/reconcile | native/{apply,reconcile,settle}.ts |
| native/spotlight-notice | chat/{read-tool-group,tool-execution,transcript-actions,transcript-presentation}.ts + chrome/transcript-container.ts + overlays/spotlight-sheet.ts |
| native/table-chart-native | chat/{assistant-message,table-chart}.ts (native/state.rs is ported) |

### chat (38)
| File | Subject module(s) not ported |
|---|---|
| assistant-message-link-targets | chat/assistant-message.ts + components/image.ts + prompt/interactive-context-helpers.ts |
| assistant-message-narration, assistant-message-prose-color | chat/assistant-message.ts |
| assistant-text-renderers | chat/assistant-message.ts + chat/transcript-render-helpers.ts |
| attachment-chips | prompt/attachment-chips.ts |
| bash-execution-clamp, bash-execution-pty, bash-execution-sixel | chat/bash-execution.ts |
| composer-cache | prompt/composer-cache.ts |
| composer-click, composer-inline-shrink, composer-native, composer-shape-preview, composer-welcome-scrollback | prompt/composer.ts (+ chrome/transcript-container.ts, overlays/composer-shape-preview.ts, prompt/welcome.ts) |
| custom-editor, custom-editor-buffered-double-esc, custom-editor-keybindings, custom-editor-plugin-ctor, custom-editor-vim | prompt/custom-editor.ts (+ vim.ts) |
| emoji-autocomplete | prompt/emoji-autocomplete.ts |
| execution-border-status | chat/bash-execution.ts + chat/eval-execution.ts |
| github-ref-autocomplete | prompt/github-ref-autocomplete.ts |
| image-references | prompt/composer-attachments.ts + prompt/image-references.ts |
| live-tool-session | overlays/extensions/live-tool-session.ts |
| macos-spelling | prompt/macos-spelling.ts |
| prompt-action-autocomplete | prompt/prompt-action-autocomplete.ts |
| prompt-start-write | chat/user-message.ts |
| prose-only-thinking | chat/thinking-display.ts |
| provider-error-expand | chat/assistant-message.ts |
| thinking | thinking.ts |
| thinking-display | chat/thinking-display.ts |
| typing-word | prompt/typing-word.ts |
| welcome, welcome-tip | prompt/welcome.ts |
| word-completion | prompt/word-completion.ts |

### overlays (49)
| File | Subject module(s) not ported |
|---|---|
| agent-hub-native, agent-hub-projection, agents-hub | overlays/agent-hub.ts, overlays/agent-hub-projection.ts, overlays/agents-hub.ts |
| annotation-editor-boundary, annotation-editor-fullscreen, annotation-filename-reveal, annotation-overlay | overlays/annotation-overlay.ts |
| ask-dialog | overlays/ask-dialog.ts |
| advisor-config, advisor-config-layout | overlays/advisor-config.ts |
| btw-history-panel-branch, btw-history-panel-cursor, btw-history-panel-wheel | overlays/btw-history-panel.ts |
| btw-inject | overlays/btw-inject.ts |
| context-viewer, context-viewer-overview | overlays/context-viewer.ts, overlays/context-viewer-overview.ts |
| copy-targets | overlays/copy-targets.ts |
| dialogs-native | overlays/{ask,approval,login,plan-review}-dialog.ts |
| handle-input-or-escape | overlays/plugin-settings.ts |
| hook-input-timeout | overlays/hook-input.ts |
| hook-selector-overflow, hook-selector-slider | overlays/hook-selector.ts |
| html-form | html-form/{data,media,dialog,paginate,parse}.ts |
| jobs-panel | overlays/jobs-panel.ts |
| link-browser | overlays/link-browser.ts |
| login-dialog-manual-code | overlays/login-dialog.ts |
| mcp-add-wizard-native | overlays/mcp-add-wizard.ts |
| model-hub-assign-scope, model-hub-roles-search | overlays/model-hub.ts |
| model-picker-native | overlays/model-hub.ts + overlays/model-picker.ts |
| overlay-native-select | overlays/logout-account-selector.ts |
| plan-review-overlay | overlays/plan-review-overlay.ts |
| plan-toc | overlays/plan-toc.ts |
| report-panel | overlays/report-panel.ts + chrome/transcript-container.ts |
| review-overlays-native | overlays/{annotation-overlay,plan-review-overlay}.ts |
| selector-picker-native | overlays/{hook,oauth,theme,thinking,session-account,reset-usage}-selector.ts |
| session-date-locale | overlays/session-selector.ts `systemDateLocale` |
| settings-prefs-native | overlays/{plugin-settings,settings-defs,settings-selector}.ts |
| setup-model-native | apps/setup-model-picker.ts |
| setup-native, setup-wizard-sign-in | setup/{wizard-overlay,startup-splash,scenes/*}.ts |
| show-images-selector-mouse | overlays/show-images-selector.ts |
| timeline-picker-native | overlays/{copy-selector,rewind-selector,tree-selector}.ts + chat/transcript-entry.ts |
| usage-dashboard, usage-native-describe | overlays/{usage-dashboard,usage-row,session-info-overlay}.ts |
| wrapper-selector-mouse-offset | overlays/{plugin,queue-mode,theme,thinking}-selector.ts |

### apps (9)
| File | Subject module(s) not ported |
|---|---|
| autoresearch-dashboard-native | apps/autoresearch-dashboard.ts (only apps/autoresearch_data.rs:183 ported) |
| extension-dashboard-native | overlays/extensions/extension-dashboard.ts |
| extension-dashboard-state | overlays/extensions/state-manager.ts |
| git-commit-view-hints | apps/git/{git-tui,sidebar,state}.ts (only git/colors.rs:153 + git/avatar.rs:69 ported) |
| live-visualizer | apps/live-visualizer.ts |
| mcp-runtime-snapshot | overlays/extensions/mcp-runtime.ts |
| raw-sse-buffer | apps/debug/raw-sse-buffer.ts |
| raw-sse-pretty | apps/debug/raw-sse.ts |
| session-picker-native | apps/session-picker.ts + overlays/{history-search,session-selector}.ts + key-hint-format.ts |

### status-line (4)
| File | Subject module(s) not ported |
|---|---|
| context-usage | status-line/context-usage.ts |
| status-line-git-utils | status-line/git-utils.ts |
| status-line-loop | status-line/loop.ts |
| status-line-native | status-line/{startup,host,metrics,presets,loop,footer,prompt-cache}.ts |

### theme (4)
| File | Subject module(s) not ported |
|---|---|
| mermaid-cache | theme/mermaid-cache.ts |
| session-color | theme/session-color.ts |
| shimmer | theme/shimmer.ts |
| theme-epoch-fallback | theme/theme.ts `getThemeEpoch` |

### terminal (2)
| File | Subject module(s) not ported |
|---|---|
| resize-anchor-recovery | kitty-vt-wasm `KittyTerminal` (not in Rust; only kitty_graphics.rs:238) |
| stdin-buffer | stdin-buffer.ts |

### windows (0)
No MISSING files.

### misc (2)
| File | Subject module(s) not ported |
|---|---|
| box-border | components/box.ts |
| hotkeys-markdown | hotkeys-markdown.ts |

## 5. PARTIAL — 88 files

### terminal (39)
| File | Ported evidence | Gap |
|---|---|---|
| adaptive-render-backpressure | tui.rs | no `#[cfg(test)]` |
| alt-frame-row-diff | tui.rs | no tests |
| cursor-visibility-dedupe | tui.rs | no tests |
| destructive-reset-clear-order | tui.rs | no tests |
| drain-input-da1-sentinel | terminal/process.rs | no tests |
| emergency-restore-altscreen | tui.rs + terminal/process.rs | no tests |
| focus-menu-regression | tui.rs (components/menu_selection.rs:1045 ported) | TUI focus path untested |
| herdr-sync-output | tui.rs | no tests |
| history-frame-plan | tui.rs | no tests |
| inband-resize-introducer-split | terminal/process.rs | no tests |
| input-priority | tui.rs | no tests |
| input-render-scheduling | tui.rs | no tests |
| issue-2045-repro | terminal/process.rs | no tests |
| issue-8318-repro | terminal/process.rs | no tests |
| issue-8542-repro | terminal/process.rs | no tests |
| loop-watchdog-wiring | loop_watchdog.rs:95 | TUI wiring untested |
| mouse | mouse.rs (mouse_sgr.rs:63 ported) | mouse.rs has no tests |
| mouse-tracking | tui.rs | no tests |
| notifications | desktop_notify.rs:69 | terminal/terminal-capabilities integration untested |
| output-backpressure-gate | tui.rs | no tests |
| paint-listener | tui.rs | no tests |
| process-terminal-headless | terminal/process.rs | no tests |
| process-terminal-render | terminal/process.rs | no tests |
| program-status-probe | terminal/process.rs + multiplexers.rs:90 | process.rs untested |
| protocol-probe | kitty_graphics.rs:238 + terminal_capabilities.rs:1226 | terminal probing untested |
| resize-alt-toggle-echo | tui.rs | no tests |
| resize-multiplexer-anchor | tui.rs + multiplexers.rs:90 | tui.rs untested |
| resize-preserved-clear | tui.rs | no tests |
| resize-settle-fused-exit | tui.rs | no tests |
| resize-tmux-sync | tui.rs + multiplexers.rs:90 | tui.rs untested |
| start-listener | tui.rs | no tests |
| startup-probe-leak | tui.rs + terminal/process.rs | no tests |
| stdout-stall-watchdog | terminal/process.rs | no tests |
| terminal-appearance | keybindings.rs:245 + terminal/process.rs | process.rs untested |
| terminal-background | tui.rs + render/sixel.rs:97 | tui.rs untested |
| terminal-oversized-frame | tui.rs + terminal/process.rs | no tests |
| terminal-write-log | terminal/process.rs | no tests |
| terminal-write-loop-phase | terminal/process.rs | no tests |
| terminal-disconnect-raw-mode-throw | terminal/process.rs | no tests |

### render (13)
| File | Ported evidence | Gap |
|---|---|---|
| issue-848-repro | text_shape.rs (ports utils `truncateToWidth`) | text_shape.rs has no tests |
| line-width-sidecar | text_shape.rs | no tests |
| output-block | render/output_block.rs:593 + chrome/overlay_box.rs:589 | render/code-cell.ts not ported |
| render-utils | render/render_utils.rs:982 | app-keybindings.ts not ported |
| sgr-coalesce | tui.rs (`coalesceAdjacentSgr`, l/src/tui.ts:779) | tui.rs has no tests |
| sixel-block-repaint | render/sixel.rs:97 | tui/terminal integration untested |
| sixel-probe | render/sixel.rs:97 + kitty_graphics.rs:238 | terminal probing untested |
| strip-terminal-sequences | text_shape.rs | no tests |
| terminal-jamo-width-probe | text_shape.rs | no tests |
| text-utils | text_shape.rs | no tests |
| truncate-to-width | text_shape.rs | no tests |
| visible-width | text_shape.rs | no tests |
| wrap-ansi | text_shape.rs | no tests |

### theme (5)
| File | Ported evidence | Gap |
|---|---|---|
| theme-auto-detection | theme/loader.rs:332 + terminal_capabilities.rs:1226 | theme/theme.ts auto-detection missing |
| theme-extension-link-color | theme/loader.rs:332 + components/markdown.rs:1069 | render/extension-links.ts missing |
| theme-fg-nesting | theme/theme_class.rs:1515 + theme/loader.rs:332 | theme/theme.ts missing |
| theme-highlight-diff-parity | chrome/diff.rs:594 | theme/theme.ts `highlightCode` missing |
| theme-spinner-frames | theme/symbols.rs:192 | spinner_clock.rs has no tests |

### native (8)
| File | Ported evidence | Gap |
|---|---|---|
| interactive-native | components editor/input/loader/select-list/settings-list + native/node.rs:1267 + native/state.rs:1414 | chrome/countdown-timer.ts, prompt/custom-editor.ts, prompt/magic-keywords.ts missing |
| native/backend | native/describe.rs:377 + native/node.rs:1267 + native/state.rs:1414 | native/reconcile.ts + native/settle.ts missing |
| native/held-input | terminal_capabilities.rs:1226 | tui integration untested |
| native/host-send | native/encode.rs:939 | native/reconcile.ts + prompt/custom-editor.ts missing |
| native/icons | native/describe.rs:377 + native/node.rs:1267 | native/reconcile.ts missing |
| native/optimistic | native/describe.rs:377 + native/node.rs:1267 + native/state.rs:1414 | native/reconcile.ts missing |
| native/probe | native/encode.rs:939 + multiplexers.rs:90 | terminal/process.rs untested |
| native-content | components/{disclosure:181,markdown:1069,table:517,text:103}.rs | components/box.ts missing |

### status-line (3)
| File | Ported evidence | Gap |
|---|---|---|
| spinner-phase | components/loader.rs:115 + status_line/component.rs:968 + spinner_clock.rs | spinner_clock.rs untested; chat/tool-execution.ts + status-line/host.ts missing |
| status-line-active-window-repaint | status_line/component.rs:968 + native/state.rs:1414 | status-line/host.ts missing; spinner_clock.rs untested |
| status-line-pin | status_line/component.rs:968 | status-line/host.ts + theme/session-color.ts missing |

### tools (7)
| File | Ported evidence | Gap |
|---|---|---|
| bash-render | tools/bash_execution.rs:371 + tools/shell_viz.rs:739 + render/render_utils.rs:982 | bash_execution.rs header omits syntax highlighting, output-pane minimisation, native run box |
| eval-agent-progress-render | tools/eval_execution.rs:178 + render/render_utils.rs:982 | eval-format tokenizer not ported |
| eval-code-preview-render | tools/eval_execution.rs:178 | eval-format tokenizer not ported |
| eval-output-views | tools/eval_execution.rs:178 | eval-format tokenizer not ported |
| eval-status-event-cap | tools/eval_execution.rs:178 | eval-format tokenizer not ported |
| tools-default-card | tools/tool_execution.rs:197 (ports tools/renderer.ts + render/tool-card.ts) | tools/default-renderer.ts not ported |
| tools-native-shell | tools/bash_execution.rs:371 + tools/shell_viz.rs:739 | syntax highlighting / output-pane / native run box omitted |

### overlays (2)
| File | Ported evidence | Gap |
|---|---|---|
| overlay-focus | tui.rs | no tests |
| overlay-scroll | tui.rs | no tests |

### apps (2)
| File | Ported evidence | Gap |
|---|---|---|
| apps-native | src/apps/ps_data.rs:308 | cleanse-board, git/git-tui, git/state, ps-top, tools/daemon missing |
| shell-trace | tools/shell_viz.rs:739 | apps/shell-visualizer.ts + tools/shell-trace.ts missing |

### windows (3)
| File | Ported evidence | Gap |
|---|---|---|
| issue-2034-repro | terminal/process.rs | no tests (ConPTY) |
| kitty-keyboard-da1-ordering | keybindings.rs:245 | ProcessTerminal integration untested |
| resize-conpty-warp | tui.rs + terminal/process.rs | no tests |

### misc (6)
| File | Ported evidence | Gap |
|---|---|---|
| container-dispose | tui.rs (`Container`) | tui.rs has no tests |
| container-memo | tui.rs + components/container.rs:66 | tui.rs untested; components/box.ts missing |
| editor-autocomplete-actions | components/editor.rs:861 | prompt/emoji-autocomplete.ts missing |
| editor-vim | components/editor.rs:861 | vim.ts missing |
| keybindings-display | keybindings.rs:245 | app-keybindings.ts missing |
| tight-layout | components/{editor:861,markdown:1069,text:103}.rs | components/box.ts missing; utils untested |

## 6. Highest-leverage next file per group

| Group | Next file | One-line reason |
|---|---|---|
| transcript | `transcript-container.test.ts` | chrome/transcript-container is the shared transcript surface; 8 root tests + 3 native tests depend on it. |
| tools | `tools-core-native.test.ts` | exercises the shared tool renderer used by most tools/* modules; unblocks the largest MISSING cluster. |
| render | `image-render.test.ts` | components/image is a whole missing component depended on by image-budget, image-clip, image-references, transcript-image-links. |
| native | `native/reconcile.test.ts` | native/reconcile is the core native engine; unblocks optimistic, icons, host-send, backend, quiet-motion. |
| chat | `composer-native.test.ts` | prompt/composer is the input surface; unblocks composer-cache/click/inline-shrink/shape-preview/welcome-scrollback. |
| overlays | `dialogs-native.test.ts` | ask/approval/login/plan-review dialogs are the shared modal primitives. |
| apps | `apps-native.test.ts` | already PARTIAL; finishing cleanse-board/git-tui/ps-top/daemon completes the apps area. |
| status-line | `status-line-native.test.ts` | covers startup/host/loop/metrics segments; unblocks the remaining status-line cluster. |
| theme | `theme-epoch-fallback.test.ts` | theme/theme.ts singleton (`getThemeEpoch`) is missing and underlies many theme tests. |
| terminal | `stdin-buffer.test.ts` | stdin-buffer.ts is a foundational missing module for the terminal input path. |
| windows | `resize-conpty-warp.test.ts` (PARTIAL) | only ConPTY-specific resize test; tui/terminal tests are absent. |
| misc | `box-border.test.ts` | components/box.ts is missing and blocks container-memo and tight-layout. |

## 7. Cheapest wins — subject already ported, assertions clearly not covered

These are PARTIAL entries whose subject module **exists in Rust but has no `#[cfg(test)]`
block at all**, so porting the reference assertions is pure addition:

1. **`text_shape.rs` has no tests** → `visible-width`, `truncate-to-width`, `wrap-ansi`,
   `text-utils`, `strip-terminal-sequences`, `line-width-sidecar`,
   `terminal-jamo-width-probe`, `issue-848-repro`. (8 files, all utils shaping.)
2. **`tui.rs` has no tests** → `resize-preserved-clear`, `resize-tmux-sync`,
   `resize-alt-toggle-echo`, `resize-settle-fused-exit`, `resize-multiplexer-anchor`,
   `paint-listener`, `cursor-visibility-dedupe`, `destructive-reset-clear-order`,
   `adaptive-render-backpressure`, `alt-frame-row-diff`, `input-priority`,
   `input-render-scheduling`, `output-backpressure-gate`, `start-listener`,
   `startup-probe-leak`, `history-frame-plan`, `herdr-sync-output`,
   `focus-menu-regression`, `terminal-oversized-frame`, `terminal-background`,
   `overlay-focus`, `overlay-scroll`. (21 files.)
3. **`terminal/process.rs` has no tests** → `process-terminal-headless`,
   `process-terminal-render`, `terminal-write-log`, `terminal-write-loop-phase`,
   `terminal-disconnect-raw-mode-throw`, `stdout-stall-watchdog`,
   `drain-input-da1-sentinel`, `issue-8542-repro`, `issue-8318-repro`,
   `issue-2045-repro`, `inband-resize-introducer-split`, `program-status-probe`,
   `notifications`, `mouse-tracking`, `mouse`. (14 files; `mouse.rs` also untested.)
4. **`spinner_clock.rs` has no tests** → `spinner-phase`, `theme-spinner-frames`,
   `status-line-active-window-repaint`. (3 files.)
5. **`components/box.ts` missing** (small) → `box-border`, `container-memo`,
   `tight-layout`, `native-content`. (4 files.)
6. **Ported but behavior explicitly omitted** → `bash-render` + `tools-native-shell`
   (syntax highlighting / output-pane / native run box), `eval-*` (JS/Python tokenizer),
   `output-block` (code-cell), `render-utils` (app-keybindings),
   `tools-default-card` (default-renderer), `theme-extension-link-color`
   (extension-links), `theme-highlight-diff-parity` (theme `highlightCode`),
   `theme-auto-detection` / `theme-fg-nesting` (theme/theme.ts),
   `keybindings-display` (app-keybindings), `editor-autocomplete-actions`
   (emoji-autocomplete), `editor-vim` (vim.ts), `apps-native` / `shell-trace`
   (remaining apps modules).

## 8. Appendix — ported-module map (evidence)

Rust modules with `#[cfg(test)]` (90) and their reference source:

- Root: active_terminal.rs:64, autocomplete.rs:1740, bracketed_paste.rs:110,
  debug_server.rs:173, deccara.rs:287, desktop_notify.rs:69, frame_rate.rs:46,
  fuzzy.rs:805, glyph_protocol.rs:180, keybindings.rs:245 (keys.ts + keybindings.ts),
  kitty_graphics.rs:238, lang_from_path.rs:296, latex_to_unicode.rs:3079,
  loop_watchdog.rs:95, mouse_sgr.rs:63, multiplexers.rs:90,
  terminal_capabilities.rs:1226/1619, ttyid.rs:59, windows_altgr.rs:264,
  windows_input.rs:457.
- render/: hyperlink.rs:415, output_block.rs:593, render_utils.rs:982,
  row_moves.rs:178, sixel.rs:97, tool_card.rs:468, types.rs:28,
  width_aware_text.rs:126.
- components/: container.rs:66, disclosure.rs:181, editor.rs:861, form.rs:799,
  input.rs:330, key_value_list.rs:286, loader.rs:115, markdown.rs:1069,
  menu_selection.rs:1045, metric_row.rs:248, panel.rs:101, progress_bar.rs:282,
  scroll_view.rs:629, scroll_viewport.rs:636, section.rs:241, select_list.rs:729,
  settings_list.rs:938, spacer.rs:30, sparkline.rs:321, tab_bar.rs:447,
  table.rs:517, text.rs:103, tree_view.rs:585, truncated_text.rs:58,
  wizard_step.rs:528, layout/{geometry:70, row:54, split_pane:100, stack:38}.
- status_line/: component.rs:968, schema.rs:879, segments.rs:2044, separators.rs:132.
- theme/: builtin.rs:264, color.rs:435, loader.rs:332, schema.rs:839,
  symbols.rs:192, theme_class.rs:1515.
- tools/: bash_execution.rs:371, eval_execution.rs:178, shell_viz.rs:739,
  tool_execution.rs:197.
- chrome/: context.rs:683, diff.rs:594, message.rs:467, overlay_box.rs:589.
- native/: blobs.rs:416, describe.rs:377, encode.rs:939, node.rs:1267,
  spans.rs:521, state.rs:1414.
- src/apps/: ps_data.rs:308, autoresearch_data.rs:183, git/colors.rs:153,
  git/avatar.rs:69, debug/{log_formatting:75, log_viewer:654, terminal_info:157,
  viewer_frame:298}.

Rust modules with **no** `#[cfg(test)]` (PARTIAL risk): terminal/process.rs,
terminal/virt.rs, tui.rs, mouse.rs, component.rs, ansi_scan.rs, style.rs,
text_shape.rs, symbols.rs, keys.rs, trigger.rs, spinner_clock.rs, screen.rs,
and all `*/mod.rs` files.

Reference modules confirmed **absent** from Rust (MISSING drivers): chrome/*
(transcript-container, visual-truncate, qrcode, segment-track, countdown-timer,
dynamic-border, chat-block, live-board, status-notice, streaming-panel, tool-activity,
keybinding-hints, error-block, form-theme, format, local-date, collab-qrcode,
select-list-mouse-routing, selector-helpers, context-view-model, transcript-notice,
transcript-status, shared);
components/{image,cancellable-loader,box}.ts + components/composer/*;
prompt/* (all); chat/* (all); tools/* except shell-viz, eval, bash, renderer;
overlays/* (all) + overlays/extensions/*; setup/*; html-form/*;
status-line/{startup,loop,metrics,host,git-utils,context-usage,footer,prompt-cache}.ts;
native/{tone,reconcile,settle,quiet,overlay,picker,memo,icons,measure,backend,apply}.ts;
theme/{theme,tui-adapters,session-color,shimmer,mermaid-cache,active-symbols}.ts;
render/{code-cell,output-pane,link-hints,extension-links,tree-list,file-list,
hanging-wrap,composite}.ts;
root {latex-block,stdin-buffer,vim,thinking,hotkeys-markdown,key-hint-format,
keybinding-matchers,motion-effects,space-hold,kill-ring,editor-component,tmux,sixel}.ts;
apps/* except ps-data, autoresearch-data, git/colors, git/avatar, debug/*.
