//! Two-dimensional layout engine for *display* LaTeX math.
//!
//! Reference: `l/src/latex-block.ts`.
//!
//! Deviations forced by Rust:
//!
//! - The reference indexes JavaScript UTF-16 code units; this port indexes
//!   `char`s (Unicode scalar values). Source positions are used only inside
//!   the parser, so the difference is invisible for the ASCII command and
//!   delimiter grammar and only affects surrogate-pair content offsets.
//! - `String::trim` follows Unicode `White_Space`, which is the practical
//!   equivalent of JavaScript's `trim` for LaTeX source.

use std::rc::Rc;

use crate::latex_to_unicode::{
    latex_color_scope, latex_to_unicode, ColorScope, MATH_FONT_COMMANDS,
};
use crate::native::describe::node;
use crate::native::node::{NativeNode, NodeKind, Props};
use crate::text_shape::visible_width;

const BAR: &str = "─";

const FRAC_COMMANDS: &[&str] = &["frac", "dfrac", "tfrac", "cfrac"];
const BINOM_COMMANDS: &[&str] = &["binom", "dbinom", "tbinom"];

/// Display "wrapper" environments whose body is an expression (possibly with
/// `\\` row breaks and `&` alignment).
const DISPLAY_ROW_ENVIRONMENTS: &[&str] = &[
    "equation",
    "eqnarray",
    "align",
    "aligned",
    "alignat",
    "alignedat",
    "flalign",
    "split",
    "gather",
    "gathered",
    "gatheredat",
    "multline",
    "displaymath",
    "math",
];

/// Operators whose display-style scripts stack above/below the symbol.
const LIMIT_OPERATORS: &[&str] = &[
    "sum", "prod", "coprod", "bigcup", "bigcap", "bigsqcup", "bigvee", "bigwedge", "bigoplus",
    "bigotimes", "bigodot", "biguplus", "lim", "limsup", "liminf", "projlim", "injlim",
    "varlimsup", "varliminf", "varprojlim", "varinjlim", "max", "min", "sup", "inf", "det", "gcd",
    "Pr", "argmax", "argmin",
];

/// Integral-family operators: scripts stay beside the symbol unless an
/// explicit `\limits` follows.
const INTEGRAL_OPERATORS: &[&str] = &[
    "int", "iint", "iiint", "iiiint", "oint", "oiint", "oiiint", "idotsint", "intop", "smallint",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CellAlign {
    L,
    C,
    R,
}

impl CellAlign {
    fn classify(c: char) -> Option<Self> {
        match c {
            'l' => Some(Self::L),
            'c' => Some(Self::C),
            'r' => Some(Self::R),
            _ => None,
        }
    }
}

/// A rectangular block of rendered text. Every line is padded to exactly
/// `width` visible columns; `baseline` is the row that aligns with the
/// surrounding text when boxes are placed side by side.
#[derive(Clone, Debug)]
struct MathBox {
    lines: Vec<String>,
    baseline: usize,
    width: usize,
}

#[derive(Clone, Copy, Debug)]
struct HBraceSpec {
    left: &'static str,
    mid: &'static str,
    center: &'static str,
    right: &'static str,
    over: bool,
}

const OVERBRACE: HBraceSpec = HBraceSpec {
    left: "╭",
    mid: "─",
    center: "┴",
    right: "╮",
    over: true,
};
const UNDERBRACE: HBraceSpec = HBraceSpec {
    left: "╰",
    mid: "─",
    center: "┬",
    right: "╯",
    over: false,
};
const OVERBRACKET: HBraceSpec = HBraceSpec {
    left: "┌",
    mid: "─",
    center: "─",
    right: "┐",
    over: true,
};
const UNDERBRACKET: HBraceSpec = HBraceSpec {
    left: "└",
    mid: "─",
    center: "─",
    right: "┘",
    over: false,
};
const OVERPAREN: HBraceSpec = HBraceSpec {
    left: "╭",
    mid: "─",
    center: "─",
    right: "╮",
    over: true,
};
const UNDERPAREN: HBraceSpec = HBraceSpec {
    left: "╰",
    mid: "─",
    center: "─",
    right: "╯",
    over: false,
};

fn hbrace_spec(name: &str) -> Option<HBraceSpec> {
    Some(match name {
        "overbrace" => OVERBRACE,
        "underbrace" => UNDERBRACE,
        "overbracket" => OVERBRACKET,
        "underbracket" => UNDERBRACKET,
        "overparen" => OVERPAREN,
        "underparen" => UNDERPAREN,
        _ => return None,
    })
}

fn command_arity(name: &str) -> Option<usize> {
    Some(match name {
        "overset" | "underset" | "stackrel" => 2,
        "sqrt" => 1,
        "frac" | "dfrac" | "tfrac" | "cfrac" | "binom" | "dbinom" | "tbinom" => 2,
        "overbrace" | "underbrace" | "overbracket" | "underbracket" | "overparen" | "underparen" => 1,
        _ => return None,
    })
}

fn grid_delims(base: &str) -> Option<(&'static str, &'static str)> {
    Some(match base {
        "matrix" | "smallmatrix" | "array" => ("", ""),
        "pmatrix" => ("(", ")"),
        "bmatrix" => ("[", "]"),
        "Bmatrix" => ("{", "}"),
        "vmatrix" => ("|", "|"),
        "Vmatrix" => ("‖", "‖"),
        "cases" | "dcases" => ("{", ""),
        "rcases" | "drcases" => ("", "}"),
        _ => return None,
    })
}

fn display_row_environment(base: &str) -> bool {
    DISPLAY_ROW_ENVIRONMENTS.contains(&base)
}

#[derive(Clone, Copy)]
struct DelimPieces {
    only: &'static str,
    top: &'static str,
    mid: &'static str,
    bot: &'static str,
    axis: Option<&'static str>,
}

const PAREN: DelimPieces = DelimPieces { only: "(", top: "⎛", mid: "⎜", bot: "⎝", axis: None };
const RPAREN: DelimPieces = DelimPieces { only: ")", top: "⎞", mid: "⎟", bot: "⎠", axis: None };
const BRACK: DelimPieces = DelimPieces { only: "[", top: "⎡", mid: "⎢", bot: "⎣", axis: None };
const RBRACK: DelimPieces = DelimPieces { only: "]", top: "⎤", mid: "⎥", bot: "⎦", axis: None };
const BRACE: DelimPieces = DelimPieces { only: "{", top: "⎧", mid: "⎪", bot: "⎩", axis: Some("⎨") };
const RBRACE: DelimPieces = DelimPieces { only: "}", top: "⎫", mid: "⎪", bot: "⎭", axis: Some("⎬") };
const PIPE: DelimPieces = DelimPieces { only: "|", top: "│", mid: "│", bot: "│", axis: None };
const DPIPE: DelimPieces = DelimPieces { only: "‖", top: "║", mid: "║", bot: "║", axis: None };
const LCEIL: DelimPieces = DelimPieces { only: "⌈", top: "⎡", mid: "⎢", bot: "⎢", axis: None };
const RCEIL: DelimPieces = DelimPieces { only: "⌉", top: "⎤", mid: "⎥", bot: "⎥", axis: None };
const LFLOOR: DelimPieces = DelimPieces { only: "⌊", top: "⎢", mid: "⎢", bot: "⎣", axis: None };
const RFLOOR: DelimPieces = DelimPieces { only: "⌋", top: "⎥", mid: "⎥", bot: "⎦", axis: None };

fn delim_pieces(key: &str) -> Option<&'static DelimPieces> {
    Some(match key {
        "(" => &PAREN,
        ")" => &RPAREN,
        "[" => &BRACK,
        "]" => &RBRACK,
        "{" => &BRACE,
        "}" => &RBRACE,
        "|" => &PIPE,
        "‖" => &DPIPE,
        "⌈" => &LCEIL,
        "⌉" => &RCEIL,
        "⌊" => &LFLOOR,
        "⌋" => &RFLOOR,
        _ => return None,
    })
}

