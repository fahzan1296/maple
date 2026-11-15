# Behavioral spec: `l/src/{theme,render,chrome,tools,status-line,native,overlays,prompt,setup,html-form,charts}` → `crates/maple-tui`

Conventions: TS `string` = Unicode scalar text unless noted; `visibleWidth` = terminal cell width (wide CJK/emoji count 2); all ANSI is SGR. File citations are `path:line`. Items marked **[INFERENCE]** were not directly observed.

---

## 1. `theme/`

### 1.1 Schema (`theme/schema.ts`)

```ts
type ColorValue = string | number;   // "#rgb"/"#rrggbb"/"#rrggbbaa", "", or 0-255 palette index
type ColorMode  = "truecolor" | "256color";
```

`ANSI_COLOR_INDEX` (`schema.ts:10-26`): `black0 red1 green2 yellow3 blue4 magenta5 cyan6 white7 brightBlack8 brightRed9 brightGreen10 brightYellow11 brightBlue12 brightMagenta13 brightCyan14 brightWhite15`; `type AnsiColor = keyof`.

`OPTIONAL_THEME_COLOR_RECORD` (`schema.ts:18-22`): `{ assistantMessageText, mdExtensionLink }` — tokens a theme may omit. `Theme.fg()` still resolves them (to terminal default), `Theme.hasColor()` reports them unset. `mdExtensionLink` is special: `createTheme` derives it.

`ThemeJson` (`schema.ts:24-49`):
- `$schema?: string`, `name: string`
- `vars?: Record<string, ColorValue>` — named color refs, resolved recursively
- `colors`: every `ThemeColor`/`ThemeBg` except `thinkingMax` and the two optional tokens required; those three optional; `thinkingMax?: ColorValue`
- `export?: { pageBg?, cardBg?, infoBg? }`
- `terminal?: { background?, foreground?, chrome?, widget?, ansi?: ColorValue[] /* exactly 16 */ }`
- `symbols?: { preset?: "unicode"|"nerd"|"ascii"; overrides?: Record<string,string>; spinnerFrames?: string[] | {status?: string[]; activity?: string[]} }`

`ThemeColor` union, in order (`schema.ts:52-98`): `accent, border, borderAccent, borderMuted, success, error, warning, muted, dim, text, assistantMessageText, thinkingText, userMessageText, customMessageText, customMessageLabel, toolTitle, toolOutput, mdHeading, mdLink, mdExtensionLink, mdLinkUrl, mdCode, mdCodeBlock, mdCodeBlockBorder, mdQuote, mdQuoteBorder, mdHr, mdListBullet, toolDiffAdded, toolDiffRemoved, toolDiffContext, syntaxComment, syntaxKeyword, syntaxFunction, syntaxVariable, syntaxString, syntaxNumber, syntaxType, syntaxOperator, syntaxPunctuation, thinkingOff, thinkingMinimal, thinkingLow, thinkingMedium, thinkingHigh, thinkingXhigh, thinkingMax, bashMode, pythonMode, statusLineSep, statusLineModel, statusLinePath, statusLineGitClean, statusLineGitDirty, statusLineContext, statusLineSpend, statusLineStaged, statusLineDirty, statusLineUntracked, statusLineOutput, statusLineCost, statusLineSubagents`.

`ThemeBg` (`schema.ts:148-154`): `selectedBg, userMessageBg, customMessageBg, toolPendingBg, toolSuccessBg, toolErrorBg, statusLineBg`.

`isValidThemeColor`, `isValidThemeBg`: set-membership guards.

### 1.2 Built-in themes and exact defaults

`BUILTIN_THEMES` (`loader.ts:19-24`) = `{ dark: dark.json, light: light.json, ...defaults.json }`. `defaults.json` is a single object with **100** keys (exact names, in file order):

`alabaster, amethyst, anthracite, basalt, birch, dark-abyss, dark-arctic, dark-aurora, dark-catppuccin, dark-cavern, dark-celestial, dark-copper, dark-cosmos, dark-cyberpunk, dark-dracula, dark-eclipse, dark-ember, dark-equinox, dark-forest, dark-github, dark-gruvbox, dark-lavender, dark-lunar, dark-midnight, dark-monochrome, dark-monokai, dark-nebula, dark-neon-noir, dark-nord, dark-ocean, dark-one, dark-poimandres, dark-rainforest, dark-reef, dark-retro, dark-rose-pine, dark-sakura, dark-slate, dark-solarized, dark-solstice, dark-starfall, dark-sunset, dark-swamp, dark-synthwave, dark-taiga, dark-terminal, dark-tokyo-night, dark-tundra, dark-twilight, dark-volcanic, graphite, light-arctic, light-aurora-day, light-canyon, light-catppuccin, light-cirrus, light-coral, light-cyberpunk, light-dawn, light-dunes, light-eucalyptus, light-forest, light-frost, light-github, light-glacier, light-gruvbox, light-haze, light-honeycomb, light-lagoon, light-lavender, light-meadow, light-mint, light-monochrome, light-ocean, light-one, light-opal, light-orchard, light-paper, light-poimandres, light-prism, light-retro, light-sand, light-savanna, light-solarized, light-soleil, light-sunset, light-synthwave, light-tokyo-night, light-wetland, light-zenith, limestone, mahogany, marble, obsidian, onyx, pearl, porcelain, quartz, sandstone, titanium`.

`dark.json` (exact):

- `vars`: `cyan #0088fa, blue #178fb9, green #89d281, red #fc3a4b, yellow #e4c00f, gray #777d88, dimGray #5f6673, darkGray #3d424a, accent #febc38, selectedBg #31363f, userMsgBg #221d1a, toolPendingBg #1d2129, toolSuccessBg #161a1f, toolErrorBg #291d1d, customMsgBg #2a2530`
- `colors` (value = var name or literal): `accent accent; border blue; borderAccent cyan; borderMuted darkGray; success green; error red; warning yellow; muted gray; dim dimGray; text ""; thinkingText gray; selectedBg selectedBg; userMessageBg userMsgBg; userMessageText ""; customMessageBg customMsgBg; customMessageText ""; customMessageLabel #b281d6; toolPendingBg toolPendingBg; toolSuccessBg toolSuccessBg; toolErrorBg toolErrorBg; toolTitle ""; toolOutput gray; mdHeading #febc38; mdLink #0088fa; mdLinkUrl dimGray; mdCode #e5c1ff; mdCodeBlock #9CDCFE; mdCodeBlockBorder gray; mdQuote gray; mdQuoteBorder darkGray; mdHr darkGray; mdListBullet accent; toolDiffAdded green; toolDiffRemoved red; toolDiffContext gray; link #0088fa` ← **not a `ThemeColor`** (extra key, ignored by `Theme`); `syntaxComment #6A9955; syntaxKeyword #569CD6; syntaxFunction #DCDCAA; syntaxVariable #9CDCFE; syntaxString #CE9178; syntaxNumber #B5CEA8; syntaxType #4EC9B0; syntaxOperator #D4D4D4; syntaxPunctuation #D4D4D4; thinkingOff darkGray; thinkingMinimal dimGray; thinkingLow #178fb9; thinkingMedium #0088fa; thinkingHigh #b281d6; thinkingXhigh #e5c1ff; bashMode cyan; statusLineBg #121212; statusLineSep 244; statusLineModel #d787af; statusLinePath #00afaf; statusLineGitClean #5faf5f; statusLineGitDirty #d7af5f; statusLineContext #8787af; statusLineSpend #5fafaf; statusLineStaged 70; statusLineDirty 178; statusLineUntracked 39; statusLineOutput 205; statusLineCost 205; statusLineSubagents accent; pythonMode yellow`. (`thinkingMax` absent.)
- `export`: `pageBg #18181e, cardBg #1e1e24, infoBg #26262e`
- `terminal`: `background #18181e, foreground #cccccc, chrome #121212, widget #1e1e24`, `ansi` = `["#000000","#cd3131","#0dbc79","#e5e510","#2472c8","#bc3fbc","#11a8cd","#e5e5e5","#666666","#f14c4c","#23d18b","#f5f543","#3b8eea","#d670d6","#29b8db","#e5e5e5"]`

`light.json` (exact):

- `vars`: `teal #5a8080, blue #547da7, green #588458, red #aa5555, yellow #9a7326, mediumGray #6c6c6c, dimGray #767676, lightGray #b0b0b0, selectedBg #d0d0e0, userMsgBg #e8e8e8, toolPendingBg #e8e8f0, toolSuccessBg #e8f0e8, toolErrorBg #f0e8e8, customMsgBg #ede7f6`
- `colors`: `accent teal; border blue; borderAccent teal; borderMuted lightGray; success green; error red; warning yellow; muted mediumGray; dim dimGray; text ""; thinkingText mediumGray; selectedBg selectedBg; userMessageBg userMsgBg; userMessageText ""; customMessageBg customMsgBg; customMessageText ""; customMessageLabel #7e57c2; toolPendingBg toolPendingBg; toolSuccessBg toolSuccessBg; toolErrorBg toolErrorBg; toolTitle ""; toolOutput mediumGray; mdHeading yellow; mdLink blue; mdLinkUrl dimGray; mdCode teal; mdCodeBlock green; mdCodeBlockBorder mediumGray; mdQuote mediumGray; mdQuoteBorder mediumGray; mdHr mediumGray; mdListBullet green; toolDiffAdded green; toolDiffRemoved red; toolDiffContext mediumGray; syntaxComment #008000; syntaxKeyword #0000FF; syntaxFunction #795E26; syntaxVariable #001080; syntaxString #A31515; syntaxNumber #098658; syntaxType #267F99; syntaxOperator #000000; syntaxPunctuation #000000; thinkingOff lightGray; thinkingMinimal #767676; thinkingLow blue; thinkingMedium teal; thinkingHigh #875f87; thinkingXhigh #8b008b; bashMode green; statusLineBg #e0e0e0; statusLineSep #808080; statusLineModel #875f87; statusLinePath #005f87; statusLineGitClean #005f00; statusLineGitDirty #af5f00; statusLineContext #5f5f87; statusLineSpend #005f5f; statusLineStaged 28; statusLineDirty 136; statusLineUntracked 31; statusLineOutput 133; statusLineCost 133; statusLineSubagents teal; pythonMode yellow`
- `export`: `pageBg #f8f8f8, cardBg #ffffff, infoBg #fffae6`
- `terminal`: `background #f8f8f8, foreground #3b3b3b, chrome #f8f8f8, widget #ffffff`, `ansi` = `["#000000","#cd3131","#107c10","#919514","#0451a5","#bc05bc","#0598bc","#555555","#666666","#f14c4c","#09a60a","#919516","#3b8eea","#ce68ce","#1a9cba","#a5a5a5"]`

### 1.3 Loader (`theme/loader.ts`)