fn delim_key(token: &str) -> String {
    let mapped = match token {
        "(" => Some("("),
        ")" => Some(")"),
        "[" => Some("["),
        "]" => Some("]"),
        "\\{" => Some("{"),
        "\\}" => Some("}"),
        "\\lbrace" => Some("{"),
        "\\rbrace" => Some("}"),
        "|" => Some("|"),
        "\\vert" => Some("|"),
        "\\lvert" => Some("|"),
        "\\rvert" => Some("|"),
        "\\|" => Some("‖"),
        "\\Vert" => Some("‖"),
        "\\lVert" => Some("‖"),
        "\\rVert" => Some("‖"),
        "\\langle" => Some("⟨"),
        "\\rangle" => Some("⟩"),
        "<" => Some("⟨"),
        ">" => Some("⟩"),
        "\\lceil" => Some("⌈"),
        "\\rceil" => Some("⌉"),
        "\\lfloor" => Some("⌊"),
        "\\rfloor" => Some("⌋"),
        "\\lbrack" => Some("["),
        "\\rbrack" => Some("]"),
        "." => Some(""),
        _ => None,
    };
    match mapped {
        Some(key) => key.to_string(),
        None => {
            if token.starts_with('\\') {
                latex_to_unicode(token).trim().to_string()
            } else {
                token.to_string()
            }
        }
    }
}

/// Inline-run conversion context. `wrap` re-applies the scoped commands (math
/// fonts, colors) active at this point in the parse.
#[derive(Clone)]
struct Ctx {
    wrap: Rc<dyn Fn(&str) -> String>,
}

fn root_ctx() -> Ctx {
    Ctx {
        wrap: Rc::new(|run: &str| run.to_string()),
    }
}

fn inner_ctx(ctx: &Ctx, color: &str) -> Ctx {
    if color.is_empty() {
        return ctx.clone();
    }
    let pre = color.to_string();
    let base = ctx.clone();
    Ctx {
        wrap: Rc::new(move |run: &str| (base.wrap)(&format!("{pre}{run}"))),
    }
}

fn at(src: &[char], i: usize) -> char {
    src.get(i).copied().unwrap_or('\0')
}

fn starts_with(src: &[char], i: usize, pat: &str) -> bool {
    let mut n = i;
    for ch in pat.chars() {
        if src.get(n).copied() != Some(ch) {
            return false;
        }
        n += 1;
    }
    true
}

fn slice_chars(src: &[char], start: usize, end: usize) -> String {
    let end = end.min(src.len());
    if start >= end {
        return String::new();
    }
    src[start..end].iter().collect()
}

fn find_char(src: &[char], from: usize, target: char) -> Option<usize> {
    (from..src.len()).find(|&i| src[i] == target)
}

fn spaces(n: usize) -> String {
    " ".repeat(n)
}

/// Pad `line` on the right to `width` visible columns.
fn pad_right(line: &str, width: usize) -> String {
    let extra = width.saturating_sub(visible_width(line));
    let mut out = String::with_capacity(line.len() + extra);
    out.push_str(line);
    out.push_str(&spaces(extra));
    out
}

/// Pad `line` symmetrically (left-biased) to `width` visible columns.
fn center(line: &str, width: usize) -> String {
    let extra = width.saturating_sub(visible_width(line));
    if extra == 0 {
        return line.to_string();
    }
    let left = extra >> 1;
    let mut out = String::with_capacity(line.len() + extra);
    out.push_str(&spaces(left));
    out.push_str(line);
    out.push_str(&spaces(extra - left));
    out
}

/// A single rendered string (possibly multi-line) as a baseline-centered box.
fn text_box(text: &str) -> MathBox {
    let raw: Vec<&str> = text.split('\n').collect();
    let width = raw.iter().map(|line| visible_width(line)).max().unwrap_or(0);
    let lines = raw.iter().map(|line| pad_right(line, width)).collect();
    MathBox {
        lines,
        baseline: (raw.len() - 1) >> 1,
        width,
    }
}

fn empty_box() -> MathBox {
    MathBox {
        lines: vec![String::new()],
        baseline: 0,
        width: 0,
    }
}

/// Pad every line of `b` to `width` per `align`, keeping the baseline.
fn pad_box(b: MathBox, width: usize, align: CellAlign) -> MathBox {
    if b.width >= width {
        return b;
    }
    let lines = b
        .lines
        .into_iter()
        .map(|line| {
            let extra = width.saturating_sub(visible_width(&line));
            match align {
                CellAlign::L => line + &spaces(extra),
                CellAlign::R => spaces(extra) + &line,
                CellAlign::C => {
                    let left = extra >> 1;
                    spaces(left) + &line + &spaces(extra - left)
                }
            }
        })
        .collect();
    MathBox {
        lines,
        baseline: b.baseline,
        width,
    }
}

/// Place boxes side by side, aligning their baselines.
fn hconcat(boxes: Vec<MathBox>) -> MathBox {
    if boxes.len() == 1 {
        return boxes.into_iter().next().unwrap();
    }
    let mut above = 0usize;
    let mut below = 0usize;
    for b in &boxes {
        above = above.max(b.baseline);
        below = below.max((b.lines.len() - 1).saturating_sub(b.baseline));
    }
    let height = above + below + 1;
    let width: usize = boxes.iter().map(|b| b.width).sum();
    let mut lines = Vec::with_capacity(height);
    for row in 0..height {
        let mut line = String::new();
        for b in &boxes {
            let local = row as isize - (above as isize - b.baseline as isize);
            if local >= 0 && (local as usize) < b.lines.len() {
                line.push_str(&b.lines[local as usize]);
            } else {
                line.push_str(&spaces(b.width));
            }
        }
        lines.push(line);
    }
    MathBox {
        lines,
        baseline: above,
        width,
    }
}

/// Stack boxes vertically, e.g. the rows of an aligned block.
fn vconcat(boxes: &[MathBox], align: CellAlign) -> MathBox {
    if boxes.len() == 1 {
        return boxes[0].clone();
    }
    let width = boxes.iter().map(|b| b.width).max().unwrap_or(0);
    let mut lines = Vec::new();
    for b in boxes {
        for line in &b.lines {
            if align == CellAlign::C {
                lines.push(center(line, width));
            } else {
                lines.push(pad_right(line, width));
            }
        }
    }
    let baseline = (lines.len() - 1) >> 1;
    MathBox {
        lines,
        baseline,
        width,
    }
}

/// Stack `num` over `den`, separated by a bar; the bar becomes the baseline.
fn frac_box(num: MathBox, den: MathBox) -> MathBox {
    let width = num.width.max(den.width) + 2;
    let mut lines = Vec::with_capacity(num.lines.len() + 1 + den.lines.len());
    for line in &num.lines {
        lines.push(center(line, width));
    }
    lines.push(BAR.repeat(width));
    for line in &den.lines {
        lines.push(center(line, width));
    }
    MathBox {
        baseline: num.lines.len(),
        lines,
        width,
    }
}

/// One vertical delimiter column of `height` rows for piece-table key `key`;
/// `None` when `key` is empty (`\left.`).
fn delim_column(key: &str, height: usize, baseline: usize) -> Option<MathBox> {
    if key.is_empty() {
        return None;
    }
    let pieces = delim_pieces(key);
    if height <= 1 {
        let only = pieces.map(|p| p.only).unwrap_or(key);
        if only.is_empty() {
            return None;
        }
        let width = visible_width(only);
        return Some(MathBox {
            lines: vec![only.to_string()],
            baseline: 0,
            width,
        });
    }
    let width = visible_width(pieces.map(|p| p.only).unwrap_or(key));
    let blank = spaces(width);
    let mut lines = Vec::with_capacity(height);
    let Some(pieces) = pieces else {
        for y in 0..height {
            lines.push(if y == baseline { key.to_string() } else { blank.clone() });
        }
        return Some(MathBox {
            lines,
            baseline,
            width,
        });
    };
    let axis_row = baseline.max(1).min(height.saturating_sub(2));
    for y in 0..height {
        if y == 0 {
            lines.push(pieces.top.to_string());
        } else if y == height - 1 {
            lines.push(pieces.bot.to_string());
        } else if y == axis_row && pieces.axis.is_some() {
            lines.push(pieces.axis.unwrap().to_string());
        } else {
            lines.push(pieces.mid.to_string());
        }
    }
    Some(MathBox {
        lines,
        baseline,
        width,
    })
}

/// Wrap `inner` in (possibly stretched) delimiters, padding tall content.
fn delim_box(inner: MathBox, left: &str, right: &str) -> MathBox {
    let height = inner.lines.len();
    let lcol = delim_column(left, height, inner.baseline);
    let rcol = delim_column(right, height, inner.baseline);
    if lcol.is_none() && rcol.is_none() {
        return inner;
    }
    let pad = if height > 1 { Some(text_box(" ")) } else { None };
    let mut parts = Vec::new();
    if let Some(col) = lcol {
        parts.push(col);
    }
    if let Some(pad) = &pad {
        parts.push(pad.clone());
    }
    parts.push(inner);
    if let Some(pad) = &pad {
        parts.push(pad.clone());
    }
    if let Some(col) = rcol {
        parts.push(col);
    }
    hconcat(parts)
}

/// `\binom{n}{k}`: `n` over `k` (no bar) inside stretched parentheses.
fn binom_box(top: MathBox, bottom: MathBox) -> MathBox {
    let width = top.width.max(bottom.width);
    let mut lines = Vec::with_capacity(top.lines.len() + 1 + bottom.lines.len());
    for line in &top.lines {
        lines.push(center(line, width));
    }
    lines.push(spaces(width));
    for line in &bottom.lines {
        lines.push(center(line, width));
    }
    let baseline = top.lines.len();
    delim_box(
        MathBox {
            lines,
            baseline,
            width,
        },
        "(",
        ")",
    )
}

/// A drawn radical for a multi-line radicand: overline row on top, bar column
/// on the left, hook at the bottom.
fn radical_box(
    inner: MathBox,
    degree: Option<&str>,
    wrap: &Rc<dyn Fn(&str) -> String>,
) -> MathBox {
    let mut lines = Vec::with_capacity(inner.lines.len() + 1);
    lines.push(format!(" ┌{}", BAR.repeat(inner.width + 1)));
    for (y, line) in inner.lines.iter().enumerate() {
        let prefix = if y == inner.lines.len() - 1 { "╲│ " } else { " │ " };
        lines.push(format!("{prefix}{line}"));
    }
    let box_width = inner.width + 3;
    let baseline = inner.baseline + 1;
    let base = MathBox {
        lines,
        baseline,
        width: box_width,
    };
    let Some(degree) = degree else {
        return base;
    };
    let deg = latex_to_unicode(&wrap(&format!("^{{{degree}}}")));
    let deg_width = visible_width(&deg);
    let deg_box = MathBox {
        lines: vec![deg.clone(), spaces(deg_width)],
        baseline: 1,
        width: deg_width,
    };
    hconcat(vec![deg_box, base])
}

/// Big operator with limits: `sup` centered above `glyph`, `sub` below.
fn limits_box(glyph: MathBox, sub: Option<MathBox>, sup: Option<MathBox>) -> MathBox {
    let width = glyph
        .width
        .max(sub.as_ref().map(|b| b.width).unwrap_or(0))
        .max(sup.as_ref().map(|b| b.width).unwrap_or(0));
    let mut lines = Vec::new();
    if let Some(sup) = &sup {
        for line in &sup.lines {
            lines.push(center(line, width));
        }
    }
    let baseline = lines.len() + glyph.baseline;
    for line in &glyph.lines {
        lines.push(center(line, width));
    }
    if let Some(sub) = &sub {
        for line in &sub.lines {
            lines.push(center(line, width));
        }
    }
    MathBox {
        lines,
        baseline,
        width,
    }
}

/// `\underbrace{content}_{label}` / `\overbrace{content}^{label}`.
fn hbrace_box(content: MathBox, spec: HBraceSpec, label: Option<MathBox>) -> MathBox {
    let brace_width = content.width.max(3);
    let width = brace_width.max(label.as_ref().map(|b| b.width).unwrap_or(0));
    let lead = (brace_width - 3) >> 1;
    let mut brace = String::new();
    brace.push_str(spec.left);
    brace.push_str(&spec.mid.repeat(lead));
    brace.push_str(spec.center);
    brace.push_str(&spec.mid.repeat(brace_width - 3 - lead));
    brace.push_str(spec.right);
    let brace = center(&brace, width);
    let content_lines: Vec<String> = content.lines.iter().map(|line| center(line, width)).collect();
    let label_lines: Vec<String> = match &label {
        Some(label) => label.lines.iter().map(|line| center(line, width)).collect(),
        None => Vec::new(),
    };
    if spec.over {
        let mut lines = label_lines;
        lines.push(brace);
        lines.extend(content_lines);
        MathBox {
            baseline: label_len(&label) + 1 + content.baseline,
            lines,
            width,
        }
    } else {
        let mut lines = content_lines;
        lines.push(brace);
        lines.extend(label_lines);
        MathBox {
            baseline: content.baseline,
            lines,
            width,
        }
    }
}

fn label_len(label: &Option<MathBox>) -> usize {
    label.as_ref().map(|b| b.lines.len()).unwrap_or(0)
}

/// Attach block scripts to `base` as one shared right-hand column.
fn attach_scripts(base: MathBox, sub: Option<MathBox>, sup: Option<MathBox>) -> MathBox {
    if sub.is_none() && sup.is_none() {
        return base;
    }
    let single = base.lines.len() == 1;
    let width = sub
        .as_ref()
        .map(|b| b.width)
        .unwrap_or(0)
        .max(sup.as_ref().map(|b| b.width).unwrap_or(0));
    let blank = spaces(width);
    let mut lines: Vec<String> = Vec::new();
    let mut baseline = 0usize;
    if let Some(sup) = &sup {
        let lift = if single { 1 } else { base.baseline };
        for line in &sup.lines {
            lines.push(pad_right(line, width));
        }
        for _ in 0..lift {
            lines.push(blank.clone());
        }
        baseline = lines.len() - 1;
    }
    if let Some(sub) = &sub {
        let below = (base.lines.len() - 1)
            .saturating_sub(base.baseline)
            .saturating_sub(sub.lines.len() - 1);
        let mut drop = below.max(if single { 1 } else { 0 });
        if sup.is_some() && drop < 1 {
            drop = 1;
        }
        let gap = if lines.is_empty() { drop } else { drop - 1 };
        for _ in 0..gap {
            lines.push(blank.clone());
        }
        for line in &sub.lines {
            lines.push(pad_right(line, width));
        }
    }
    let column = MathBox {
        lines,
        baseline,
        width,
    };
    hconcat(vec![base, column])
}

/// Lay out parsed cells as a grid.
fn grid_box(
    rows: &[Vec<MathBox>],
    align: &dyn Fn(usize) -> CellAlign,
    gap: &dyn Fn(usize) -> usize,
    row_gap: usize,
) -> MathBox {
    let mut ncols = 0usize;
    for row in rows {
        ncols = ncols.max(row.len());
    }
    if ncols == 0 || rows.is_empty() {
        return text_box("");
    }
    let mut widths = vec![0usize; ncols];
    for row in rows {
        for (j, cell) in row.iter().enumerate() {
            widths[j] = widths[j].max(cell.width);
        }
    }
    let mut row_boxes: Vec<MathBox> = Vec::new();
    for row in rows {
        if row_gap > 0 && !row_boxes.is_empty() {
            for _ in 0..row_gap {
                row_boxes.push(empty_box());
            }
        }
        let mut parts: Vec<MathBox> = Vec::new();
        for j in 0..ncols {
            if j > 0 {
                let g = gap(j);
                if g > 0 {
                    parts.push(MathBox {
                        lines: vec![spaces(g)],
                        baseline: 0,
                        width: g,
                    });
                }
            }
            let cell = row.get(j).cloned().unwrap_or_else(empty_box);
            parts.push(pad_box(cell, widths[j], align(j)));
        }
        row_boxes.push(hconcat(parts));
    }
    let grid = vconcat(&row_boxes, CellAlign::L);
    if row_gap > 0 && rows.len() > 1 && grid.lines.len() % 2 == 0 {
        let baseline = grid.lines.len() >> 1;
        let mut lines = grid.lines;
        lines.push(spaces(grid.width));
        return MathBox {
            lines,
            baseline,
            width: grid.width,
        };
    }
    grid
}