- `getBuiltinThemes(): Record<string, ThemeJson>` (19-24).
- `getAvailableThemes(): Promise<string[]>` (26-38): builtin keys ∪ `*.json` basenames in `getCustomThemesDir()`, sorted (`Array.sort`, code-unit).
- `getAvailableThemesWithPaths(): Promise<ThemeInfo[]>` (44-66): `{name, path: string|undefined}`; builtins `path: undefined`, custom `path: <dir>/<name>.json`; a custom name shadowing a builtin is skipped; sorted by `localeCompare`.
- `parseThemeJson(name, content)` (68-93): `JSON.parse`; on parse error `Failed to parse theme <name>: <err>`. Then `validateThemeJson` (`./schema-validation`, lazy `require`); on failure message `Invalid theme "<name>":\n` + optional `Missing required color tokens:` list parsed from `/missing keys: (.+)/i` + `Please add these colors to your theme's "colors" object.` + `See the built-in themes (dark.json, light.json) for reference values.` + `Validation error:\n  - <msg>`.
- `loadThemeJson(name)` (95-104): builtin returns embedded object; else read `<custom>/<name>.json` as UTF-8 (`Bun.file(...).text()`); ENOENT → `Theme not found: <name>`.
- `loadThemeJsonSync` (106-114): same with `fs.readFileSync`.
- `CreateThemeOptions { mode?: ColorMode; symbolPresetOverride?: SymbolPreset; colorBlindMode?: boolean }` (116-120).
- Constants (122-125): `COLORBLIND_ADJUSTMENT = { h: 60, s: 0.71 }`; `EXTENSION_LINK_TINT = 0.45`.
- `intendedColor(value, terminalForeground, ansiPalette)` (131-138): `""` → `terminalForeground ?? ""`; numeric `<16` → `ansiPalette[value] ?? value`; else unchanged.
- `createTheme(themeJson, options)` (141-176), exact pipeline:
  1. `colorMode = mode ?? detectColorMode()`.
  2. `resolvedColors = resolveThemeColors(themeJson.colors, themeJson.vars)`.
  3. If `colorBlindMode` and `resolvedColors.toolDiffAdded` is a string starting `#`, replace with `adjustHsv(added, {h:60, s:0.71})`.
  4. `ansiPalette = themeJson.terminal?.ansi?.map(c => resolveVarRefs(c, vars))`.
  5. If `resolvedColors.mdExtensionLink === undefined`: `mixThemeColors(intendedColor(text), intendedColor(mdLink), 0.45, isLightSurface(resolvedColors.statusLineBg))`.
  6. Split resolved colors into `fgColors` / `bgColors` by the exact key set `{selectedBg,userMessageBg,customMessageBg,toolPendingBg,toolSuccessBg,toolErrorBg,statusLineBg}` (all other keys → fg).
  7. `symbolPreset = symbolPresetOverride ?? themeJson.symbols?.preset ?? "unicode"`; `symbolOverrides = themeJson.symbols?.overrides ?? {}`; `spinnerFramesOverrides = normalizeSpinnerFramesOverride(themeJson.symbols?.spinnerFrames)`.
  8. `new Theme(fgColors, bgColors, colorMode, symbolPreset, symbolOverrides, spinnerFramesOverrides, ansiPalette)`.
- `loadTheme`, `loadThemeSync`, `getThemeByName` (returns `undefined` on any error) (178-196).

### 1.4 `Theme` class (`theme/theme-class.ts`)

Constructor (206-258) `new Theme(fgColors: Record<ThemeColor, string|number>, bgColors: Record<ThemeBg, string|number>, mode, symbolPreset, symbolOverrides, spinnerFramesOverrides = {}, ansiPalette = [])`:
- `statusLineLuminance = colorLuma(bgColors.statusLineBg)`; `#statusLineContrastLuminance = relativeLuminance(bgColors.statusLineBg)`; `slIsLight = isLightLuma(statusLineLuminance)`.
- Per fg color: keep only `isValidThemeColor` keys; `#fgColors[k] = fgAnsi(value, mode)`; `#hexFgColors[k] = resolveToHex(value, slIsLight)`.
- Then fill every `OPTIONAL_THEME_COLOR_RECORD` key with `"\x1b[39m"` (fg ANSI) and `""` (hex) if absent.
- `#ansiFgColors[i] = fgAnsi(ansiPalette[i] ?? i, mode)` for i∈0..15.
- Per bg color: `#bgColors[k] = bgAnsi(value, mode)`, `#hexBgColors[k] = resolveToHex(value, slIsLight)`.
- `#symbols = {...SYMBOL_PRESETS[symbolPreset]}` then apply overrides whose key exists (invalid keys logged at debug and dropped); then `status.done` aliases `status.success` unless explicitly overridden.

Core primitives (semantics exact):
- `get isLight()` → `isLightLuma(statusLineLuminance)` (259).
- `get accentSurfaceLuminance()` → `isLight ? #statusLineContrastLuminance : undefined` (267).
- `getColorHex(color)` (274-280): throws `Unknown theme color: <c>`; returns hex or `isLight ? "#000000" : "#e5e5e7"`.
- `getBgHex(color)` (282-289): throws `Unknown theme background color: <c>`; returns hex or `isLight ? "#ffffff" : "#000000"`.
- `getAllThemeColorHexes()` (292): all non-empty fg then bg hexes, insertion order.
- `getMajorThemeColorHexes()` (309-337): exact order `accent, border, borderAccent, borderMuted, success, error, warning, mdHeading, mdLink, mdCode, mdCodeBlock, mdQuoteBorder, mdListBullet, toolDiffAdded, toolDiffRemoved, customMessageLabel, thinkingText`; non-empty only.
- `getAccentColorHex()` = `getColorHex("accent")` (339).
- `get sessionAccentInputs()` (348): memoized frozen `{ accentHex, colorHexes: frozen major hexes, surfaceLuminance: accentSurfaceLuminance }`.
- `hasColor(color)` (364): `fgAnsi !== undefined && fgAnsi !== "\x1b[39m"`.
- `fg(color, text)` (369): throws if unknown; `wrapForeground(ansi, text)`.
- `ansiFg(color, text)` (380): `wrapForeground(#ansiFgColors[ANSI_COLOR_INDEX[color]], text)`.
- `fgResolved(color, text)` (385): if token is `\x1b[39m`, resolve `colorToAnsi(getColorHex(color), mode)`; memoized per color; `wrapForeground`.
- `bg(color, text)` (396): `${ansi}${text}\x1b[49m`.
- `bgFill(color, text)` (408): `${ansi}${text.replace(/\x1b\[(?:0|49)m/g, `$&${ansi}`)}\x1b[49m`.
- `fgOnBg(color, background, text)` (419) → `wrapForeground(getFgOnBgAnsi(...))`.
- Styles (424-447): `bold` `\x1b[1m`/`\x1b[22m`; `faint` dim; `italic`; `underline`; `strikethrough`; `inverse` (chalk semantics; additive).
- `getFgAnsi` (449), `getBgAnsi` (455): raw SGR, throw on unknown.
- `getFgOnBgAnsi(color, background)` (465-484): explicit token wins; if token is `\x1b[39m` and bg is `\x1b[49m` return token; else pick `#000000` when `colorLuma(getBgHex(bg)) > 0.5` else `#e5e5e7`.
- `getContrastFgAnsi(fillColor)` (486-493): parse `38;2;R;G;B` out of resolved fg; luma `0.299R+0.587G+0.114B`; `>140` → `\x1b[38;2;0;0;0m` else `\x1b[38;2;255;255;255m`; if not truecolor (no match) → `#fgColors.text`.
- `getColorMode()` (494).
- `getThinkingBorderColor(level)` (499): `thinkingLevelToken(level)`; if `thinkingMax` and token unset → `thinkingXhigh`; returns `(str) => fg(color, str)`.
- `getBashModeBorderColor()` → `fg("bashMode", …)`; `getPythonModeBorderColor()` → `fg("pythonMode", …)` (506-519).
- `symbol(key)` (521); `styledSymbol(key, color)` = `fg(color, symbol)` (528); `getSymbolPreset()` (535).

`wrapForeground(ansi, text)` (169-174): if text contains `\x1b[`, replace every `FOREGROUND_RESET_PATTERN = /\x1b\[(?:0|39)m/g` with `$&${ansi}`; return `${ansi}${body}\x1b[39m`.

Group getters (all plain objects of symbol strings): `status` (543, includes deprecated `done`), `nav` (560), `tree` (570), `progress` (580), `boxRound` (587-609, corners/horizontal/vertical + `cross/teeDown/teeUp/teeRight/teeLeft` sourced from `boxSharp.*`), `boxDotted` (610), `boxSharp` (617), `sep` (633: `powerline, powerlineThin, powerlineLeft, powerlineRight, powerlineThinLeft, powerlineThinRight, powerlineCapLeft, block, space, asciiLeft, asciiRight, dot, slash, pipe`), `icon` (652-711), `cmd` (712-786, includes aliases to `icon.*`), `thinking` (787), `checkbox` (799), `radio` (806), `format` (813), `md` (822), `spinnerFrames` (834) = `getSpinnerFrames()`.

- `getSpinnerFrames(type: SpinnerType = "status")` (841): `overrides[type] ?? SPINNER_FRAMES[preset][type]`.
- `getLangIcon(lang)` (~848): empty → `lang.default`; lowercase lookup in `langMap`; miss → `lang.default`.
- `getLangIconStyled(lang)` (~860): no icon → return bare; no brand entry → `fg("muted", icon)`; else `${colorToAnsi(hex, mode)}${icon}\x1b[39m`.

`langMap` (`theme-class.ts:29-128`) exact name→key mapping (keys are lowercased before lookup):
`typescript|ts|tsx→lang.typescript; javascript|js|jsx|mjs|cjs→lang.javascript; python|py→lang.python; rust|rs→lang.rust; go→lang.go; java→lang.java; c→lang.c; cpp|c++|cc|cxx→lang.cpp; csharp|cs→lang.csharp; ruby|rb→lang.ruby; julia|jl→lang.julia; php→lang.php; swift→lang.swift; kotlin|kt→lang.kotlin; bash|sh|zsh|fish|powershell|just|shell→lang.shell; html|htm|astro|vue|svelte→lang.html; css|scss|sass|less→lang.css; json→lang.json; yaml|yml→lang.yaml; markdown|md→lang.markdown; sql→lang.sql; dockerfile|docker→lang.docker; lua→lang.lua; text|txt|plain→lang.text; log→lang.log; env|dotenv→lang.env; toml→lang.toml; xml→lang.xml; ini→lang.ini; conf|cfg|config|properties→lang.conf; csv→lang.csv; tsv→lang.tsv; image|img|png|jpg|jpeg|gif|webp|svg|ico|bmp|tiff→lang.image; pdf→lang.pdf; zip|tar|gz|tgz|bz2|xz|7z→lang.archive; exe|dll|so|dylib|wasm|bin→lang.binary`.

`LANG_BRAND_COLORS` (`theme-class.ts:133-138`): `lang.javascript #f7df1e`, `lang.python #3776ab`, `lang.ruby #cc342d`, `lang.julia #9558b2`.

`thinkingLevelToken(level)` (`theme-class.ts:176-188`): `minimal→thinkingMinimal, low→thinkingLow, medium→thinkingMedium, high→thinkingHigh, xhigh→thinkingXhigh, max→thinkingMax`, default `thinkingOff`.

### 1.5 Active theme singleton (`theme/theme.ts`)