struct Span {
    text: String,
    end: usize,
}

/// Read a balanced `{…}` beginning at `i` (which must point at `{`).
fn read_brace_group(src: &[char], i: usize) -> Span {
    let mut depth = 0usize;
    let mut out = String::new();
    let mut j = i;
    while j < src.len() {
        let c = src[j];
        if c == '\\' {
            out.push(c);
            if let Some(next) = src.get(j + 1) {
                out.push(*next);
            }
            j += 2;
            continue;
        }
        if c == '{' {
            depth += 1;
            if depth > 1 {
                out.push(c);
            }
            j += 1;
            continue;
        }
        if c == '}' {
            depth -= 1;
            if depth == 0 {
                j += 1;
                break;
            }
            out.push(c);
            j += 1;
            continue;
        }
        out.push(c);
        j += 1;
    }
    Span {
        text: out,
        end: j,
    }
}

/// Read one command argument: a `{…}` group, a single char, or a `\command`
/// together with its arguments (or whole `\begin…\end` block).
fn read_arg(src: &[char], i: usize) -> Span {
    let mut i = i;
    while matches!(at(src, i), ' ' | '\t' | '\n') {
        i += 1;
    }
    if i >= src.len() {
        return Span {
            text: String::new(),
            end: i,
        };
    }
    if src[i] == '{' {
        return read_brace_group(src, i);
    }
    if src[i] != '\\' {
        return Span {
            text: src[i].to_string(),
            end: i + 1,
        };
    }
    let mut j = i + 1;
    let mut name = String::new();
    while j < src.len() && src[j].is_ascii_alphabetic() {
        name.push(src[j]);
        j += 1;
    }
    if name == "begin" {
        if let Some(env) = consume_environment(src, i) {
            return env;
        }
    }
    if name.is_empty() {
        let end = (i + 2).min(src.len());
        return Span {
            text: slice_chars(src, i, end),
            end,
        };
    }
    if let Some(arity) = command_arity(&name) {
        let mut end = j;
        loop {
            while matches!(at(src, end), ' ' | '\t' | '\n') {
                end += 1;
            }
            if at(src, end) != '[' {
                break;
            }
            match find_char(src, end, ']') {
                Some(close) => end = close + 1,
                None => end = src.len(),
            }
        }
        for _ in 0..arity {
            end = read_arg(src, end).end;
        }
        return Span {
            text: slice_chars(src, i, end),
            end,
        };
    }
    let mut end = j;
    while matches!(at(src, end), '[' | '{') {
        if src[end] == '{' {
            end = read_brace_group(src, end).end;
        } else {
            match find_char(src, end, ']') {
                Some(close) => end = close + 1,
                None => end = src.len(),
            }
        }
    }
    Span {
        text: slice_chars(src, i, end),
        end,
    }
}

/// Read a `\left`/`\right`/`\middle` delimiter token (char or `\command`).
fn read_delim_token(src: &[char], i: usize) -> Option<Span> {
    let mut i = i;
    while at(src, i) == ' ' {
        i += 1;
    }
    if i >= src.len() {
        return None;
    }
    if src[i] != '\\' {
        return Some(Span {
            text: src[i].to_string(),
            end: i + 1,
        });
    }
    let mut j = i + 1;
    if !at(src, j).is_ascii_alphabetic() {
        let end = (j + 1).min(src.len());
        return Some(Span {
            text: slice_chars(src, i, end),
            end,
        });
    }
    while j < src.len() && src[j].is_ascii_alphabetic() {
        j += 1;
    }
    Some(Span {
        text: slice_chars(src, i, j),
        end: j,
    })
}

struct LeftRightParts {
    left: String,
    segments: Vec<String>,
    middles: Vec<String>,
    right: String,
    end: usize,
}

/// Parse `\left⟨tok⟩ … \right⟨tok⟩` starting at the backslash of `\left`.
fn read_left_right(src: &[char], start: usize) -> Option<LeftRightParts> {
    let left = read_delim_token(src, start + 5)?;
    let mut segments = Vec::new();
    let mut middles = Vec::new();
    let mut depth = 1usize;
    let mut k = left.end;
    let mut seg_start = k;
    while k < src.len() {
        if src[k] != '\\' {
            k += 1;
            continue;
        }
        if starts_with(src, k, "\\left") && !at(src, k + 5).is_ascii_alphabetic() {
            depth += 1;
            let tok = read_delim_token(src, k + 5);
            k = tok.as_ref().map(|t| t.end).unwrap_or(k + 5);
            continue;
        }
        if starts_with(src, k, "\\right") && !at(src, k + 6).is_ascii_alphabetic() {
            depth -= 1;
            let tok = read_delim_token(src, k + 6);
            if depth == 0 {
                segments.push(slice_chars(src, seg_start, k));
                return Some(LeftRightParts {
                    left: left.text.clone(),
                    segments,
                    middles,
                    right: tok.as_ref().map(|t| t.text.clone()).unwrap_or_else(|| ".".to_string()),
                    end: tok.as_ref().map(|t| t.end).unwrap_or(k + 6),
                });
            }
            k = tok.as_ref().map(|t| t.end).unwrap_or(k + 6);
            continue;
        }
        if depth == 1 && starts_with(src, k, "\\middle") && !at(src, k + 7).is_ascii_alphabetic() {
            segments.push(slice_chars(src, seg_start, k));
            let tok = read_delim_token(src, k + 7);
            middles.push(tok.as_ref().map(|t| t.text.clone()).unwrap_or_else(|| "|".to_string()));
            k = tok.as_ref().map(|t| t.end).unwrap_or(k + 7);
            seg_start = k;
            continue;
        }
        k += 2;
    }
    None
}

/// Index of the `close` matching the `open` at `i`, skipping escapes and brace
/// groups; `None` when unbalanced.
fn match_delim(src: &[char], i: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut k = i;
    while k < src.len() {
        let c = src[k];
        if c == '\\' {
            k += 2;
            continue;
        }
        if c == '{' {
            k = read_brace_group(src, k).end;
            continue;
        }
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(k);
            }
        }
        k += 1;
    }
    None
}

struct EnvParts {
    env: String,
    body_start: usize,
    body_end: usize,
    end: usize,
}

/// Locate a `\begin{env}…\end{env}` block (balanced) starting at the backslash.
fn read_environment(src: &[char], start: usize) -> Option<EnvParts> {
    let mut i = start + 6;
    while at(src, i) == ' ' {
        i += 1;
    }
    if at(src, i) != '{' {
        return None;
    }
    let name_group = read_brace_group(src, i);
    let mut k = name_group.end;
    let mut depth = 1usize;
    let mut body_end = src.len();
    while k < src.len() && depth > 0 {
        if starts_with(src, k, "\\begin") {
            depth += 1;
            k += 6;
            continue;
        }
        if starts_with(src, k, "\\end") {
            depth -= 1;
            if depth == 0 {
                body_end = k;
            }
            k += 4;
            while at(src, k) == ' ' {
                k += 1;
            }
            if at(src, k) == '{' {
                k = read_brace_group(src, k).end;
            }
            if depth == 0 {
                break;
            }
            continue;
        }
        k += 1;
    }
    Some(EnvParts {
        env: name_group.text.trim().to_string(),
        body_start: name_group.end,
        body_end,
        end: k,
    })
}

/// The full `\begin{env}…\end{env}` substring as an inline run.
fn consume_environment(src: &[char], start: usize) -> Option<Span> {
    let env = read_environment(src, start)?;
    Some(Span {
        text: slice_chars(src, start, env.end),
        end: env.end,
    })
}