`theme` is the active `Theme` instance (imported everywhere as `theme`). Exports:
- `onTerminalBackgroundFallbackChange(cb): () => void` (52)
- `detectTerminalBackground(): "dark" | "light"` (64)
- `bindTheme(binding: ThemeBinding): () => void` (107)
- `getCurrentThemeName(): string | undefined` (125)
- `fgOrPlain(color, text, styledText = text)` (130)
- `ThemeChangeEvent` (133)
- `initThemeSync(...)` (176), `ensureThemeSync()` (194), `ensureTheme(): Promise<void>` (199), `initTheme(...)` (204)
- `setTheme(...)` (226)
- `previewTheme(...)` (271): request-id guarded; a stale request returns `{success:false, error:"Theme preview superseded by a newer request"}`; success `assignTheme` + `notifyThemeChange`.
- `enableAutoTheme(event = {})` (298), `setAutoThemeMapping(mode, themeName)` (307), `onTerminalAppearanceChange(mode, event = {ephemeral:true})` (318; no-op if unchanged)
- `setThemeInstance(themeInstance)` (327): `autoDetectedTheme=false`, assign, `currentThemeName="<in-memory>"`, stop watcher.
- `setSymbolPreset(preset)` (338), `getSymbolPresetOverride()` (359), `setNativeSymbolPreset(preset): boolean` (371)
- `setColorBlindMode(enabled)` (391), `getColorBlindMode()` (412)
- `onThemeChange(cb): () => void` (416)
- `getThemeEpoch(): number` (432) — bumped on every assignment; native describe memos key on it.
- `startMacOSAppearanceReprobeFallback(terminal)` (563), `stopThemeWatcher()` (734)
- `getResolvedThemeColors(themeName?): Promise<Record<string,string>>` (803)
- `isLightTheme(themeName?): boolean` (836): builtin or `<custom>/<name>.json`, `isLightThemeJson`.
- `getThemeExportColors(themeName?): Promise<{pageBg?, cardBg?, infoBg?}>` (858).

`theme/index.ts` re-exports `../lang-from-path`, `./session-color`, `./shimmer`, `./theme` (plus loader/schema/symbols/Theme via theme.ts re-exports).

### 1.6 Shimmer (`theme/shimmer.ts`)

- `type ShimmerMode = "classic" | "kitt" | "disabled"`; module `activeMode` default `"classic"`; `setShimmerMode(mode)`; `shimmerEnabled()` = mode ≠ disabled.
- `ShimmerPalette { low, mid, high: ThemeColor | {ansi:string}; bold?: boolean }`; `ShimmerSegment { text: string; palette?: ShimmerPalette }`.
- `DEFAULT_SHIMMER_PALETTE = { low:"dim", mid:"muted", high:"accent", bold:true }`.
- Constants: `SHIMMER_SPEED_CELLS_PER_S = 30`; `CLASSIC_PADDING = 10`; `CLASSIC_BAND_HALF_WIDTH = 6`; `KITT_HEAD_HALF = 0.6`; `KITT_TRAIL_LEN = 7`; `TIER_HIGH = 0.65`; `TIER_MID = 0.22`; `BOLD_OPEN "\x1b[1m"`, `BOLD_CLOSE "\x1b[22m"`.
- `classicIntensity(index, position)`: `dist=|index+10-position|`; `dist>=6 → 0`; else `0.5*(1+cos(π·dist/6))`.
- `kittIntensity(index, head, goingRight)`: `abs=|index-head|`; `abs<=0.6 → 1`; `behind = goingRight ? head-index : index-head`; `behind<=0.6 → 0`; `t=(behind-0.6)/7`; `t>=1 → 0`; else `(1-t)^2`.
- `tierFor(i)`: `>=0.65 high`, `>=0.22 mid`, else `low`.
- `describeShimmer(segments, key?)`: native node. Spans = each segment's text with style = its palette `mid` (string tokens only; raw-ANSI tiers dropped). `disabled` → `node("text",{spans})`. Else `node("shimmer", { spans, mode: activeMode, palette: {low?,mid?,high?} }, key)` using the first segment's palette (default palette if none).
- `shimmerSegments(segments, theme)`:
  - If `isNativeRendering()` → concatenate raw text, no styling.
  - `disabled`: if no segment has text → `""`; else each segment wrapped in its `mid` open/close.
  - Else total = sum of code points; if 0 → `""`. `travel = (Date.now()/1000)*30`.
  - classic: `position = travel % (total + 20)`; `bandLo = position-16`, `bandHi = position-4`.
  - kitt: `range = total-1`; if `<=0` position 0; else `cycleCells=2*range`, `sweep=travel%cycleCells`, `goingRight = sweep<range`, `position = goingRight ? sweep : cycleCells-sweep`; band window extended by head/trail in travel direction.
  - Per segment: compile palette (cached on the palette object via Symbols), walk UTF-16 with surrogate-pair atomicity (step 2 for high+low surrogate), compute tier (outside window → `low`), coalesce runs of equal tier, emit `{open}{slice}{close}`. Position/index counted in code points.

### 1.7 Session accents (`theme/session-color.ts`)

`SessionAccentTheme { accentHex: string; colorHexes: readonly string[]; surfaceLuminance?: number }`.
- `nameToHash` = djb2: `h=5381; h=((h<<5)+h)^code; h>>>=0`.
- Constants: `FALLBACK_CUSP_CHROMA_FRACTION=0.7`; `MIN_CHROMA=0.05`; `MAX_CHROMA=0.21`; `DARK_MIN_LIGHTNESS=0.65`; `DARK_MAX_LIGHTNESS=0.88`; `ACCENT_MIN_CONTRAST=3`; `MIN_HUE_DISTANCE=10`; `MIN_CHROMA_FOR_HUE=0.03`; `LIGHT_HUE_INTERVALS=[[195,330]]`; `ACCENT_CACHE_LIMIT=256`.
- `DARK_HUE_INTERVALS` computed: contiguous hue runs `h∈[0,360)` where `oklchCusp(h).l <= 0.88`.
- `accentLuminanceCap(surfaceLuminance) = max(0,(surfaceLuminance+0.05)/3 - 0.05)`.
- `hexToHue(hex)`: OKLCH `{c,h}`; undefined if `!finite(h)` or `c<0.03`.
- `findSafeHue(pos, occupied, intervals)`: target = arc position→hue; if all occupied hues ≥10° away return target; else walk `d=1..total-1` in directions `+1,-1`, first candidate with all distances ≥10; fallback target.
- `arcLength`/`arcToHue`: inclusive intervals `[a,b]`, positions counted per integer degree.
- `getSessionAccentHex(name, theme)`: memoized by `${name}\0${accentHex}\0${surfaceLuminance}\0${colorHexes.join(",")}`; clears cache at 256. Algorithm:
  1. `isDark = surfaceLuminance === undefined`; intervals = dark/light.
  2. `pos = nameToHash(name) % arcLength`.
  3. `themeHues = colorHexes.map(hexToHue).filter(defined)`; `targetHue = findSafeHue(pos, themeHues, intervals)`.
  4. `accent = hexToOklch(accentHex)`; usable iff l,c,h finite and `c>=0.03`. `target = oklchCusp(targetHue)`. If usable: `source = oklchCusp(accent.h)`; `lightness = accent.l<=source.l ? (accent.l/source.l)*target.l : target.l + ((accent.l-source.l)/(1-source.l))*(1-target.l)`; `chromaFraction = accent.c/source.c`. Else `lightness=target.l`, `chromaFraction=0.7`. `chroma = clamp(chromaFraction*target.c, 0.05, 0.21)`.
  5. Dark: `l = clamp(lightness, 0.65, 0.88)`; return `oklchToHex({l,c:chroma,h:targetHue})`.
  6. Light: `cap = accentLuminanceCap(surfaceLuminance)`; `top = oklchToHex({lightness,chroma,targetHue})`; if `relativeLuminance(top) <= cap` return top; else bisect lightness 20 iterations (`lo=0`, `hi=lightness`; mid > cap → hi=mid else lo=mid), return `oklchToHex({l:lo,...})`.
- `getSessionAccentAnsi(hex)`: `Bun.color(hex, trueColor ? "ansi-16m" : "ansi-256")`; cached by `hex` or `256:<hex>`; undefined for nullish.

### 1.8 Color utilities (`theme/color.ts`)

- `FG_RESET = "\x1b[39m"`, `BG_RESET = "\x1b[49m"`.
- `detectColorMode(env = Bun.env)`: `WT_SESSION` set → `"truecolor"`; else terminal info `trueColor` → truecolor/256color.
- `colorToAnsi(color, mode)`: `Bun.color(color, mode==="truecolor"?"ansi-16m":"ansi-256")`; null → throw `Invalid color value: <color>`. Cache: maps per mode, only colors `length<=128`, clear at 256 entries.
- `fgAnsi(color, mode)`: `""`→`\x1b[39m`; number→`\x1b[38;5;${n}m`; string→`colorToAnsi`; else throw.
- `bgAnsi(color, mode)`: `""`→`\x1b[49m`; number→`\x1b[48;5;${n}m`; string→`colorToAnsi(...).replace("\x1b[38;","\x1b[48;")`.
- `resolveVarRefs(value, vars, visited?)`: number or `""` or starts `#` → as-is; cycle → `Circular variable reference detected: <v>`; missing → `Variable reference not found: <v>`; else recurse.
- `resolveThemeColors(colors, vars={})`: resolve every entry.
- `isLightLuma(luma)`: `luma !== undefined && luma > 0.5`.
- `isLightSurface(surface)`: `isLightLuma(surface===undefined ? undefined : colorLuma(surface))`.
- `resolveToHex(value, isLight)`: number→`ansi256ToHex`; `""`→`isLight ? "#000000" : "#e5e5e7"`; else value.
- `mixThemeColors(a,b,t,isLight)`: normalize each via `resolveToHex` then `Bun.color(resolved,"hex")` (null → throw), `mixHex(hexA,hexB,t)`.
- `ansi256ToHex(index)`: `paletteToRgb` (0-15 basic, 16-231 6×6×6 cube, 232-255 gray ramp) → `rgbToHex`; invalid index throws.

### 1.9 Path→language (`lang-from-path.ts`, re-exported by `theme/index.ts`)

- `EXTENSION_LANG: Record<ext, readonly [highlightId, lspId]>` (lowercased extension, no dot). Observed entries include: `ts/cts/mts→[typescript,typescript]`; `tsx→[tsx,typescriptreact]`; `js/jsx/mjs/cjs→[javascript,javascript(javascriptreact for jsx)]`; `rs→[rust,rust]`; `go`; `c/h→[c,c]`; `cpp/cc/cxx/hh/hpp/hxx/cu/cuh/ino→[cpp,cpp]`; `zig`; `py/pyi→[python,python]`; `rb/rbw/gemspec→[ruby,ruby]`; `erb→[erb,erb]`; `lua`; `sh/bash/zsh/ksh→[bash,shellscript]`; plus more (markdown, json, yaml, docker, etc. — read the file for the complete table).
- `getLanguageFromPath(path)`: basename lowercased; special names first; else extension lookup → highlight id, default `"plaintext"`.
- `detectLanguageId(path)`: basename special cases in order: `dockerfile`, `dockerfile.*`, `containerfile` → `"dockerfile"`; `.emacs` → `"emacs-lisp"`; `makefile`/`gnumakefile` → `"makefile"`; `justfile` → `"just"`; then `cmakelists.txt` or ext `cmake` → `"cmake"`; else `EXTENSION_LANG[ext]?.[1] ?? "plaintext"`.
- `isMarkdownPath(path)` = `getLanguageFromPath(path) === "markdown"`.

### 1.10 Symbols (`theme/symbols.ts`)