/// Split an environment body on top-level `\\` row breaks (depth-aware).
fn split_rows(body: &[char]) -> Vec<String> {
    let mut rows = Vec::new();
    let mut brace_depth: i64 = 0;
    let mut env_depth: i64 = 0;
    let mut last = 0usize;
    let mut i = 0usize;
    while i < body.len() {
        if starts_with(body, i, "\\begin") {
            env_depth += 1;
            i += 6;
            continue;
        }
        if starts_with(body, i, "\\end") {
            env_depth -= 1;
            i += 4;
            continue;
        }
        let c = body[i];
        if c == '\\' {
            if at(body, i + 1) == '\\' && brace_depth == 0 && env_depth == 0 {
                rows.push(slice_chars(body, last, i));
                i += 2;
                while at(body, i) == ' ' {
                    i += 1;
                }
                if at(body, i) == '[' {
                    match find_char(body, i, ']') {
                        Some(close) => i = close + 1,
                        None => i = body.len(),
                    }
                }
                last = i;
                continue;
            }
            i += 2;
            continue;
        }
        if c == '{' {
            brace_depth += 1;
        } else if c == '}' {
            brace_depth -= 1;
        }
        i += 1;
    }
    rows.push(slice_chars(body, last, body.len()));
    rows
}

/// Split a row on top-level `&` column separators (depth-aware), trimming cells.
fn split_cells(row: &str) -> Vec<String> {
    let chars: Vec<char> = row.chars().collect();
    let mut cells = Vec::new();
    let mut brace_depth: i64 = 0;
    let mut env_depth: i64 = 0;
    let mut last = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        if starts_with(&chars, i, "\\begin") {
            env_depth += 1;
            i += 6;
            continue;
        }
        if starts_with(&chars, i, "\\end") {
            env_depth -= 1;
            i += 4;
            continue;
        }
        let c = chars[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        if c == '{' {
            brace_depth += 1;
        } else if c == '}' {
            brace_depth -= 1;
        } else if c == '&' && brace_depth == 0 && env_depth == 0 {
            cells.push(slice_chars(&chars, last, i));
            last = i + 1;
        }
        i += 1;
    }
    cells.push(slice_chars(&chars, last, chars.len()));
    cells.iter().map(|cell| cell.trim().to_string()).collect()
}

/// Append a script (`^`/`_`) and its argument to the inline run verbatim.
fn read_script(src: &[char], i: usize) -> Span {
    let mut out = src[i].to_string();
    let mut i = i + 1;
    while at(src, i) == ' ' {
        out.push(' ');
        i += 1;
    }
    if at(src, i) == '{' {
        let group = read_brace_group(src, i);
        return Span {
            text: format!("{out}{{{}}}", group.text),
            end: group.end,
        };
    }
    if at(src, i) == '\\' {
        let mut j = i + 1;
        if at(src, j).is_ascii_alphabetic() {
            while at(src, j).is_ascii_alphabetic() {
                j += 1;
            }
        } else {
            j += 1;
        }
        return Span {
            text: out + &slice_chars(src, i, j),
            end: j,
        };
    }
    if i < src.len() {
        return Span {
            text: out + &src[i].to_string(),
            end: i + 1,
        };
    }
    Span { text: out, end: i }
}

/// Bare argument of a script read by `read_script`.
fn script_arg_of(text: &str) -> String {
    let mut chars = text.chars();
    chars.next();
    let arg = chars.as_str().trim_start();
    if arg.starts_with('{') && arg.ends_with('}') && arg.chars().count() >= 2 {
        let mut inner = arg.chars();
        inner.next();
        inner.next_back();
        inner.as_str().to_string()
    } else {
        arg.to_string()
    }
}

fn unconvertible(raw: Option<&str>) -> bool {
    match raw {
        Some(raw) => {
            let flat = latex_to_unicode(raw);
            flat.starts_with('^') || flat.starts_with('_')
        }
        None => false,
    }
}

/// Multi-letter script words would convert per-char into Unicode glyphs of
/// uneven height; commands are stripped first.
fn ragged(raw: Option<&str>) -> bool {
    let Some(raw) = raw else { return false };
    let arg = script_arg_of(raw);
    let mut chars = arg.chars().peekable();
    let mut letters = 0usize;
    while let Some(c) = chars.next() {
        if c == '\\' {
            while matches!(chars.peek(), Some(n) if n.is_ascii_alphabetic()) {
                chars.next();
            }
            continue;
        }
        if c.is_ascii_alphabetic() {
            letters += 1;
        }
    }
    letters >= 2
}

fn paint_box(b: MathBox, scope: &Option<ColorScope>) -> MathBox {
    match scope {
        Some(scope) => colorize_box(b, scope),
        None => b,
    }
}

/// Paint every line of `box` through a color scope.
fn colorize_box(b: MathBox, scope: &ColorScope) -> MathBox {
    MathBox {
        lines: b.lines.iter().map(|line| scope.paint(line)).collect(),
        baseline: b.baseline,
        width: b.width,
    }
}

/// Render a `\begin{env}…\end{env}` block.
fn parse_environment(src: &[char], start: usize, ctx: &Ctx) -> Option<(MathBox, usize)> {
    let env = read_environment(src, start)?;
    let (starred, base) = match env.env.strip_suffix('*') {
        Some(base) => (true, base),
        None => (false, env.env.as_str()),
    };
    if let Some((open, close)) = grid_delims(base) {
        let mut p = env.body_start;
        while matches!(at(src, p), ' ' | '\n' | '\t') {
            p += 1;
        }
        if starred && at(src, p) == '[' {
            if let Some(close_idx) = find_char(src, p, ']') {
                if close_idx < env.body_end {
                    p = close_idx + 1;
                    while matches!(at(src, p), ' ' | '\n' | '\t') {
                        p += 1;
                    }
                }
            }
        }
        let mut col_spec: Option<Vec<CellAlign>> = None;
        if base == "array" && at(src, p) == '{' {
            let spec = read_brace_group(src, p);
            col_spec = Some(spec.text.chars().filter_map(CellAlign::classify).collect());
            p = spec.end;
        }
        let cells: Vec<Vec<MathBox>> = split_rows(&src[p..env.body_end])
            .iter()
            .map(|row| row.trim())
            .filter(|row| !row.is_empty())
            .map(|row| {
                split_cells(row)
                    .iter()
                    .map(|cell| parse_expr_str(cell, ctx))
                    .collect()
            })
            .collect();
        let is_cases = matches!(base, "cases" | "dcases" | "rcases" | "drcases");
        let spec = col_spec.clone();
        let align: Box<dyn Fn(usize) -> CellAlign> = if let Some(spec) = spec {
            Box::new(move |col| spec.get(col).copied().unwrap_or(CellAlign::C))
        } else if is_cases {
            Box::new(|_| CellAlign::L)
        } else {
            Box::new(|_| CellAlign::C)
        };
        let grid = grid_box(&cells, &align, &|_| 2, 1);
        return Some((delim_box(grid, open, close), env.end));
    }
    if !display_row_environment(base) {
        let text = latex_to_unicode(&(ctx.wrap)(&slice_chars(src, start, env.end)));
        return Some((text_box(&text), env.end));
    }
    let mut body_start = env.body_start;
    if matches!(base, "alignat" | "alignedat" | "gatheredat") {
        let mut p = body_start;
        while matches!(at(src, p), ' ' | '\n') {
            p += 1;
        }
        if at(src, p) == '{' {
            body_start = read_brace_group(src, p).end;
        }
    }
    let rows: Vec<String> = split_rows(&src[body_start..env.body_end])
        .iter()
        .map(|row| row.trim().to_string())
        .filter(|row| !row.is_empty())
        .collect();
    if rows.is_empty() {
        return Some((text_box(""), env.end));
    }
    let cell_rows: Vec<Vec<String>> = rows.iter().map(|row| split_cells(row)).collect();
    let ncols = cell_rows.iter().map(|row| row.len()).max().unwrap_or(0);
    if ncols <= 1 {
        let centered = matches!(base, "gather" | "gathered" | "multline");
        let boxes: Vec<MathBox> = rows.iter().map(|row| parse_expr_str(row, ctx)).collect();
        return Some((
            vconcat(&boxes, if centered { CellAlign::C } else { CellAlign::L }),
            env.end,
        ));
    }
    let cells: Vec<Vec<MathBox>> = cell_rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| parse_expr_str(cell, ctx))
                .collect()
        })
        .collect();
    let align: Box<dyn Fn(usize) -> CellAlign> = Box::new(|col| {
        if col % 2 == 0 {
            CellAlign::R
        } else {
            CellAlign::L
        }
    });
    let gap = Box::new(|col: usize| if col % 2 == 1 { 1 } else { 3 });
    let grid = grid_box(&cells, &align, &gap, 0);
    Some((grid, env.end))
}