- `SymbolPreset = "unicode" | "nerd" | "ascii"`; `SymbolMap = Record<SymbolKey,string>`.
- `SymbolKey` exact vocabulary (grouped): `status.{success,error,warning,info,pending,disabled,enabled,running,shadowed,aborted,done}`; `nav.{cursor,selected,expand,collapse,back}`; `tree.{branch,last,vertical,horizontal,hook}`; `progress.{filled,empty}`; `context.{speculation,compaction,content,messages,buffer,free}`; `boxRound.{topLeft,topRight,bottomLeft,bottomRight,horizontal,vertical}`; `boxDotted.{horizontal,vertical}`; `boxSharp.{topLeft,topRight,bottomLeft,bottomRight,horizontal,vertical,cross,teeDown,teeUp,teeRight,teeLeft}`; `sep.{powerline,powerlineThin,powerlineLeft,powerlineRight,powerlineThinLeft,powerlineThinRight,powerlineCapLeft,block,space,asciiLeft,asciiRight,dot,slash,pipe}`; `icon.{model,plan,prewalk,goal,pause,loop,folder,worktree,search,scratchFolder,file,git,branch,pr,pin,tokens,context,cost,subscription,advisor,advisorClosed,time,omp,ghost,agents,job,cache,cacheMiss,cacheWarm,cacheCold,input,output,throughput,intelligence,host,session,package,warning,rewind,auto,fast,extensionSkill,extensionTool,extensionSlashCommand,extensionMcp,extensionRule,extensionHook,extensionPrompt,extensionContextFile,extensionInstruction,vimNormal,vimInsert,vimVisual,vimVisualLine,vimReplace,mic,camera}`; `cmd.{action,prompt,extension,settings,gear,shield,wave,compass,inbox,swap,expand,computer,eye,todo,stats,news,keyboard,export,clipboard,share,broadcast,globe,copy,plus,restart,eraser,trash,compress,vibrate,handoff,history,question,rocket,stethoscope,redo,bug,memory,pencil,folderMove,folderPlus,folderMinus,hammer,power,cart}`; `thinking.{minimal,low,medium,high,xhigh,max,autoPending}`; `checkbox.{checked,unchecked}`; `radio.{selected,unselected}`; `format.{bullet,dash,bracketLeft,bracketRight}`; `md.{quoteBorder,hrChar,bullet,colorSwatch}`; `advisor.rail`; `skill.rail`; `btw.rail`; `lang.{default,typescript,javascript,python,rust,go,java,c,cpp,csharp,ruby,julia,php,swift,kotlin,shell,html,css,json,yaml,markdown,sql,docker,lua,text,env,toml,xml,ini,conf,log,csv,tsv,image,pdf,archive,binary}`; `chip.{image,video,paste}`; `tab.{appearance,model,interaction,context,files,shell,tools,memory,tasks,providers}`; `tool.{write,edit,bash,ssh,lsp,gh,webSearch,exa,browser,eval,debug,mcp,job,launch,task,todo,memory,ask,resolve,review,goal,irc,delete,move}`; `key.{ctrl,ctrlMac,shift,alt,altMac,super,superMac,joiner,enter,esc,tab,backspace,delete,space,up,down,left,right}`.
- Unicode preset values (exact, `symbols.ts:386-560`): `status.success ✔, error ✘, warning ⚠, info ⓘ, pending ⏳, disabled ⦸, enabled ●, running ⟳, shadowed ○, aborted ⏹, done ✔; nav.cursor ❯, selected ➤, expand ▸, collapse ▾, back ⟵; tree.branch ├─, last └─, vertical │, horizontal ─, hook └; progress.filled ━, empty ─; context.speculation ╎, compaction ┃, content ⛁, messages ⛃, buffer ⛝, free ⛶; boxRound ╭ ╮ ╰ ╯ ─ │; boxDotted ┄ ┆; boxSharp ┌ ┐ └ ┘ ─ │ ┼ ┬ ┴ ├ ┤; sep.powerline ▕, powerlineThin ┆, powerlineLeft ▶, powerlineRight ◀, powerlineThinLeft >, powerlineThinRight <, powerlineCapLeft "", block ▌, space " ", asciiLeft >, asciiRight <, dot " · ", slash " / ", pipe " │ "; icon.model ⬢, plan 🗺, prewalk 🏃, goal 🎯, pause ⏸, loop ↻, folder 📁, worktree 🌳, search 🔍, scratchFolder 🗑, file 📄, git ⎇, branch ⑂, pr ⤴, pin 📌, tokens 🪙, context ◫, cost 💲, subscription "(sub)", advisor 👁, advisorClosed 🙈, time ⏱, omp π, ghost 👻, agents 👥, job ⚙, cache 💾, cacheMiss ⊘, cacheWarm ●, cacheCold ⊘, input ⤵, output ⤤, throughput ⚡, intelligence 🧠, host 🖥, session 🆔, package 📦, warning ⚠, rewind ↶, auto ⟲, fast ⚡, extensionSkill ✦, extensionTool 🛠, extensionSlashCommand ⌘, extensionMcp 🔌, extensionRule ⚖, extensionHook 🪝, extensionPrompt ✎, extensionContextFile 📎, extensionInstruction 📘; vimNormal ■, vimInsert ▎, vimVisual ◉, vimVisualLine ≡, vimReplace ▁; cmd.* as listed in the source; thinking.* / checkbox.* / radio.* / format.* / md.* / rails / lang.* / chip.* / tab.* / tool.* / key.*`.
- `SPINNER_FRAMES` (`symbols.ts:1515-1537`): unicode `status ["⣾","⣽","⣻","⢿","⡿","⣟","⣯","⣷"]`, `activity ["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"]`; nerd `status ["󱑖","󱑋","󱑌","󱑍","󱑎","󱑏","󱑐","󱑑","󱑒","󱑓","󱑔","󱑕"]`, `activity` same braille; ascii `status ["[|]","[/]","[-]","[\\]"]`, `activity ["-","\\","|","/"]`.
- `SpinnerFramesOverride = string[] | {status?: string[]; activity?: string[]}`; `normalizeSpinnerFramesOverride` (flat array → both).
- `getAvailableSymbolPresets()` → `["unicode","nerd","ascii"]`; `isValidSymbolPreset`.
- `SlashCommandIconName` = the 48 `cmd.*` names + 37 shared `icon.*` aliases (`model, plan, prewalk, goal, pause, loop, session, jobs, gauge, context, ghost, agents, branch, tree, signIn, signOut, advisor, host, package, fast, voice, tools, rule, skill, mcp, pin`).

---

## 2. `render/`

`render/index.ts` re-exports: `code-cell, extension-links, file-list, hyperlink, link-hints, output-block, output-pane, render-utils, sixel, status-line, tool-card, tree-list, types, utils, width-aware-text`.

### 2.1 `output-block.ts`

- `OutputBlockOptions` (17-34): `header?, headerMeta?, footerMeta?, state?: State, sections?: Array<{label?, lines: readonly string[], separator?}>, width: number, applyBg?, contentPaddingLeft?, contentPaddingRight?, borderColor?: ThemeColor`.
- `markFramedBlockComponent(c)` / `isFramedBlockComponent(c)` (39-57): Symbol `framedBlockComponent` brand.
- `outputBlockContentWidth(width, left?, right?)` (66-75): `left = normalize(left)` (default 1, floor, ≥0); `right = normalize(right ?? left)`; `max(1, width-2-left-right)`.
- `renderOutputBlock(options, theme): string[]` (77-223): border color = `borderColor ?? (error→"error", warning→"warning", running|pending→"accent", else "dim")`. Background stabilization (84-97): when state && applyBg, `bgFn` re-applies `bgAnsi` after every `\x1b[0m`/`\x1b[m` and after `\x1b[49m`, wrapping `${bgAnsi}…\x1b[49m`. Header row: icon + title (+ meta); sections separated; bottom border embeds `footerMeta` as `╰── <meta> ──╯`.
- `outputStateTone(state)` (225) → `TspTone`; `outputStateStatus(state)` (242) → `TspCardStatus|undefined`.
- `NativeOutputBlockSection`, `NativeOutputBlockOptions` (259-295), `describeOutputBlock(options): NativeNode` (297), `CachedOutputBlock` (338) — memoizing block renderer.

### 2.2 `tool-card.ts`

- `ToolCardPhase = "pending"|"running"|"partial"|"success"|"warning"|"error"|"info"`; `ToolCardVariant = "framed"|"plain"`; `ToolCardContent = readonly string[] | Component | ((width)=>readonly string[]|Component)`.
- `ToolCardSection { label?, content, separator? }`; `ToolCardSnapshot { header?, status?: StatusLineOptions, headerMeta?, footerMeta?, phase?, body?, sections?, footer?, applyBg?, contentPaddingLeft?, contentPaddingRight?, borderColor? }`.
- `ToolCardBuildContext { width, contentWidth, contentWidthFor(left?,right?) }`.
- `ToolCardOptions { variant, paddingX?, paddingY?, ignoreTight?, onInvalidate?, onReleaseRenderCaches?, onDispose? }`.
- `toolCardState(phase)` (83-88): `partial→"pending"`, `info→undefined`, else phase as `State`.
- `ToolCard` (137-411): `render(width)` builds snapshot; framed → `CachedOutputBlock.render(OutputBlockOptions)` (reuses previous options object when width/header/headerMeta/footerMeta/state/applyBg/paddings/borderColor and per-section label+lines identity+separator all match); plain → header line (`headerMeta` appended with a space), sections (blank line before `separator` sections, label line, content lines), `footerMeta` line, then `Text` with key `${width}:${text.length}:${hash36(text)}:${state??"info"}:${applyBg??true}` and background `theme.bg(getStateBgColor(state))` when state && applyBg ≠ false. `describe(cx)` → `describeOutputBlock` with `role:"omp.tool"`, `inset: plain || undefined`, tone from `borderColor` (`colorTone(...) ?? "neutral"`) or neutral when plain && applyBg===false; head spans from `snapshot.header` via `styledSpans`/`compactText`, else `statusSpans`; sections keyed `String(index)`.
- `framedToolCard(theme, build, options)` / `plainToolCard(...)` (415-430).

### 2.3 `output-pane.ts`

- `OutputPaneEdge = "head"|"tail"`.
- `OutputPaneFormatOptions` (17-35): `lines, expanded, collapsedMaxLines, expandedMaxLines?, edge?, visual?, width?, styleLine?, uncapSixel?, showHiddenMarker?, showExpandHint?, showExpandHintWhenUncapped?, formatHidden?`.
- `splitTerminalOutputLines(text)` (48-53): `text.split(/\r?\n/u)`, then per line keep the substring **after the last `\r`** (bare-CR progress overwrite semantics).
- `defaultHiddenLabel(hidden, shown, total, edge)`: tail → `… (${hidden} earlier lines, showing ${shown} of ${total})`; head → `… ${hidden} more line${hidden===1?"":"s"}`.
- `formatOutputPaneLines(options, theme)` (66-119): sixel mask from `getSixelLineMask` when `TERMINAL.imageProtocol === Sixel`; sixel rows are never styled; `limit = (hasSixel && uncapSixel) ? undefined : (expanded ? expandedMaxLines : collapsedMaxLines)`; if limit finite: 0 → all hidden; `visual && tail` → `truncateToVisualLines`; `visual` head → render `Text` then slice; else slice head/tail. Hidden marker (dim) appended (head) or prepended (tail) unless `showHiddenMarker===false`; expand hint appended unless `showExpandHint===false`; if uncapped and `!expanded && showExpandHintWhenUncapped`, append hint.
- `describeOutputLines(lines, {expanded, collapsedMaxLines, expandedMaxLines?, edge?, key?})` (134-141): `ansi(lines.join("\n"), { follow: edge==="tail" ? true : undefined, preview: {lines: floor(limit)} })`.
- `OutputPane` class (156-380), `styleToolOutputLine(line, theme)` (382).

### 2.4 `status-line.ts` (render primitive)

`StatusLineOptions { icon?: ToolUIStatus; iconOverride?: string; spinnerFrame?: number; title: string; titleColor?: ThemeColor; description?: string; badge?: {label,color}; meta?: string[] }`.
`renderStatusLine(options, theme): string` (33-60):
1. `flattenForHeader` = `text.replace(/\r\n?|\n/g, " ")` applied to title/description/badge label/meta (tabs untouched).
2. `icon = iconOverride ?? (icon ? formatStatusIcon(icon, theme, spinnerFrame) : "")`.
3. `titleColor = titleColor ?? "accent"`; `line = icon ? `${icon} ${title}` : title`.
4. description → `line += ": " + fg("muted", description)`.
5. badge → `line += " " + fg(color, bracketLeft + label + bracketRight)`.
6. meta (flattened, empties filtered) → `line += " " + fg("dim", meta.join(theme.sep.dot))`.

### 2.5 Other render modules (exports)

- `width-aware-text.ts`: `WidthAwareText implements Component`.
- `tree-list.ts`: `TreeListOptions<T>`, `renderTreeList(options, theme): string[]`, `describeTreeList(options, theme): NativeNode`.
- `sixel.ts`: `isSixelPassthroughEnabled`, `containsSixelSequence`, `getSixelLineMask(lines): boolean[]`, `isSixelLine`, `sanitizeWithOptionalSixelPassthrough`.
- `row-moves.ts`: `BLANK_ROW=0`, `UNKNOWN_ROW=-1`, `RowMove`, `findRowMove(screen,target,base)`, `applyRowMove`, `rowMoveSequence(move,height)`.
- `render-utils.ts`: `ConfiguredThinkingLevel`, `sanitizeCarriageReturns`, `disambiguateDisplayLabels`, `setInlineImageMaxColumns/Rows`, `resolveImageOptions`, `PREVIEW_LIMITS`, `DEFAULT_TERMINAL_PREVIEW_LINES=10`, `FENCE_RE=/^( {0,3})([`~]{3,})/`, `EMPTY_LINK_TARGETS`, `formatStatusIcon`, `formatCodeFrameLine`, `formatExpandHint`, `replaceTabs`, `CodeFrameMarker`.
- `link-hints.ts`: `ViewportLink`, `ViewportLinkTarget`, `viewportLinkTargets(rows)`, `LinkHints`.
- `hyperlink.ts`: `FileLinkConfig`, `applyFileLinkSetting`, `tryFileUri`, `fileUriForTerminal`, `isHyperlinkEnabled`, `applyHyperlinkSetting`, `uriHyperlink`, `urlHyperlink`, `urlHyperlinkAlways`, `urlLinkSpan`.
- `file-list.ts`: `FileEntry`, `FileListOptions`, `describeFileList`, `renderFileList`.
- `hanging-wrap.ts`: `wrapPrefixedLine`, `wrapTreeLine`, `wrapGuidedLine`.
- `extension-links.ts`: `setExtensionLinks`, `extensionLinksVersion`, `extensionLinksRouted`, `setExtensionLinkPicking`, `extensionLinkClicksAvailable`, `extensionLinkScheme`, `formatExtensionLink`, `stripExtensionLinkMarkers`, `ExtensionLinkLabel`, `extensionLinkLabels`.
- `code-cell.ts`: `CodeCellOptions`, `renderCodeCell`, `describeCodeCell`, `describeMarkdownCell`, `MarkdownCellOptions`, `renderMarkdownCell`.
- `utils.ts`: `Hasher`, `RenderCache`, `buildTreePrefix`, `getTreeBranch`, `getTreeContinuePrefix`, `padToWidth`, `getStateBgColor`.
- `types.ts`: `State = "pending"|"running"|"success"|"error"|"warning"`, `TreeContext`.
- `composite.ts`: `compositeLineAt`.

---

## 3. `chrome/`

`chrome/index.ts` re-exports: `chat-block, context-thresholds, countdown-timer, diff, dynamic-border, error-block, keybinding-hints, message-divider, message-frame, message-notice, overlay-box, segment-track, select-list-mouse-routing, selector-helpers, shared, streaming-panel, tool-activity, transcript-container, transcript-notice, transcript-status, visual-truncate`.

### 3.1 `overlay-box.ts`

- `topBorder(width, title, color?)` (22), `divider(width)` (36), `bottomBorder(width, color?)` (41), `row(content,width,color?)` (47), `splitBodyWidth(width, sidebarWidth)` (64), `topBorderSplit(width,title,sidebarWidth)` (69), `dividerSplit(width,sidebarWidth)` (89), `splitRow(sidebar,body,width,sidebarWidth)` (100).
- `PanelRows implements Component` (123-176): `setLines(lines)` (byte-identical array retained), `setHeight(height|undefined)` (natural height when undefined; clamped/truncated ≥0), `invalidate()`, `render(width)` → memo per (width,height,lines identity); row count = height ?? lines.length; missing rows = `""`; rows wider than width truncated via `truncateToWidth`.
- `PanelDivider implements Component` (181-190): `render()` → no rows; `describe()` → `node("rule")` (sentinel `├───┤`).
- `OverlayPanel implements Component` (207-344): `constructor(title="", role="omp.overlay")`; `collapseTitle` = `title.replace(/\s+/g," ").trim()`; `children: Component[]`; `nativeRole`, `title` getter/setter; `render(width)` produces a one-row titled top border + `│ … │` rows (content inset 2 columns each side) + bottom border; `describe()` → root `card` with `head`=title, `role`=`omp.overlay.<name>`; memo keyed by width/title/children.

### 3.2 Message components

`message-frame.ts`:
- `FramedMessage { customType: string; content: string | (TextContent | {type:string})[] }`.
- `FramedRenderer<M> = (message, {expanded}, theme) => Component|undefined`.
- `FramedMessageOptions<M> { message, icon?: string|(()=>string), hideHeader?: boolean|(()=>boolean), borderColor?: ThemeColor, collapseAfterLines?: number, customRenderer?, role: string }`.
- `FramedMessageComponent<M> extends Container` (61-214): background `theme.bg("customMessageBg", text)` in a `Box(1,1)` with `ignoreTight`; `#rebuild()` on expand/invalidate; `describe()` memoized on `[version, expanded, getThemeEpoch()]`: custom component → `col([custom], {role})`; else `card({role, tone: colorTone(borderColor), head: hideHeader?undefined:[span(customType,"customMessageLabel strong")], collapsible: collapseAfterLines!==undefined, collapsed: collapseAfterLines!==undefined ? !expanded : undefined, preview: collapseAfterLines!==undefined ? {lines:collapseAfterLines} : undefined}, [md(messageText)])`; `messageText` = string content or join of `part.type==="text"` with `"\n"`; `handleNativeEvent` toggles expansion via `rootToggleExpanded`; `isTranscriptBlockCompact()` delegates.

`message-notice.ts`: `MessageNoticeContext`, `MessageNoticePresentation`, `MessageNoticeNativePresentation`, `MessageNoticeOptions`, `MessageNoticeComponent extends Container` (59).

`message-divider.ts`: `MessageDividerOptions` (10), `MessageDividerComponent implements Component` (31).

### 3.3 `diff.ts`

- `nativeDiff(diffText, props = {}): NativeNode` (121-162): parses omp compact diff rows `^([+-\s])(\s*\d+)\|(.*)$` (canonical) or `^([+-\s])(?:(\s*\d+)\s)?(.*)$` (legacy). Builds `TspDiffHunk {oldStart,newStart,lines}`; a blank row, `...`, or `…` closes the current run; a line-number jump closes the run and advances both old/new by the skip; runs with no `+`/`-` row are dropped; node `diff` with `hunks` and `path` grammar from props.
- `renderDiff(diffText, options: {filePath?, theme?}): string` (177-330): gutter width = `max(3, longest line-number length)` (constant 3 minimum so streaming rows stay byte-identical). Context lines dim (`toolDiffContext`) with batched `highlightContextLines`; `-` red with inverse word-diffs; `+` green with inverse word-diffs; gap rows render as dim `…`; duplicate consecutive line numbers blanked; intra-line diff via `diffWords` with leading-whitespace stripping from inverse spans; indent visualization: tabs → dim `→` (with `DEFAULT_TAB_WIDTH` padding), leading spaces → dim `·`.

### 3.4 Context

`context-thresholds.ts`:
- `ContextUsageLevel = "normal"|"warning"|"purple"|"error"`.
- Thresholds: warning 50% / 150_000 tokens; purple 70% / 270_000; error 90% / 500_000.
- `reachesThreshold(percent, window, pct, tok)`: false if percent non-finite or ≤0; if window non-finite/≤0 → `percent >= pct`; else `percent >= min(pct, tok/window*100)`.
- `getContextUsageLevel(percent, window)`: error → purple → warning → normal (first match wins).
- `getContextMeterThresholds(window)` → `{warn, bad}` as shares 0-1 (`min(pct, tok/window*100)/100`).
- `formatContextUsage(percent|null|undefined, window, usedTokens?)`: window unknown/≤0 → `${formatNumber(usedTokens??0)}/?`; else `${percent===null||undefined ? "?" : percent.toFixed(1)+"%"}/${formatNumber(window)}`.
- `getContextUsageTone(level)`: error→`"error"`, purple|warning→`"warning"`, normal→undefined.
- `getContextUsageThemeColor(level)`: error→`"error"`, purple→`"thinkingHigh"`, warning→`"warning"`, normal→`"statusLineContext"`.

`context-view-model.ts`:
- `CONTEXT_CATEGORIES` (display order, id/label): `system-prompt` "System Prompt"; `instruction-files` "Instruction Files"; `skills` "Skills"; `built-in-tools` "Built-in Tools"; `custom-tools` "Custom Tools"; `user-messages` "User / Developer Messages"; `assistant-messages` "Assistant Messages"; `assistant-thinking` "Assistant Thinking"; `tool-calls` "Tool Calls"; `tool-output` "Tool Output".
- `ContextCategoryId` = union of those ids. `contextCategoryDescriptor(id)`.
- `ContextViewEntry { id, label, text, tokens, interrupted?, thinkingGroup?, path?, info?, timestamp?, callId?, sourceMarkers? }`; `ContextViewSourceMarker {start,end,category}` (UTF-16 offsets); `ContextViewCategory { id, key?, label, tokens, entries, children? }`; `ContextViewData { breakdown: ContextBreakdown, categories, notices? }`.

`context-presentation.ts`: `CONTEXT_PRESENTATION` maps each category (+`buffer`,`free`) to `{color, symbol}`: system-prompt `{accent, context.content}`; instruction-files `{muted, context.content}`; skills `{success, context.content}`; built-in-tools `{warning, context.content}`; custom-tools `{customMessageLabel, context.content}`; user-messages `{accent, context.messages}`; assistant-messages `{userMessageText, context.messages}`; assistant-thinking `{customMessageLabel, context.content}`; tool-calls `{warning, context.content}`; tool-output `{muted, context.messages}`; buffer `{warning, context.buffer}`; free `{dim, context.free}`. `ContextPresentationId = ContextCategoryId | "buffer" | "free"`.