fn flush_run(boxes: &mut Vec<MathBox>, inline: &mut String, color: &str, ctx: &Ctx) {
    if inline.is_empty() {
        return;
    }
    let text = latex_to_unicode(&(ctx.wrap)(&format!("{color}{inline}")));
    boxes.push(text_box(&text));
    inline.clear();
}

fn parse_expr_str(src: &str, ctx: &Ctx) -> MathBox {
    let chars: Vec<char> = src.chars().collect();
    parse_expr_with_ctx(&chars, ctx)
}

/// Parse a math fragment into a layout box.
fn parse_expr_with_ctx(src: &[char], ctx: &Ctx) -> MathBox {
    let mut boxes: Vec<MathBox> = Vec::new();
    let mut inline = String::new();
    let mut color = String::new();
    let mut color_scope: Option<ColorScope> = None;
    let mut i = 0usize;
    while i < src.len() {
        let c = src[i];
        if c == '\\' {
            let mut j = i + 1;
            let mut name = String::new();
            while j < src.len() && src[j].is_ascii_alphabetic() {
                name.push(src[j]);
                j += 1;
            }
            if !name.is_empty() && FRAC_COMMANDS.contains(&name.as_str()) {
                flush_run(&mut boxes, &mut inline, &color, ctx);
                let inner = inner_ctx(ctx, &color);
                let num = read_arg(src, j);
                let den = read_arg(src, num.end);
                let num_box = parse_expr_str(&num.text, &inner);
                let den_box = parse_expr_str(&den.text, &inner);
                boxes.push(paint_box(frac_box(num_box, den_box), &color_scope));
                i = den.end;
                continue;
            }
            if !name.is_empty() && BINOM_COMMANDS.contains(&name.as_str()) {
                flush_run(&mut boxes, &mut inline, &color, ctx);
                let inner = inner_ctx(ctx, &color);
                let top = read_arg(src, j);
                let bottom = read_arg(src, top.end);
                let top_box = parse_expr_str(&top.text, &inner);
                let bottom_box = parse_expr_str(&bottom.text, &inner);
                boxes.push(paint_box(binom_box(top_box, bottom_box), &color_scope));
                i = bottom.end;
                continue;
            }
            if let Some(spec) = (!name.is_empty()).then(|| hbrace_spec(&name)).flatten() {
                flush_run(&mut boxes, &mut inline, &color, ctx);
                let inner = inner_ctx(ctx, &color);
                let arg = read_arg(src, j);
                let mut sub_text: Option<String> = None;
                let mut sup_text: Option<String> = None;
                let mut m = arg.end;
                loop {
                    let mut n = m;
                    while at(src, n) == ' ' {
                        n += 1;
                    }
                    if at(src, n) == '_' && sub_text.is_none() {
                        let s = read_arg(src, n + 1);
                        sub_text = Some(s.text);
                        m = s.end;
                        continue;
                    }
                    if at(src, n) == '^' && sup_text.is_none() {
                        let s = read_arg(src, n + 1);
                        sup_text = Some(s.text);
                        m = s.end;
                        continue;
                    }
                    break;
                }
                let label_text = if spec.over { sup_text.clone() } else { sub_text.clone() };
                let other_text = if spec.over { sub_text } else { sup_text };
                let label_box = label_text.map(|text| parse_expr_str(&text, &inner));
                let mut box_ = hbrace_box(parse_expr_str(&arg.text, &inner), spec, label_box);
                if let Some(other) = other_text {
                    let other_box = parse_expr_str(&other, &inner);
                    let (sub_attach, sup_attach) = if spec.over {
                        (None, Some(other_box))
                    } else {
                        (Some(other_box), None)
                    };
                    box_ = attach_scripts(box_, sub_attach, sup_attach);
                }
                boxes.push(paint_box(box_, &color_scope));
                i = m;
                continue;
            }
            if name == "overset" || name == "underset" || name == "stackrel" {
                flush_run(&mut boxes, &mut inline, &color, ctx);
                let inner = inner_ctx(ctx, &color);
                let anno = read_arg(src, j);
                let base = read_arg(src, anno.end);
                let anno_box = parse_expr_str(&anno.text, &inner);
                let base_box = parse_expr_str(&base.text, &inner);
                let (sub, sup) = if name == "underset" {
                    (Some(anno_box), None)
                } else {
                    (None, Some(anno_box))
                };
                boxes.push(paint_box(limits_box(base_box, sub, sup), &color_scope));
                i = base.end;
                continue;
            }
            if name == "sqrt" {
                let mut k = j;
                while at(src, k) == ' ' {
                    k += 1;
                }
                let mut degree: Option<String> = None;
                if at(src, k) == '[' {
                    match find_char(src, k, ']') {
                        Some(close) => {
                            degree = Some(slice_chars(src, k + 1, close));
                            k = close + 1;
                        }
                        None => {
                            degree = Some(slice_chars(src, k + 1, src.len()));
                            k = src.len();
                        }
                    }
                }
                let arg = read_arg(src, k);
                flush_run(&mut boxes, &mut inline, &color, ctx);
                let inner = inner_ctx(ctx, &color);
                boxes.push(paint_box(
                    radical_box(parse_expr_str(&arg.text, &inner), degree.as_deref(), &ctx.wrap),
                    &color_scope,
                ));
                i = arg.end;
                continue;
            }
            if name == "left" {
                if let Some(lr) = read_left_right(src, i) {
                    let inner = inner_ctx(ctx, &color);
                    let seg_boxes: Vec<MathBox> = lr
                        .segments
                        .iter()
                        .map(|segment| parse_expr_str(segment, &inner))
                        .collect();
                    let mut above = 0usize;
                    let mut below = 0usize;
                    for b in &seg_boxes {
                        above = above.max(b.baseline);
                        below = below.max((b.lines.len() - 1).saturating_sub(b.baseline));
                    }
                    let height = above + below + 1;
                    if height == 1 {
                        inline.push_str(&slice_chars(src, i, lr.end));
                        i = lr.end;
                        continue;
                    }
                    flush_run(&mut boxes, &mut inline, &color, ctx);
                    let mut parts: Vec<MathBox> = Vec::new();
                    if let Some(col) = delim_column(&delim_key(&lr.left), height, above) {
                        parts.push(col);
                    }
                    for (s, segment) in seg_boxes.into_iter().enumerate() {
                        parts.push(segment);
                        if s < lr.middles.len() {
                            if let Some(col) = delim_column(&delim_key(&lr.middles[s]), height, above) {
                                parts.push(col);
                            }
                        }
                    }
                    if let Some(col) = delim_column(&delim_key(&lr.right), height, above) {
                        parts.push(col);
                    }
                    boxes.push(paint_box(hconcat(parts), &color_scope));
                    i = lr.end;
                    continue;
                }
            }
            if !name.is_empty() && (LIMIT_OPERATORS.contains(&name.as_str()) || INTEGRAL_OPERATORS.contains(&name.as_str())) {
                let mut k = j;
                while at(src, k) == ' ' {
                    k += 1;
                }
                let mut stack = LIMIT_OPERATORS.contains(&name.as_str());
                let mut resume = j;
                if starts_with(src, k, "\\limits") && !at(src, k + 7).is_ascii_alphabetic() {
                    stack = true;
                    resume = k + 7;
                    k = k + 7;
                } else if starts_with(src, k, "\\nolimits") && !at(src, k + 9).is_ascii_alphabetic() {
                    stack = false;
                    resume = k + 9;
                }
                if stack {
                    let mut sub_text: Option<String> = None;
                    let mut sup_text: Option<String> = None;
                    let mut m = k;
                    loop {
                        let mut n = m;
                        while at(src, n) == ' ' {
                            n += 1;
                        }
                        if at(src, n) == '_' && sub_text.is_none() {
                            let arg = read_arg(src, n + 1);
                            sub_text = Some(arg.text);
                            m = arg.end;
                            continue;
                        }
                        if at(src, n) == '^' && sup_text.is_none() {
                            let arg = read_arg(src, n + 1);
                            sup_text = Some(arg.text);
                            m = arg.end;
                            continue;
                        }
                        break;
                    }
                    if sub_text.is_some() || sup_text.is_some() {
                        flush_run(&mut boxes, &mut inline, &color, ctx);
                        let inner = inner_ctx(ctx, &color);
                        let glyph = text_box(&latex_to_unicode(&(ctx.wrap)(&format!("{color}\\{name}"))));
                        let sub_box = sub_text.map(|text| parse_expr_str(&text, &inner));
                        let sup_box = sup_text.map(|text| parse_expr_str(&text, &inner));
                        boxes.push(paint_box(limits_box(glyph, sub_box, sup_box), &color_scope));
                        i = m;
                        continue;
                    }
                }
                inline.push_str(&format!("\\{name}"));
                i = resume;
                continue;
            }
            if name == "color" || name == "normalcolor" {
                flush_run(&mut boxes, &mut inline, &color, ctx);
                if name == "normalcolor" {
                    color.clear();
                    color_scope = None;
                    i = j;
                    continue;
                }
                let mut k = j;
                while at(src, k) == ' ' {
                    k += 1;
                }
                let mut opt = String::new();
                if at(src, k) == '[' {
                    if let Some(close) = find_char(src, k, ']') {
                        opt = slice_chars(src, k, close + 1);
                        k = close + 1;
                        while at(src, k) == ' ' {
                            k += 1;
                        }
                    }
                }
                if at(src, k) == '{' {
                    let spec = read_brace_group(src, k);
                    color = format!("\\color{opt}{{{}}}", spec.text);
                    let model = opt
                        .strip_prefix('[')
                        .and_then(|s| s.strip_suffix(']'))
                        .map(str::trim);
                    color_scope = latex_color_scope(model, &spec.text);
                    i = spec.end;
                } else {
                    color.clear();
                    color_scope = None;
                    i = k;
                }
                continue;
            }
            if name == "begin" {
                let inner = inner_ctx(ctx, &color);
                if let Some((env_box, env_end)) = parse_environment(src, i, &inner) {
                    flush_run(&mut boxes, &mut inline, &color, ctx);
                    boxes.push(paint_box(env_box, &color_scope));
                    i = env_end;
                    continue;
                }
            }
            if !name.is_empty() && (MATH_FONT_COMMANDS.contains(&name.as_str()) || name == "textcolor") {
                let mut k = j;
                while at(src, k) == ' ' {
                    k += 1;
                }
                let mut prefix = format!("\\{name}");
                let mut scope: Option<ColorScope> = None;
                if name == "textcolor" {
                    let mut model: Option<String> = None;
                    if at(src, k) == '[' {
                        if let Some(close) = find_char(src, k, ']') {
                            model = Some(slice_chars(src, k + 1, close).trim().to_string());
                            prefix.push_str(&slice_chars(src, k, close + 1));
                            k = close + 1;
                            while at(src, k) == ' ' {
                                k += 1;
                            }
                        }
                    }
                    if at(src, k) != '{' {
                        inline.push_str(&format!("\\{name}"));
                        i = j;
                        continue;
                    }
                    let spec = read_brace_group(src, k);
                    prefix.push_str(&format!("{{{}}}", spec.text));
                    scope = latex_color_scope(model.as_deref(), &spec.text);
                    k = spec.end;
                    while at(src, k) == ' ' {
                        k += 1;
                    }
                }
                if at(src, k) == '{' {
                    let content = read_brace_group(src, k);
                    flush_run(&mut boxes, &mut inline, &color, ctx);
                    let pre = color.clone();
                    let base = ctx.clone();
                    let content_ctx = Ctx {
                        wrap: Rc::new(move |run: &str| (base.wrap)(&format!("{pre}{prefix}{{{run}}}"))),
                    };
                    let mut box_ = parse_expr_str(&content.text, &content_ctx);
                    if let Some(scope) = &scope {
                        box_ = colorize_box(box_, scope);
                    }
                    boxes.push(paint_box(box_, &color_scope));
                    i = content.end;
                    continue;
                }
            }
            if name.is_empty() {
                if j < src.len() {
                    inline.push('\\');
                    inline.push(src[j]);
                }
                i = j + 1;
                continue;
            }
            inline.push_str(&format!("\\{name}"));
            i = j;
            while matches!(at(src, i), '[' | '{') {
                if src[i] == '{' {
                    let group = read_brace_group(src, i);
                    inline.push('{');
                    inline.push_str(&group.text);
                    inline.push('}');
                    i = group.end;
                } else {
                    match find_char(src, i, ']') {
                        Some(close) => {
                            let end = close + 1;
                            inline.push_str(&slice_chars(src, i, end));
                            i = end;
                        }
                        None => {
                            let end = src.len();
                            inline.push_str(&slice_chars(src, i, end));
                            i = end;
                        }
                    }
                }
            }
            continue;
        }
        if c == '^' || c == '_' {
            let first = read_script(src, i);
            let mut n = first.end;
            while at(src, n) == ' ' {
                n += 1;
            }
            let second = if at(src, n) == if c == '^' { '_' } else { '^' } {
                Some(read_script(src, n))
            } else {
                None
            };
            let end = second.as_ref().map(|s| s.end).unwrap_or(first.end);
            let sup_text: Option<String> = if c == '^' {
                Some(first.text.clone())
            } else {
                second.as_ref().map(|s| s.text.clone())
            };
            let sub_text: Option<String> = if c == '_' {
                Some(first.text.clone())
            } else {
                second.as_ref().map(|s| s.text.clone())
            };
            let inner = inner_ctx(ctx, &color);
            let sup_box = sup_text
                .as_deref()
                .map(|raw| parse_expr_str(&script_arg_of(raw), &inner));
            let sub_box = sub_text
                .as_deref()
                .map(|raw| parse_expr_str(&script_arg_of(raw), &inner));
            let tall = sup_box.as_ref().map(|b| b.lines.len() > 1).unwrap_or(false)
                || sub_box.as_ref().map(|b| b.lines.len() > 1).unwrap_or(false);
            if tall
                || unconvertible(sup_text.as_deref())
                || unconvertible(sub_text.as_deref())
                || ragged(sup_text.as_deref())
                || ragged(sub_text.as_deref())
            {
                flush_run(&mut boxes, &mut inline, &color, ctx);
                let base = boxes.pop().unwrap_or_else(|| text_box(""));
                boxes.push(paint_box(attach_scripts(base, sub_box, sup_box), &color_scope));
                i = end;
                continue;
            }
            if inline.is_empty() {
                if let Some(last) = boxes.last() {
                    if last.lines.len() > 1 {
                        let corner = |raw: Option<&str>| -> Option<MathBox> {
                            raw.map(|raw| text_box(&latex_to_unicode(&(ctx.wrap)(&format!("{color}{raw}")))))
                        };
                        let new_last = attach_scripts(
                            last.clone(),
                            corner(sub_text.as_deref()),
                            corner(sup_text.as_deref()),
                        );
                        let idx = boxes.len() - 1;
                        boxes[idx] = paint_box(new_last, &color_scope);
                        i = end;
                        continue;
                    }
                }
            }
            inline.push_str(&slice_chars(src, i, end));
            i = end;
            continue;
        }
        if c == '{' {
            let group = read_brace_group(src, i);
            flush_run(&mut boxes, &mut inline, &color, ctx);
            let inner = inner_ctx(ctx, &color);
            boxes.push(paint_box(parse_expr_str(&group.text, &inner), &color_scope));
            i = group.end;
            continue;
        }
        if c == '(' || c == '[' {
            let close_ch = if c == '(' { ')' } else { ']' };
            if let Some(close) = match_delim(src, i, c, close_ch) {
                let inner = inner_ctx(ctx, &color);
                let inner_box = parse_expr_str(&slice_chars(src, i + 1, close), &inner);
                if inner_box.lines.len() > 1 {
                    flush_run(&mut boxes, &mut inline, &color, ctx);
                    boxes.push(paint_box(delim_box(inner_box, &c.to_string(), &close_ch.to_string()), &color_scope));
                    i = close + 1;
                    continue;
                }
            }
        }
        inline.push(c);
        i += 1;
    }
    flush_run(&mut boxes, &mut inline, &color, ctx);
    if boxes.is_empty() {
        return text_box("");
    }
    hconcat(boxes)
}