`context-map.ts`: read earlier; provides the context map presentation (category rows with weights, marks, labels) used by the `/context` viewer.

### 3.5 Other chrome modules (exports)

- `chat-block.ts`: `ChatBlockHost`.
- `dynamic-border.ts`: `DynamicBorder implements Component`.
- `error-block.ts`: `sanitizeErrorLine`, `formatErrorBlock`.
- `keybinding-hints.ts`: `editorKey`, `editorKeys`, `boundKeys`, `interruptKey`, `appKey`, `keyHint`, `appKeyHint`, `rawKeyHint`.
- `segment-track.ts`: `TrackSegment`, `resolveSegmentPalette(count): ThemeColor[]`, `describeSegmentTrack`, `renderSegmentTrack`.
- `select-list-mouse-routing.ts`: `routeSelectListMouseWithTopBorder`.
- `selector-helpers.ts`: `renderScrollableList`, `centeredWindow`, `contentRowWidth`, `clampSelection`, `handleTabSwitchKey`, `padLinesToHeight`.
- `shared.ts`: `sanitizeStatusText`, `sanitizeHookStatusText`, `getTabBarTheme`.
- `streaming-panel.ts`: `StreamingPanelPresentation`, `StreamingPanelContent extends Container`.
- `tool-activity.ts`: `ToolActivityComponent`, `isToolActivityComponent`, `ToolActivityContainer`.
- `transcript-container.ts`: `AnimationFrame`, `TranscriptPresentationTarget`, `TranscriptBlockMode`, `TranscriptStableRow`, `AppendOnlyTranscriptBlock`, `CompactTranscriptBlock`, `isRowPrefix`, `trimBlankEdges`, `TranscriptViewportSpan`, `TranscriptContainer`.
- `transcript-notice.ts`: `NoticeTone`, `NoticeRowOptions`, `noticeRow(tone,message,options)`, `TranscriptNotice`.
- `transcript-status.ts`: `TranscriptStatusPart`, `TranscriptStatusNative`, `TranscriptStatusRow`, `TranscriptStatusBlock`.
- `visual-truncate.ts`: `VisualTruncateResult`, `truncateToVisualLines`.
- `countdown-timer.ts`: `CountdownTimer`.
- `qrcode.ts`: `QrEcLevel`, `QrEncodeOptions`, `QrCode`, `QrRenderOptions`, `renderQrText`, `renderQrHalfBlocks`.
- `collab-qrcode.ts`: `collabBrowserLink`, `CollabQrCodeComponent`.
- `live-board.ts`: `LiveBoardOutput`, `LiveBoard`, `createLiveBoard`.
- `local-date.ts`: `formatLocalCalendarDate`, `formatLocalDateTimeWithOffset`.
- `form-theme.ts`: `formTheme: FormFieldTheme`.

---

## 4. `status-line/`

### 4.1 Schema/types (`schema.ts`, `types.ts`)

- `STATUS_LINE_SEGMENT_IDS` (28 ids, order): `pi, status, model, mode, path, git, pr, subagents, token_in, token_out, token_total, token_rate, cost, context_pct, context_total, time_spent, time, session, hostname, cache_read, cache_write, cache_hit, cache, session_name, usage, collab, stream, vim`.
- `CUSTOM_STATUS_LINE_DEFAULTS`: left `["vim","model","mode","path","git","pr"]`, right `["session_name","token_total","cost","context_pct"]`.
- `CONTEXT_LINE_MODE_VALUES = ["off","percentage","annotated","embedded"]`; `STATUS_LINE_PRESET_VALUES = ["default","minimal","compact","full","nerd","ascii","custom"]`; `STATUS_LINE_SEPARATOR_VALUES = ["powerline","powerline-thin","slash","pipe","block","none","ascii"]`.
- `StatusLineSettings` (types.ts): `preset?, leftSegments?, rightSegments?, separator?, segmentOptions?, showHookStatus?, sessionAccent?, transparent?, compactThinkingLevel?, contextLine?`.
- `StatusLineSegmentOptions`: `pi.showProfile?`, `model.showThinkingLevel?`, `path.{abbreviate?,maxLength?,stripWorkPrefix?}`, `git.{showBranch?,showStaged?,showUnstaged?,showUntracked?}`, `time.{format?:"12h"|"24h",showSeconds?}`.
- `SegmentContext` (full field list at `types.ts:73-250`): `session, now?, hostname?, focusedAgentId?, sessionAccent?, previewTitle?, isPinned?, activeRepo, width, options, compactThinkingLevel, hookStatuses?, planMode, prewalk, loopMode, goalStatusInFooter?, goalMode, vibeMode, vim, collab, stream, recording, usageStats{input,output,cacheRead,cacheWrite,totalTokens,orchestrationInput,orchestrationOutput,orchestrationCacheRead,premiumRequests,cost,subagentCost?,tokensPerSecond}, contextPercent, contextTokens, contextWindow, autoCompactEnabled, compactionSpeculation, speculationBlinkOn, subagentCount, subagentTreeCost?, activeMs, turnElapsedMs, brandFgAnsi?, git{branch,status{staged,unstaged,untracked},pr}, worktree, usage{tier,fiveHour,daily,sevenDay,monthly,resetCredits}, promptCache?`.
- `RenderedSegment {content, visible}`; `SegmentView {spans, icon?, tone?, motion?, title?}`; `SeparatorDef {left,right,endCaps?{left,right,useBgAsFg}}`; `PresetDef {leftSegments,rightSegments,separator,segmentOptions}`.

### 4.2 Presets (exact)

`default`: left `[pi, vim, model, mode, collab, stream, path, git, pr, context_pct, cost]`, right `[session_name]`, separator `powerline-thin`, options `{model:{showThinkingLevel:true}, path:{abbreviate:true,maxLength:40,stripWorkPrefix:true}, git:{showBranch:true,showStaged:true,showUnstaged:true,showUntracked:true}}`.
`minimal`: left `[vim, path, git]`, right `[session_name, mode, context_pct]`, `slash`, `{path:{abbreviate:true,maxLength:30}, git:{showBranch:true,showStaged:false,showUnstaged:false,showUntracked:false}}`.
`compact`: left `[vim, model, mode, git, pr]`, right `[session_name, cost, context_pct]`, `powerline-thin`, `{model:{showThinkingLevel:false}, git:{showBranch:true,showStaged:true,showUnstaged:true,showUntracked:false}}`.
`full`: left `[pi, vim, hostname, model, mode, path, git, pr, subagents]`, right `[session_name, cache_hit, token_in, token_out, token_rate, cache_read, cache, cost, context_pct, time_spent, time]`, `powerline`, `{model:{showThinkingLevel:true}, path:{abbreviate:true,maxLength:50}, git:all true, time:{format:"24h",showSeconds:false}}`.
`nerd`: left `[pi, vim, hostname, model, mode, path, git, pr, session, subagents]`, right `[session_name, token_in, token_out, cache_read, cache_write, token_rate, cache, cost, context_pct, context_total, time_spent, time]`, `powerline`, `{model:{showThinkingLevel:true}, path:{abbreviate:true,maxLength:60}, git:all true, time:{format:"24h",showSeconds:true}}`.
`ascii`: left `[vim, model, mode, path, git, pr]`, right `[session_name, token_total, cost, context_pct]`, `ascii`, `{model:{showThinkingLevel:true}, path:{abbreviate:true,maxLength:40}, git:all true}`.
`custom`: `CUSTOM_STATUS_LINE_DEFAULTS`, `powerline-thin`, `{}`.
`getPreset(name)` falls back to `default`.

### 4.3 Separators (`separators.ts`)

`getSeparator(style, theme)`:
- `powerline`: `left = sep.powerlineLeft`, `right = sep.powerlineRight`, `endCaps {left: sep.powerlineRight, right: sep.powerlineLeft, useBgAsFg:true}`.
- `slash`: `left = right = sep.slash.trim()`, no caps.
- `pipe`: `left = right = sep.pipe.trim()`.
- `block`: `left = right = sep.block`.
- `none`: `left = right = sep.space`.
- `ascii`: `left = sep.asciiLeft`, `right = sep.asciiRight`.
- default (incl. `powerline-thin`): `left = sep.powerlineThinLeft`, `right = sep.powerlineThinRight`, caps as powerline.

### 4.4 Segments (`segments.ts`)

- `SEGMENTS: Record<StatusLineSegmentId, StatusLineSegment>` (1331) — one `{id, render(ctx), describe(ctx)}` per id; `renderSegment(id, ctx)` (1362), `describeSegment(id, ctx)` (1371), `ALL_SEGMENT_IDS` (1375).
- `thinkingLevelWord(level)` (299).

### 4.5 Composition rules (`component.ts`, layout at 3149-3363)

Settings resolution: preset supplies `leftSegments/rightSegments/separator/segmentOptions`; user overrides merge over it. `layout` ∈ box | plain | plain-left | band (composer variants).

Order of operations for a box/band render of width `W`:
1. Render each configured left segment then right segment with `renderSegment`; keep only `visible && content !== ""`. `subagents` is skipped if an external subagent badge is present; `session_name` is skipped in `band` (title moves to the working row trailer).
2. `embedContext` is true when not plain, `contextLine === "embedded"`, `contextWindow > 0`, at least one context segment on either side, and at least one non-context segment on either side. If true, remove all context segments from both sides before overflow handling; reserve `embeddedContextGaugeMinWidth(percent, window)` for the gauge labels.
3. Right side prepends (in order): subagent badge, then background-job count `{icon.job} {n}` plus optional `formatJobWidgetGlance(...)` (skipped for `plain-left`).
4. Widths: `visibleWidth` per part. Group width = `sum(parts) + (n-1)*(sepWidth+2) + 2 + capWidth`, where `capWidth` = end cap width when caps exist and background is opaque (0 in transparent mode); band adds `bandCapWidth` (`sep.powerlineCapLeft`) to the left group.
5. Overflow, in order: (a) truncate `session_name` on the right to `minNameVW = 8` (accounting for badge/job parts unshifted ahead of tracked ids); (b) pop right segments from the end until it fits (or right is empty); (c) shrink `path` on the left, re-rendering with decreasing `maxLength` (min 4, up to 8 iterations, floor `minPathVW = 8`); (d) drop left segments from the end, but never drop `path` while another left segment remains (`leftOverflowDropIndex` returns the last index that is not `path`).
6. `renderGroup`: `bgAnsi + fgAnsi + " " + parts.join(` ${sepAnsi}${sep}${fgAnsi} `) + " " + \x1b[0m`, plus cap text at the outer edge (`capPrefix` uses `bgAnsi.replace("\x1b[48;","\x1b[38;")` when `useBgAsFg`, else `bgAnsi+sepAnsi`), and `\x1b[22m` before the cap when a subagent is focused. Right group = cap + content; left group = optional band cap + content + cap.
7. If no groups → `""`. If `W === 0`, or plain with a missing side → concatenate groups with a single space when both exist. Plain with both groups → `left + padding(gapWidth) + right` where `gapWidth = max(1, W-leftWidth-rightWidth)`.
8. Box/band: `gapWidth = max(1, W-leftWidth-rightWidth)`; when one group is absent the gauge runs to the border edge; otherwise the gap is the context gauge (annotated/percentage/embedded modes) with `embeddedContextGaugeMinWidth` budget and 1-cell fallback.

Native: `describeComposerFacts()` returns `{context: meter, model: SegmentView, extras: status node, usage: text}`; click actions map `model→status.model`, `context_pct|context_total→status.context`, `git→status.git`, `cost→status.cost`, `path→status.path`, `cache→status.cache`. `COMPOSER_HOMED_SEGMENTS` lists segments a TSP terminal shows outside composer facts: `pi, model, context_pct, context_total, cost, path, pr, session_name, subagents, vim`.

---

## 5. `tools/`

- `tools/index.ts`: `toolRenderers: Record<string, ToolRenderer>` keyed by `ask, ast_grep, ast_edit, bash, debug, eval, edit, apply_patch(→edit), find, glob, grep, lsp, wait, read, resolve, reject(→resolve), retain, recall, reflect, task, think, todo, github, goal, web_search, vibe_spawn, vibe_send, vibe_wait, vibe_kill, vibe_list, write, yield`; `export * from "./renderer"`; `setXdevRendererLookup(name => toolRenderers[name])`.
- `eval-format/index.ts`: `formatEvalCodeForDisplay(source, language: EvalLanguage): string`; `eval-format/javascript.ts`, `python.ts`.
- `shell-viz.ts`: `ShellVisualizationSource = RunTrace | ShellVisualization`; `attachShellTrace(snapshot, trace)`, `shellVisualization(snapshot)`, `defaultShellVizPreferences()`, `readShellPresentation(...)`, `isShellVizNode(value): value is string`, `decodeShellPresentation(json): ShellVizPresentation`.
- `shell-viz-render.ts`: `shellVizText(text)`, `renderShellVisualization(...)`.
- Other tool modules export their renderers (`bash.ts`, `edit.ts`, `read.ts`, `write.ts`, `grep.ts`, `glob.ts`, `find.ts`, `task.ts`, `think.ts`, `todo.ts`, `web-search.ts`, `github.ts`, `goal.ts`, `memory.ts`, `lsp.ts`, `eval.ts`, `ask.ts`, `debug.ts`, `wait.ts`, `yield.ts`, `vibe.ts`, `agent-tree.ts`, `autoresearch.ts`, `ast-edit.ts`, `ast-grep.ts`, `bash-interactive.ts`, `cfg-render.ts`, `conflict-detect.ts`, `daemon.ts`, `default-renderer.ts`, `fetch.ts`, `gh-format.ts`, `grouped-file-output.ts`, `hashline-format.ts`, `irc.ts`, `job-widget.ts`, `json-tree.ts`, `line-ranges.ts`, `list-limit.ts`, `match-line-format.ts`, `mcp.ts`, `native-view.ts`, `output-meta.ts`, `proc-render.ts`, `renderer.ts`, `resolve.ts`, `result-card.ts`, `report-tool-issue.ts`, `shell-trace.ts`, `streaming-output.ts`, `subprocess.ts`, `task-repair-args.ts`, `task-yield-assembly.ts`, `terminal-output.ts`, `todo.ts`, `tool-errors.ts`, `url-scheme-host.ts`, `xd-url.ts`, `xdev.ts`) — each exports its `ToolRenderer` plus helpers.

---

## 6. `native/` — Tern Surface Protocol (TSP)

### 6.1 Framing (`native/encode.ts`)

- `APC = "\x1b_"`, `ST = "\x1b\\"`, `TSP_APC_ID = "tsp"` (confirmed by `terminal/stdin-buffer.ts:86` comment: TSP `\x1b_tsp;`).
- `TSP_PREFIX = "\x1b_tsp;"` (26). `TSP_OSC_PREFIX = "\x1b]877;tsp;"` (28).
- Message shape: `TSP_PREFIX + verb + params + ";" + body + ST`, where `params` = concatenation of `";key=value"` for each param (`encodeParams`, 108-111), and `body` is UTF-8 JSON (or base64 for blobs).
- `encodeTspMessage(verb, body, params?, limit = TSP_DEFAULT_APC_LIMIT)` (112-133): fast path when `body.length*3 <= limit` (or exact `Buffer.byteLength(body) <= limit`) → single frame. Otherwise chunk: `chunkId = (nextChunkId++).toString(36)`; first chunk carries `params`, every chunk carries `;c=<id>`, all but last carry `;m=1`. Chunks split at safe boundaries: never mid-UTF-8 (`(byte & 0xc0) !== 0x80`) and never where the next chunk would begin with a `key=value;` parameter-shaped segment (`leadsWithParameter`, 44-60: `[A-Za-z0-9_-]+` then `=`, then printable ASCII ≠ `;`, then `;`). `chunkEnd` (63-75) picks nearest safe cut below target, else nearest above, else end. Pure ASCII without `;` cuts every `limit` characters.
- `encodeTspJson(verb, value, params?, limit?)` = `encodeTspMessage(verb, JSON.stringify(value), params, limit)`.
- `encodeTspHelloQuery(version?)` (146): hello query followed by DA1 sentinel; `features:["edit"]` and `"undo"`.
- `TspRawMessage`, `splitTspMessage(sequence)` (166) — parser for terminal input.
- Wire constants come from `@oh-my-pi/pi-wire` (`TSP_VERSION`, `TSP_DEFAULT_APC_LIMIT`, `TSP_DEFAULT_CREDITS`, `TSP_KINDS`, `TSP_TEXT_KINDS`, `TspFrame`, `TspNode`, `TspOp`, `TspEvent`, `TspReply`, `TspVerb`, `TspProps`, `TspSpan`, `TspText`, `TspTone`, `TspScrollBy`, `TspDiffHunk`). **[INFERENCE]** exact numeric values live in that package, which is not vendored in this tree.

### 6.2 Node model (`native/node.ts`)

`NativeNode` = a discriminated union over `TspKind` of `{ k: K; p?: TspProps<K>; c?: readonly NativeChild[]; key?: string; reveal?: "start"|"end"|"nearest" | {at, n}; scroll?: {by: TspScrollBy; n: number} }`. `NativeChild = NativeNode | Component`. A component child is a describe boundary; a component without `describe` becomes a `rows` fallback from `render(cols)`.

`DescribeContext { cols: number; reduceMotion: boolean; dark: boolean; hour12?: boolean; supports(kind: TspKind): boolean; feature(name: string): boolean }` — never a clock.

`NativeSurface { main: readonly NativeChild[]; dock: readonly NativeChild[] }`; `NativeScreen extends NativeSurface { role: string }`.

`NativeUiEvent` variants: `toggle{key,collapsed}`, `select{key,item}`, `activate{key,item}`, `action{key,act,value?,mods}`, `change{key,item,value}`, `edit{key} & NativeTextEdit`, `undo{key}`, `send{key,text}`.

`NativeTextEdit {from,to,text,cursor,len}` (UTF-16 offsets into text joined with `"\n"`); `resolveTextEdit(current, edit, clean, widen?)` returns `null` when `edit.len !== current.length` (stale), else clamps/swaps ends, never splits a surrogate pair, widens, cleans, maps caret; `ResolvedTextEdit {from,to,insert,text,cursor,changed}`. Helpers: `clampTextOffset`, `rootToggleExpanded`, `leafKey`, `NativeTerminalEvent`, `NativeEventTarget`, `NativeSurfaceProvider`.

### 6.3 Builders (`native/describe.ts`)

`node(k,p?,c?,key?)`, `span(t, s?, extra?)` (`s` = space-separated semantic/theme tokens), `text(content: string|TspSpan[], p?)`, `col(c,p?)`, `row(c,p?)`, `card(p,c)`, `md(source,p?)`, `code(source,p?)`, `ansi(output,p?)`, `diff(unified,p?)`, `EMPTY_NODE = node("col",{hidden:true})`, `keyed(described,key)`, `withHidden(described,hidden)`, `propNode(described): TspPropNode` (drops component children), `stableKey(identity) = Bun.hash(identity).toString(36)`, `compact(items)` (drops undefined/null/false), `kv(pairs)` (muted labels, skips empty, undefined if empty), `item(key,p)`, `list(items,p?,key?)`, `elapsed(age, stopped?)` → `node("elapsed",{age:round(ms), format:"short", stopped?})`, `keyNames(key)` (`"+"`/`"ctrl++"` special), `kbd(key,nodeKey?)`.

### 6.4 Blobs (`native/blobs.ts`)

`NativeBlob {id, mime, bytes: Uint8Array}`; id = hex sha256 of bytes. `registerNativeBlob(bytes,mime): string` (process-lifetime), `nativeBlob(bytes,mime)`, `withBlob(value,blob)` (adds `blob` id + symbol-keyed ownership), `getNativeBlob(id)`, `nativeImageNode(bytes,mime,p?,key?)`, `base64ImageNode(data,mimeType,p?,key?)` (probes pixel size), `openableImageProps(filePath)` → `{href: file://…, actions:{menu:["open"]}}` (always local file URL), `NativeImageCache.get(key,data,mime,sourcePath?)`, `readMediaPath(details)`.

### 6.5 State/settle/memo

- `state.ts`: `isNativeRendering()`, `setNativeRendering(on)`, `nativeButtons()/setNativeButtons`, `nativePacing()/setNativePacing`, `onNativeRenderingChange(listener): () => void`.
- `settle.ts`: `settleNative(component)`, `isNativeSettled(component)`.
- `memo.ts`: `sameItems(a,b)`, `sameProps(a,b)`, `Memo<T>` (keyed cache), `OwnerMemo<T>`.
- `measure.ts`: `estimateRows(root, cols, limit, described)`.
- `spans.ts`: `styledSpans(styled)`, `plainText`, `plainLine`, `styleSpans(content, style?)`, `compactText(spans)`, `rowsText(lines)`, `spansLine(content)`, `styledBackground(styled)`.
- `tone.ts`: `sampleBackground`, `sampleForeground`, `BackgroundChrome`, `backgroundChrome(bg)`, `toneColor(tone)`, `colorTone(color)`.
- `icons.ts`: `normalizeIconSpans`, `normalizeIconText`, `normalizeIconProps`.
- `quiet.ts`: `resetQuietMotion()`, `quietMotion(node)`.

### 6.6 Reconcile / apply / backend

- `reconcile.ts`: `NativeRegions`, `NativeTarget`, `nativeComponentId(component)`, `Reconciler` (assigns stable ids, diffs described trees into frame ops).
- `apply.ts`: `TspApplyError {s, op, msg}`; `TspDocument(surface)` with `focus`, `suspended`, `settled`, `size`, `has(id)`, `close()` (`x keep:true`: drops live-only `dock`/`layer` regions and focus, keeps `main`), and frame-op application with validation (a rejected op is reported and skipped; `settle` is a hint only).
- `backend.ts`: `NativeOverlay {component, options, focused}`; `NativeHost` (`describeSurface`, `overlays()`, `focused()`, `focusFromPointer`, `requestRender`, `appearanceChanged`, `motionChanged`, `invalidate`); `NativeBackendOptions {mirror?, recordPath?, stats?, scheduler?}`; `DEFAULT_SCHEDULER`; `RECENT_FRAMES=64`; `SHEET_FILL_FRACTION=0.5`; `assumedTspHello(terminal)`; `NativeBackend` — owns inline (`mode:"inline"`) and screen (`mode:"screen"`) surfaces, credit-based flow control (`TSP_DEFAULT_CREDITS`, `#limit` from hello `apc`), blob upload via verb `b`, event routing. Env overrides: `TERN_BLOB_DIR`, `PI_TUI_TSP_RECORD`, `PI_TUI_NATIVE_STATS`.
- `overlay.ts`: `NARROW_SHEET_COLS=100`, `NativeHint`, `actionHint`, `hintsRow`, `statusHintsRow`, `overlayCard`, `selectList`, `itemIndex`, `ActionButtonOptions`, `actionButton`.
- `picker.ts`: `picker`, `pickerQuery`, `PICKER_KEY="picker"`, `dockedPicker`, `pickerKeys`, `bindingKeys`, `pickerAction`, `CLOSE_ACTION = {id:"close",label:"Close",keys:["esc"],end:true}`, `pickerAge`, `pickerHits`.