/// Count the command arguments still owed at the end of `seg`.
fn braces_owed(seg: &[char]) -> usize {
    let mut pending: Vec<i64> = Vec::new();
    let consume_arg = |pending: &mut Vec<i64>| {
        let top = pending.len().wrapping_sub(1);
        if pending.is_empty() {
            return;
        }
        if pending[top] == 1 {
            pending.pop();
        } else {
            pending[top] -= 1;
        }
    };
    let mut i = 0usize;
    while i < seg.len() {
        let c = seg[i];
        if c == '\\' {
            let mut j = i + 1;
            let mut name = String::new();
            while j < seg.len() && seg[j].is_ascii_alphabetic() {
                name.push(seg[j]);
                j += 1;
            }
            consume_arg(&mut pending);
            let arity = if name.is_empty() {
                0
            } else {
                command_arity(&name).unwrap_or(0)
            };
            let mut attached = 0usize;
            if !name.is_empty() {
                while matches!(at(seg, j), '[' | '{') {
                    if seg[j] == '{' {
                        j = read_brace_group(seg, j).end;
                        if attached < arity {
                            attached += 1;
                        }
                    } else {
                        match find_char(seg, j, ']') {
                            Some(close) => j = close + 1,
                            None => j = seg.len(),
                        }
                    }
                }
            } else {
                j = i + 2;
            }
            let missing = arity as i64 - attached as i64;
            if missing > 0 {
                pending.push(missing);
            }
            i = j;
            continue;
        }
        if c == '{' {
            i = read_brace_group(seg, i).end;
            consume_arg(&mut pending);
            continue;
        }
        if c == '^' || c == '_' {
            pending.push(1);
            i += 1;
            continue;
        }
        if c == ' ' || c == '\t' || c == '\n' {
            i += 1;
            continue;
        }
        consume_arg(&mut pending);
        i += 1;
    }
    pending.iter().sum::<i64>().max(0) as usize
}

fn collapse_newlines(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == ' ' || chars[i] == '\t' {
            let mut j = i;
            while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                j += 1;
            }
            if j < chars.len() && chars[j] == '\n' {
                j += 1;
                while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                    j += 1;
                }
                out.push(' ');
                i = j;
                continue;
            }
            out.push(chars[i]);
            i += 1;
            continue;
        }
        if chars[i] == '\n' {
            let mut j = i + 1;
            while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                j += 1;
            }
            out.push(' ');
            i = j;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Split on top-level `\n` and `\\` row separators (outside braces and environments).
fn split_lines(src: &[char]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut brace_depth: i64 = 0;
    let mut env_depth: i64 = 0;
    let mut last = 0usize;
    let mut i = 0usize;
    while i < src.len() {
        if starts_with(src, i, "\\begin") {
            env_depth += 1;
            i += 6;
            continue;
        }
        if starts_with(src, i, "\\end") {
            env_depth -= 1;
            i += 4;
            continue;
        }
        let c = src[i];
        if c == '\\' {
            if at(src, i + 1) == '\\' && brace_depth == 0 && env_depth == 0 {
                lines.push(slice_chars(src, last, i));
                i += 2;
                while at(src, i) == ' ' {
                    i += 1;
                }
                if at(src, i) == '[' {
                    match find_char(src, i, ']') {
                        Some(close) => i = close + 1,
                        None => i = src.len(),
                    }
                }
                last = i;
                continue;
            }
            i += 2;
            continue;
        }
        if c == '{' {
            brace_depth += 1;
        } else if c == '}' {
            brace_depth -= 1;
        } else if c == '\n' && brace_depth == 0 && env_depth == 0 {
            if braces_owed(&src[last..i]) == 0 {
                lines.push(slice_chars(src, last, i));
                last = i + 1;
            }
        }
        i += 1;
    }
    lines.push(slice_chars(src, last, src.len()));
    lines
}

/// The native form of `latex_to_block`: the TeX source as a `math` node.
pub fn describe_latex(src: &str, display: bool) -> NativeNode {
    let mut props = Props::new();
    props.set_text(src);
    props.set_bool("display", display);
    node(NodeKind::Math, props, Vec::new(), None)
}

/// Render a display LaTeX math fragment to lines with full 2-D layout.
pub fn latex_to_block(src: &str) -> Vec<String> {
    if src.trim().is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = src.trim().chars().collect();
    let rows: Vec<MathBox> = split_lines(&chars)
        .iter()
        .map(|line| collapse_newlines(line.trim()).trim().to_string())
        .filter(|line| !line.is_empty())
        .map(|line| parse_expr_str(&line, &root_ctx()))
        .collect();
    if rows.is_empty() {
        return Vec::new();
    }
    let mut lines = vconcat(&rows, CellAlign::L).lines;
    while lines.len() > 1 && lines.last().map(|line| line.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }
    while lines.len() > 1 && lines[0].trim().is_empty() {
        lines.remove(0);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fraction_stacks_with_bar() {
        assert_eq!(latex_to_block(r"\frac{a}{b}"), vec![" a ", "───", " b "]);
    }

    #[test]
    fn matrix_stretches_parentheses() {
        assert_eq!(
            latex_to_block(r"\begin{pmatrix}1&2\\3&4\end{pmatrix}"),
            vec!["⎛ 1  2 ⎞", "⎜      ⎟", "⎝ 3  4 ⎠"]
        );
    }

    #[test]
    fn cases_brace() {
        assert_eq!(latex_to_block(r"\begin{cases}x & y\end{cases}"), vec!["{x  y"]);
    }

    #[test]
    fn sqrt_draws_roof() {
        assert_eq!(latex_to_block(r"\sqrt{x}"), vec![" ┌──", "╲│ x"]);
    }

    #[test]
    fn block_script_raises_fraction() {
        assert_eq!(
            latex_to_block(r"x^{\frac{1}{2}}"),
            vec!["  1 ", " ───", "  2 ", "x   "]
        );
    }

    #[test]
    fn blank_source_is_empty() {
        assert!(latex_to_block("").is_empty());
        assert!(latex_to_block("   \n  ").is_empty());
    }

    #[test]
    fn split_lines_keeps_pending_argument() {
        assert_eq!(split_lines(&"a\n{b+c}".chars().collect::<Vec<_>>()), vec!["a", "{b+c}"]);
        assert_eq!(
            split_lines(&"\\frac{num}\n{den}".chars().collect::<Vec<_>>()),
            vec!["\\frac{num}\n{den}"]
        );
        assert_eq!(braces_owed(&"x^".chars().collect::<Vec<_>>()), 1);
        assert_eq!(braces_owed(&"x^2".chars().collect::<Vec<_>>()), 0);
    }

    #[test]
    fn describe_latex_sets_math_props() {
        let node = describe_latex("x^2", false);
        assert_eq!(node.kind, NodeKind::Math);
        assert_eq!(node.props.text(), Some("x^2"));
        assert_eq!(node.props.get_bool("display"), Some(false));
        assert!(node.children.is_empty());
    }
}