---

## 7. `overlays/` + `overlays/extensions/`

- `extensions/types.ts`: `ExtensionProvider`, `ExtensionKind`, `ExtensionState = "active"|"disabled"|"shadowed"`, `DisabledReason`, `Extension`, `isShadowedExtension`, `TreeNodeType`, `TreeNode`, `FlatTreeItem`, `FocusRegion`.
- `state-manager.ts`: `resolveExtensionState`, `buildSidebarTree`, `flattenTree`, `applyFilter`, `buildProviderTabs`, `filterByProvider`, `applyDisabledExtensionsToState`, `createInitialState`, `refreshState`.
- `live-tool-session.ts`: `LiveToolInfo`, `LiveToolSessionLookup`, `liveToolRecordFromInfo`, `liveToolRecordFromSession`, `listLiveToolRecords`, `snapshotToolRuntimeSource`.
- `mcp-runtime.ts`: `MCPServerConfigDisplay`, `MCPServerDisplay`, `MCPImplementationDisplay`, `MCPResourceDisplay`, `MCPConnectionDisplay`, `MCPConnectionHealth`, `MCPRuntimeCatalogItem`, `MCPRuntimeSnapshot`, `MCPRuntimeSource`, `isDiscoveredMcpServer`.
- `inspector-panel.ts`: `InspectorPanel`; `extension-list.ts`: `ExtensionListCallbacks`, `ExtensionListPickerView`, `ExtensionListSwitch`, `ExtensionList`; `inspector-model.ts`: `LiveToolRecord`, `ToolRuntimeSource`, `ToolParamView`, `CommandPreview`, `isPlaceholderToolDescription`, `parseToolFileHeader`, `ExtensionInspectorSource`, `slashCommandFrontmatterDisplay`, `commandPreview`, `toolParamsFromSchema`; `display-text.ts`: `sanitizeDisplayText/SingleLine/Line/Field/LineField`; `extension-dashboard.ts`: `ExtensionDashboardRuntime`, `ExtensionDashboardOptions`, `buildTabBarTabs`, `buildPickerScopes`, `ExtensionDashboard`.
- Other overlays export selector/dashboard components: `session-selector.ts` (`SessionSelectorStatus`, `SessionSelectorEntry`, `SessionListing`, `systemDateLocale`, `SessionHistoryMatcher`, `rankSessionSearchMatches`, `mergeSessionRanking`, `SessionSelectorOptions`, `SessionSelectorComponent extends OverlayPanel`), `theme-selector.ts` (`themePickerOptions`, `ThemeSelectorComponent`), `settings-selector.ts` (`SettingsRuntimeContext`, `StatusLinePreviewSettings`, `SettingsCallbacks`, `SettingsSelectorComponent`), plus `usage-*`, `tree-selector`, `thinking-selector`, `spotlight-sheet`, `stats-notice`, `snapcompact-shape-preview`, `session-info-overlay`, `running-subagent-badge`, `rewind-selector`, `report-panel`, `reset-usage-selector`, `queue-mode-selector`, `plugin-selector`, `plugin-settings`, `plan-toc`, `plan-save-overlay`, `viewer-format`, `context-viewer-overview`, `composer-shape-preview/registry`, `cleanse-panel`, `codex-reset-fireworks`, `btw-history/inject/panel/history-panel`, `bordered-loader`, `approval-dialog`, `ask-dialog`, `annotation-overlay/types`, `agent-transcript-viewer`, `agents-hub`, `agent-hub{-projection,renderer,types}`, `advisor-config`, `agent-activity`.

---

## 8. `prompt/` + `setup/`

- `prompt/data/emojis.json` — emoji dataset consumed by emoji autocomplete.
- `prompt/queued-messages.ts`: `QueuedMessageGroup`, `QueuedMessagesBand extends Container`.
- Other prompt modules export composer/editor behavior: `word-completion`, `video`, `welcome`, `usage-amounts`, `tools-markdown`, `typing-word`, `queue-input`, `prompt-action-autocomplete`, `prose-gate`, `model-mention-autocomplete/syntax`, `markdown-prose`, `macos-spelling`, `magic-keywords`, `interactive-context-helpers`, `internal-url-autocomplete`, `image-source/references/format`, `gradient-highlight`, `emoji-autocomplete`, `github-ref-autocomplete`, `editor-top-gap`, `custom-editor`, `composer`, `composer-cache`, `composer-hints`, `composer-attachments`, `attachment-chips`, `skill-tokens`.
- `setup/scenes/types.ts`: `StyledLine`, `SetupUiHost`, `SetupHost`, `SetupSceneResult = "done"|"skipped"`, `SetupSceneHost`, `SetupSceneController extends Component`, `SETUP_SHEET_ROLE = "omp.setup.sheet"`, `SETUP_SURFACE_ROLE = "omp.app.setup"`, `SetupScene`.
- Scene modules export `SetupScene` objects: `splash.ts` (`SETUP_SPLASH_MS=2600`, `SETUP_TICK_MS=33`, `renderStarfield`, `renderSetupSplash`, `describeSetupSplash`), `sign-in.ts` (`SignInScene`, `providersSetupScene`), `model.ts` (`modelSetupScene`), `glyph.ts` (`glyphSetupScene`), `composer.ts` (`composerSetupScene`), `outro.ts` (`SETUP_OUTRO_MS=1200`, `describeSetupOutro`, `renderSetupOutro`), `updates.ts` (`updatesSetupScene`), `theme.ts` (`themeSetupScene`).
- `setup/wizard.ts`: re-exports scene types, `ALL_SCENES`, `SetupSceneSelectionOptions`, `setupWizardAllowed`, `selectSetupScenes`, `RunSetupWizardOptions`, `runSetupWizard`.
- `setup/wizard-overlay.ts`: `setupSceneApplies`, `SetupWizardComponent`.
- `setup/startup-splash.ts`: `RunStartupSplashOptions`, `runStartupSplash`.
- `setup/setup-version.ts`: `CURRENT_SETUP_VERSION = 3`. `setup/lazy.ts`: lazy scene loading.

---

## 9. `html-form/`

- `parse.ts`: `FormSpanStyle = "code"|"strong"|"em"`; `FormSpan`; `ChoiceOption {value,label: FormSpan[]}`; `ChoiceField`; `TextField`; `FormField = ChoiceField|TextField`; `FormBlock`; `FormSection`; `HtmlForm`; `parseHtmlForm(html): HtmlForm` (164); `spansText(spans): string` (195).
- `data.ts`: `HtmlFormValue = string|number|string[]`; `HtmlFormData = Record<string, HtmlFormValue>`; `FormAnswer {name, question, answer}`; `defaultFormData(form)`; `formAnswers(form, data)`; answers resolve option labels (whitespace-collapsed) or `OTHER_LABEL: <raw>`; arrays joined with `", "`.
- `dialog.ts`: `HtmlFormHost` (93), `HtmlFormDialogOptions` (101), `HtmlFormDialog implements Component, Focusable` (309).
- `paginate.ts`: `BreakRank` (const enum, 14), `PageAtom` (26), `Page` (36), `pageHeight(atoms,start,end)` (42), `paginate(atoms, rows): Page[]` (55).
- `media.ts`: `FormMedia` (16), `FormMediaOptions` (22), `MAX_FORM_IMAGE_BYTES = 10*1024*1024` (28), `svgMedia(markup)` (56), `isSvgMedia(media)` (70).

---

## 10. `charts/`

- `table-data.ts`: `Dimension = "count"|"percent"|"duration"|"bytes"|"currency"|"ratio"|"fraction"|"rate"`; `CellFit = "pure"|"note"|"range"|"arrow"|"fraction"|"prose"`; `NumberCell`; `OtherCell`; `Cell`; `ColumnRole = "measure"|"label"|"temporal"|"sequence"|"index"`; `TableColumn`; `Composition`; `TableAnalysis`; `Status = "good"|"bad"|"warn"|"pending"`.
- `chart-plan.ts`: `Polarity = "higher"|"lower"`; `Rivals = "rows"|"columns"`; `ChartKind`; `ChartPlan`; `RowReference`; `RowOrder`; `Shading = "shared"|"series"|"row"`; `ChartPoint`; `Bound = "max"|"min"`; `ChartTarget`.
- `chart-svg.ts`: `renderChartSvg(spec: ChartSpec): string` (242); `formatValue(value, dim, unit, magnitude = Math.abs(value))` (4331).

---

## Implementation notes / edge cases to preserve

1. **Terminal-default color is `""`**, not a hex. It renders as `\x1b[39m` (fg) / `\x1b[49m` (bg); `Theme.hasColor()` is false for it; `getColorHex` substitutes `#000000`/`#e5e5e7` (fg) and `#ffffff`/`#000000` (bg) by theme lightness.
2. **Nested resets**: `fg`/`fgOnBg`/`ansiFg`/`fgResolved` re-apply their SGR after every `\x1b[0m`/`\x1b[39m` inside the text; `bgFill` re-applies after `\x1b[0m`/`\x1b[49m`.
3. **Color mode**: `WT_SESSION` forces truecolor; otherwise terminal capability. Palette indices are emitted as `38;5;n`/`48;5;n`; hex as `ansi-16m`/`ansi-256` per `Bun.color` semantics.
4. **Theme lightness is judged on `statusLineBg`**, not `userMessageBg`.
5. **Symbol preset precedence**: settings override > theme `symbols.preset` > `"unicode"`; `status.done` aliases `status.success` unless overridden.
6. **Shimmer is terminal-clocked natively**; the ANSI path is frozen when native rendering is on.
7. **Session accent** is a pure function of (name, accentHex, surfaceLuminance, major color hexes); hue from the name hash, lightness/chroma carried from the theme accent re-normalized to the target hue's gamut cusp.
8. **CR handling**: `splitTerminalOutputLines` splits on `\r?\n` then keeps text after the last `\r` per line.
9. **Diff gutter** is at least 3 digits, constant, to keep streamed rows byte-identical.
10. **TSP framing** is `ESC _ tsp ; <verb> [;k=v]* ; <body> ESC \`; ConPTY input variant `ESC ] 877 ; tsp ; …`; chunking tags `c=<id>` and `m=1` with safe UTF-8/parameter boundaries.
11. **Status-line overflow** is deterministic: session_name truncate (min 8) → drop right from end → shrink path (min 4 chars, min width 8) → drop left from end, never dropping `path` while other left segments remain.

Checks to run after implementation: `cargo test` for the new crate plus a golden-file comparison of rendered theme/symbol tables against `dark.json`/`light.json`/`defaults.json`, and a byte-comparison of TSP framing/chunking against `encode.ts` vectors.