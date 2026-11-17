//! LaTeX math → Unicode/ANSI conversion.
//! Reference: `l/src/latex-to-unicode.ts`.
//!
//! Terminals cannot lay out real math, but a surprising amount of LaTeX maps
//! cleanly onto Unicode: superscripts/subscripts (`x² xᵢ`), Greek (`α β`), big
//! operators (`∫ ∑ ∏`), relations/arrows (`≤ ≠ → ⇒`), fonts via the Mathematical
//! Alphanumeric Symbols block (`ℝ 𝐱 𝔄 𝒞`), accents via combining marks
//! (`x̂ x̄ x⃗`), fractions (`½`, `(a+b)/c`), radicals (`√`, `∛`), and ANSI
//! foreground/background colors (`\textcolor`, `\color`, `\colorbox`,
//! `\fcolorbox`).
//!
//! [`latex_to_unicode`] converts a *bare* math fragment (no `$`/`\(`
//! delimiters). [`render_math_in_text`] scans prose for `$$…$$`, `$…$`, `\(…\)`,
//! `\[…\]` spans and converts only those, with anti-currency heuristics so
//! "$5 and $10" is left untouched.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

// ---------------------------------------------------------------------------
// Character maps
// ---------------------------------------------------------------------------

/// Unicode superscript forms. Letters are incomplete in Unicode (`q`, and
/// several capitals have no superscript), so the converter falls back to
/// `^(…)` when any character in a script group is unmappable.
static SUPERSCRIPT: LazyLock<HashMap<char, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ('0', "⁰"),
        ('1', "¹"),
        ('2', "²"),
        ('3', "³"),
        ('4', "⁴"),
        ('5', "⁵"),
        ('6', "⁶"),
        ('7', "⁷"),
        ('8', "⁸"),
        ('9', "⁹"),
        ('+', "⁺"),
        ('-', "⁻"),
        ('−', "⁻"),
        ('=', "⁼"),
        ('(', "⁽"),
        (')', "⁾"),
        ('.', "·"),
        (' ', " "),
        ('a', "ᵃ"),
        ('b', "ᵇ"),
        ('c', "ᶜ"),
        ('d', "ᵈ"),
        ('e', "ᵉ"),
        ('f', "ᶠ"),
        ('g', "ᵍ"),
        ('h', "ʰ"),
        ('i', "ⁱ"),
        ('j', "ʲ"),
        ('k', "ᵏ"),
        ('l', "ˡ"),
        ('m', "ᵐ"),
        ('n', "ⁿ"),
        ('o', "ᵒ"),
        ('p', "ᵖ"),
        ('r', "ʳ"),
        ('s', "ˢ"),
        ('t', "ᵗ"),
        ('u', "ᵘ"),
        ('v', "ᵛ"),
        ('w', "ʷ"),
        ('x', "ˣ"),
        ('y', "ʸ"),
        ('z', "ᶻ"),
        ('A', "ᴬ"),
        ('B', "ᴮ"),
        ('D', "ᴰ"),
        ('E', "ᴱ"),
        ('G', "ᴳ"),
        ('H', "ᴴ"),
        ('I', "ᴵ"),
        ('J', "ᴶ"),
        ('K', "ᴷ"),
        ('L', "ᴸ"),
        ('M', "ᴹ"),
        ('N', "ᴺ"),
        ('O', "ᴼ"),
        ('P', "ᴾ"),
        ('R', "ᴿ"),
        ('T', "ᵀ"),
        ('U', "ᵁ"),
        ('V', "ⱽ"),
        ('W', "ᵂ"),
        ('α', "ᵅ"),
        ('β', "ᵝ"),
        ('γ', "ᵞ"),
        ('δ', "ᵟ"),
        ('ε', "ᵋ"),
        ('θ', "ᶿ"),
        ('ι', "ᶥ"),
        ('φ', "ᵠ"),
        ('χ', "ᵡ"),
    ])
});

/// Unicode subscript forms (even sparser than superscripts).
static SUBSCRIPT: LazyLock<HashMap<char, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ('0', "₀"),
        ('1', "₁"),
        ('2', "₂"),
        ('3', "₃"),
        ('4', "₄"),
        ('5', "₅"),
        ('6', "₆"),
        ('7', "₇"),
        ('8', "₈"),
        ('9', "₉"),
        ('+', "₊"),
        ('-', "₋"),
        ('−', "₋"),
        ('=', "₌"),
        ('(', "₍"),
        (')', "₎"),
        (' ', " "),
        ('a', "ₐ"),
        ('e', "ₑ"),
        ('h', "ₕ"),
        ('i', "ᵢ"),
        ('j', "ⱼ"),
        ('k', "ₖ"),
        ('l', "ₗ"),
        ('m', "ₘ"),
        ('n', "ₙ"),
        ('o', "ₒ"),
        ('p', "ₚ"),
        ('r', "ᵣ"),
        ('s', "ₛ"),
        ('t', "ₜ"),
        ('u', "ᵤ"),
        ('v', "ᵥ"),
        ('x', "ₓ"),
        ('β', "ᵦ"),
        ('γ', "ᵧ"),
        ('ρ', "ᵨ"),
        ('φ', "ᵩ"),
        ('χ', "ᵪ"),
    ])
});

/// Prime runs: `f' f'' f''' f''''`.
const PRIMES: [&str; 5] = ["", "′", "″", "‴", "⁗"];

/// Common vulgar fractions, keyed by `${num}/${den}` of the rendered parts.
static VULGAR: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("1/2", "½"),
        ("1/3", "⅓"),
        ("2/3", "⅔"),
        ("1/4", "¼"),
        ("3/4", "¾"),
        ("1/5", "⅕"),
        ("2/5", "⅖"),
        ("3/5", "⅗"),
        ("4/5", "⅘"),
        ("1/6", "⅙"),
        ("5/6", "⅚"),
        ("1/7", "⅐"),
        ("1/8", "⅛"),
        ("3/8", "⅜"),
        ("5/8", "⅝"),
        ("7/8", "⅞"),
        ("1/9", "⅑"),
        ("1/10", "⅒"),
        ("0/3", "↉"),
    ])
});

/// `\not<rel>` negations that have a dedicated Unicode glyph (cleaner than the
/// combining-solidus fallback).
static NOT_MAP: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("=", "≠"),
        ("<", "≮"),
        (">", "≯"),
        ("∈", "∉"),
        ("∋", "∌"),
        ("⊂", "⊄"),
        ("⊃", "⊅"),
        ("⊆", "⊈"),
        ("⊇", "⊉"),
        ("≡", "≢"),
        ("∃", "∄"),
        ("≤", "≰"),
        ("≥", "≱"),
        ("≈", "≉"),
        ("≅", "≇"),
        ("∼", "≁"),
        ("≃", "≄"),
        ("∣", "∤"),
        ("∥", "∦"),
        ("≺", "⊀"),
        ("≻", "⊁"),
        ("⊑", "⋢"),
        ("⊒", "⋣"),
    ])
});

/// Combining diacritics for accent commands (applied after each base glyph).
static ACCENTS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("hat", "\u{0302}"),
        ("widehat", "\u{0302}"),
        ("check", "\u{030C}"),
        ("widecheck", "\u{030C}"),
        ("tilde", "\u{0303}"),
        ("widetilde", "\u{0303}"),
        ("acute", "\u{0301}"),
        ("grave", "\u{0300}"),
        ("dot", "\u{0307}"),
        ("ddot", "\u{0308}"),
        ("dddot", "\u{20DB}"),
        ("ddddot", "\u{20DC}"),
        ("breve", "\u{0306}"),
        ("bar", "\u{0304}"),
        ("vec", "\u{20D7}"),
        ("overrightarrow", "\u{20D7}"),
        ("overleftarrow", "\u{20D6}"),
        ("mathring", "\u{030A}"),
        ("overline", "\u{0305}"),
        ("underline", "\u{0332}"),
        ("underbar", "\u{0332}"),
    ])
});

/// Math functions rendered as their literal upright name (`sin`, `cos`, `lim`, …).
static FUNCTIONS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "sin",
        "cos",
        "tan",
        "cot",
        "sec",
        "csc",
        "sinh",
        "cosh",
        "tanh",
        "coth",
        "arcsin",
        "arccos",
        "arctan",
        "arccot",
        "arcsec",
        "arccsc",
        "sech",
        "csch",
        "ln",
        "log",
        "lg",
        "exp",
        "lim",
        "limsup",
        "liminf",
        "max",
        "min",
        "sup",
        "inf",
        "det",
        "dim",
        "ker",
        "hom",
        "arg",
        "deg",
        "gcd",
        "lcm",
        "Pr",
        "argmax",
        "argmin",
        "sgn",
        "tr",
        "rank",
        "diag",
        "var",
        "cov",
        "median",
        "mod",
    ])
});

/// Math-mode font commands → Mathematical Alphanumeric Symbols style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FontStyle {
    Bold,
    Italic,
    BoldItalic,
    Script,
    BoldScript,
    Fraktur,
    DoubleStruck,
    BoldFraktur,
    Sans,
    SansBold,
    SansItalic,
    SansBoldItalic,
    Mono,
}

impl FontStyle {
    /// Lowercase style name used by the Letterlike-Symbols hole table.
    fn name(self) -> &'static str {
        match self {
            FontStyle::Bold => "bold",
            FontStyle::Italic => "italic",
            FontStyle::BoldItalic => "bolditalic",
            FontStyle::Script => "script",
            FontStyle::BoldScript => "boldscript",
            FontStyle::Fraktur => "fraktur",
            FontStyle::DoubleStruck => "doublestruck",
            FontStyle::BoldFraktur => "boldfraktur",
            FontStyle::Sans => "sans",
            FontStyle::SansBold => "sansbold",
            FontStyle::SansItalic => "sansitalic",
            FontStyle::SansBoldItalic => "sansbolditalic",
            FontStyle::Mono => "mono",
        }
    }
}

static FONTS: LazyLock<HashMap<&'static str, FontStyle>> = LazyLock::new(|| {
    HashMap::from([
        ("mathbf", FontStyle::Bold),
        ("boldsymbol", FontStyle::BoldItalic),
        ("bm", FontStyle::BoldItalic),
        ("pmb", FontStyle::Bold),
        ("mathbb", FontStyle::DoubleStruck),
        ("Bbb", FontStyle::DoubleStruck),
        ("mathds", FontStyle::DoubleStruck),
        ("mathbbm", FontStyle::DoubleStruck),
        ("mathcal", FontStyle::Script),
        ("mathscr", FontStyle::BoldScript),
        ("mathfrak", FontStyle::Fraktur),
        ("mathbfscr", FontStyle::BoldScript),
        ("mathbfcal", FontStyle::BoldScript),
        ("mathbffrak", FontStyle::BoldFraktur),
        ("mathfrakbold", FontStyle::BoldFraktur),
        ("mathsf", FontStyle::Sans),
        ("mathsfit", FontStyle::SansItalic),
        ("mathsfbf", FontStyle::SansBold),
        ("mathbfsf", FontStyle::SansBold),
        ("mathsfbfit", FontStyle::SansBoldItalic),
        ("mathbfsfit", FontStyle::SansBoldItalic),
        ("mathtt", FontStyle::Mono),
        ("mathit", FontStyle::Italic),
        ("mathbfit", FontStyle::BoldItalic),
    ])
});

/// A text-mode style override: `None` leaves the enclosing style untouched.
#[derive(Clone, Copy, Debug, Default)]
struct TextStyleCmd {
    bold: Option<bool>,
    italic: Option<bool>,
}

static TEXT_STYLE_COMMANDS: LazyLock<HashMap<&'static str, TextStyleCmd>> = LazyLock::new(|| {
    HashMap::from([
        ("textbf", TextStyleCmd { bold: Some(true), italic: None }),
        ("textit", TextStyleCmd { bold: None, italic: Some(true) }),
        ("textsl", TextStyleCmd { bold: None, italic: Some(true) }),
        ("emph", TextStyleCmd { bold: None, italic: Some(true) }),
        ("textmd", TextStyleCmd { bold: Some(false), italic: None }),
        ("textup", TextStyleCmd { bold: None, italic: Some(false) }),
        ("texttt", TextStyleCmd { bold: None, italic: None }),
        ("textsf", TextStyleCmd { bold: None, italic: None }),
    ])
});

/// Math font and text-style command names whose single brace argument is
/// rendered with the corresponding style. The display block engine re-wraps
/// inline runs inside these commands when their argument contains 2-D layout so
/// styling survives box boundaries.
pub const MATH_FONT_COMMANDS: &[&str] = &[
    "mathbf",
    "boldsymbol",
    "bm",
    "pmb",
    "mathbb",
    "Bbb",
    "mathds",
    "mathbbm",
    "mathcal",
    "mathscr",
    "mathfrak",
    "mathbfscr",
    "mathbfcal",
    "mathbffrak",
    "mathfrakbold",
    "mathsf",
    "mathsfit",
    "mathsfbf",
    "mathbfsf",
    "mathsfbfit",
    "mathbfsfit",
    "mathtt",
    "mathit",
    "mathbfit",
    "textbf",
    "textit",
    "textsl",
    "emph",
    "textmd",
    "textup",
    "texttt",
    "textsf",
];

/// True when `name` is a math font or text-style command.
pub fn is_math_font_command(name: &str) -> bool {
    MATH_FONT_COMMANDS.contains(&name)
}

/// Text-mode commands whose argument is passed through literally (no math).
static TEXT_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "text",
        "textrm",
        "textsc",
        "mathrm",
        "mathnormal",
        "mbox",
        "hbox",
    ])
});

/// Base code points for each style's A, a, and (where it exists) 0 in the
/// Mathematical Alphanumeric Symbols block (U+1D400–U+1D7FF).
fn plane(style: FontStyle) -> (u32, u32, Option<u32>) {
    match style {
        FontStyle::Bold => (0x1d400, 0x1d41a, Some(0x1d7ce)),
        FontStyle::Italic => (0x1d434, 0x1d44e, None),
        FontStyle::BoldItalic => (0x1d468, 0x1d482, None),
        FontStyle::Script => (0x1d49c, 0x1d4b6, None),
        FontStyle::BoldScript => (0x1d4d0, 0x1d4ea, None),
        FontStyle::Fraktur => (0x1d504, 0x1d51e, None),
        FontStyle::DoubleStruck => (0x1d538, 0x1d552, Some(0x1d7d8)),
        FontStyle::BoldFraktur => (0x1d56c, 0x1d586, None),
        FontStyle::Sans => (0x1d5a0, 0x1d5ba, Some(0x1d7e2)),
        FontStyle::SansBold => (0x1d5d4, 0x1d5ee, Some(0x1d7ec)),
        FontStyle::SansItalic => (0x1d608, 0x1d622, None),
        FontStyle::SansBoldItalic => (0x1d63c, 0x1d656, None),
        FontStyle::Mono => (0x1d670, 0x1d68a, Some(0x1d7f6)),
    }
}

/// Reserved code points in the math alphabets that Unicode places in the
/// Letterlike Symbols block instead (the famous "holes").
static ALPHA_HOLES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("italic:h", "ℎ"),
        ("script:B", "ℬ"),
        ("script:E", "ℰ"),
        ("script:F", "ℱ"),
        ("script:H", "ℋ"),
        ("script:I", "ℐ"),
        ("script:L", "ℒ"),
        ("script:M", "ℳ"),
        ("script:R", "ℛ"),
        ("script:e", "ℯ"),
        ("script:g", "ℊ"),
        ("script:o", "ℴ"),
        ("fraktur:C", "ℭ"),
        ("fraktur:H", "ℌ"),
        ("fraktur:I", "ℑ"),
        ("fraktur:R", "ℜ"),
        ("fraktur:Z", "ℨ"),
        ("doublestruck:C", "ℂ"),
        ("doublestruck:H", "ℍ"),
        ("doublestruck:N", "ℕ"),
        ("doublestruck:P", "ℙ"),
        ("doublestruck:Q", "ℚ"),
        ("doublestruck:R", "ℝ"),
        ("doublestruck:Z", "ℤ"),
    ])
});
/// Matrix/cases environment delimiters: `[open, close]`.
static ENV_DELIMS: LazyLock<HashMap<&'static str, (&'static str, &'static str)>> =
    LazyLock::new(|| {
        HashMap::from([
            ("matrix", ("", "")),
            ("smallmatrix", ("", "")),
            ("array", ("", "")),
            ("tabular", ("", "")),
            ("pmatrix", ("(", ")")),
            ("bmatrix", ("[", "]")),
            ("Bmatrix", ("{", "}")),
            ("vmatrix", ("|", "|")),
            ("Vmatrix", ("‖", "‖")),
            ("cases", ("{", "")),
            ("cases*", ("{", "")),
            ("dcases", ("{", "")),
            ("dcases*", ("{", "")),
            ("rcases", ("", "}")),
            ("drcases", ("", "}")),
            ("aligned", ("", "")),
            ("aligned*", ("", "")),
            ("alignedat", ("", "")),
            ("alignedat*", ("", "")),
            ("align", ("", "")),
            ("align*", ("", "")),
            ("alignat", ("", "")),
            ("alignat*", ("", "")),
            ("split", ("", "")),
            ("gathered", ("", "")),
            ("equation", ("", "")),
            ("equation*", ("", "")),
        ])
    });

/// Greek, operators, relations, arrows, delimiters, and assorted symbols.
static SYMBOLS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        // Greek lowercase
        ("alpha", "α"),
        ("beta", "β"),
        ("gamma", "γ"),
        ("delta", "δ"),
        ("epsilon", "ϵ"),
        ("varepsilon", "ε"),
        ("zeta", "ζ"),
        ("eta", "η"),
        ("theta", "θ"),
        ("vartheta", "ϑ"),
        ("iota", "ι"),
        ("kappa", "κ"),
        ("varkappa", "ϰ"),
        ("lambda", "λ"),
        ("mu", "μ"),
        ("nu", "ν"),
        ("xi", "ξ"),
        ("omicron", "ο"),
        ("pi", "π"),
        ("varpi", "ϖ"),
        ("rho", "ρ"),
        ("varrho", "ϱ"),
        ("sigma", "σ"),
        ("varsigma", "ς"),
        ("tau", "τ"),
        ("upsilon", "υ"),
        ("phi", "ϕ"),
        ("varphi", "φ"),
        ("chi", "χ"),
        ("psi", "ψ"),
        ("omega", "ω"),
        ("digamma", "ϝ"),
        // Greek uppercase
        ("Gamma", "Γ"),
        ("Delta", "Δ"),
        ("Theta", "Θ"),
        ("Lambda", "Λ"),
        ("Xi", "Ξ"),
        ("Pi", "Π"),
        ("Sigma", "Σ"),
        ("Upsilon", "Υ"),
        ("Phi", "Φ"),
        ("Psi", "Ψ"),
        ("Omega", "Ω"),
        // Big operators
        ("sum", "∑"),
        ("prod", "∏"),
        ("coprod", "∐"),
        ("int", "∫"),
        ("iint", "∬"),
        ("iiint", "∭"),
        ("iiiint", "⨌"),
        ("oint", "∮"),
        ("oiint", "∯"),
        ("oiiint", "∰"),
        ("bigcap", "⋂"),
        ("bigcup", "⋃"),
        ("bigsqcup", "⨆"),
        ("bigvee", "⋁"),
        ("bigwedge", "⋀"),
        ("bigodot", "⨀"),
        ("bigoplus", "⨁"),
        ("bigotimes", "⨂"),
        ("biguplus", "⨄"),
        ("Cap", "⋒"),
        ("Cup", "⋓"),
        ("bigstar", "★"),
        // Binary operators
        ("pm", "±"),
        ("mp", "∓"),
        ("times", "×"),
        ("div", "÷"),
        ("ast", "∗"),
        ("star", "⋆"),
        ("circ", "∘"),
        ("bullet", "∙"),
        ("cdot", "⋅"),
        ("cdotp", "·"),
        ("centerdot", "·"),
        ("cap", "∩"),
        ("cup", "∪"),
        ("uplus", "⊎"),
        ("sqcap", "⊓"),
        ("sqcup", "⊔"),
        ("vee", "∨"),
        ("wedge", "∧"),
        ("land", "∧"),
        ("lor", "∨"),
        ("setminus", "∖"),
        ("smallsetminus", "∖"),
        ("wr", "≀"),
        ("amalg", "⨿"),
        ("diamond", "⋄"),
        ("Diamond", "◇"),
        ("bigtriangleup", "△"),
        ("bigtriangledown", "▽"),
        ("triangleleft", "◁"),
        ("triangleright", "▷"),
        ("lhd", "⊲"),
        ("rhd", "⊳"),
        ("unlhd", "⊴"),
        ("unrhd", "⊵"),
        ("oplus", "⊕"),
        ("ominus", "⊖"),
        ("otimes", "⊗"),
        ("oslash", "⊘"),
        ("odot", "⊙"),
        ("dagger", "†"),
        ("ddagger", "‡"),
        ("boxplus", "⊞"),
        ("boxtimes", "⊠"),
        ("boxdot", "⊡"),
        ("boxminus", "⊟"),
        ("ltimes", "⋉"),
        ("rtimes", "⋊"),
        ("leftthreetimes", "⋋"),
        ("rightthreetimes", "⋌"),
        ("curlyvee", "⋎"),
        ("curlywedge", "⋏"),
        ("barwedge", "⊼"),
        ("veebar", "⊻"),
        ("doublebarwedge", "⩞"),
        ("circledast", "⊛"),
        ("circledcirc", "⊚"),
        ("circleddash", "⊝"),
        ("divideontimes", "⋇"),
        ("dotplus", "∔"),
        // Relations
        ("leq", "≤"),
        ("le", "≤"),
        ("geq", "≥"),
        ("ge", "≥"),
        ("ll", "≪"),
        ("gg", "≫"),
        ("neq", "≠"),
        ("ne", "≠"),
        ("equiv", "≡"),
        ("doteq", "≐"),
        ("sim", "∼"),
        ("simeq", "≃"),
        ("approx", "≈"),
        ("approxeq", "≊"),
        ("cong", "≅"),
        ("propto", "∝"),
        ("asymp", "≍"),
        ("prec", "≺"),
        ("succ", "≻"),
        ("preceq", "⪯"),
        ("succeq", "⪰"),
        ("subset", "⊂"),
        ("supset", "⊃"),
        ("subseteq", "⊆"),
        ("supseteq", "⊇"),
        ("subsetneq", "⊊"),
        ("supsetneq", "⊋"),
        ("sqsubset", "⊏"),
        ("sqsupset", "⊐"),
        ("sqsubseteq", "⊑"),
        ("sqsupseteq", "⊒"),
        ("in", "∈"),
        ("ni", "∋"),
        ("owns", "∋"),
        ("notin", "∉"),
        ("mid", "∣"),
        ("nmid", "∤"),
        ("parallel", "∥"),
        ("nparallel", "∦"),
        ("perp", "⊥"),
        ("vdash", "⊢"),
        ("dashv", "⊣"),
        ("models", "⊨"),
        ("vDash", "⊨"),
        ("Vdash", "⊩"),
        ("bowtie", "⋈"),
        ("smile", "⌣"),
        ("frown", "⌢"),
        ("between", "≬"),
        ("lessgtr", "≶"),
        ("gtrapprox", "⪆"),
        ("leqq", "≦"),
        ("geqq", "≧"),
        ("lneq", "⪇"),
        ("gneq", "⪈"),
        ("lneqq", "≨"),
        ("gneqq", "≩"),
        ("nleq", "≰"),
        ("ngeq", "≱"),
        ("nless", "≮"),
        ("ngtr", "≯"),
        ("nsubseteq", "⊈"),
        ("nsupseteq", "⊉"),
        ("nsim", "≁"),
        ("ncong", "≇"),
        ("triangleq", "≜"),
        ("coloneqq", "≔"),
        ("eqqcolon", "≕"),
        ("risingdotseq", "≓"),
        ("fallingdotseq", "≒"),
        ("circeq", "≗"),
        ("eqcirc", "≖"),
        ("precsim", "≾"),
        ("succsim", "≿"),
        ("precapprox", "⪷"),
        ("succapprox", "⪸"),
        ("curlyeqprec", "⋞"),
        ("curlyeqsucc", "⋟"),
        ("Subset", "⋐"),
        ("Supset", "⋑"),
        ("subseteqq", "⫅"),
        ("supseteqq", "⫆"),
        ("subsetneqq", "⫋"),
        ("supsetneqq", "⫌"),
        ("Vvdash", "⊪"),
        ("shortmid", "∣"),
        ("shortparallel", "∥"),
        ("pitchfork", "⋔"),
        // Arrows
        ("leftarrow", "←"),
        ("gets", "←"),
        ("rightarrow", "→"),
        ("to", "→"),
        ("leftrightarrow", "↔"),
        ("Leftarrow", "⇐"),
        ("Rightarrow", "⇒"),
        ("Leftrightarrow", "⇔"),
        ("uparrow", "↑"),
        ("downarrow", "↓"),
        ("updownarrow", "↕"),
        ("Uparrow", "⇑"),
        ("Downarrow", "⇓"),
        ("Updownarrow", "⇕"),
        ("mapsto", "↦"),
        ("longmapsto", "⟼"),
        ("hookleftarrow", "↩"),
        ("hookrightarrow", "↪"),
        ("leftharpoonup", "↼"),
        ("rightharpoonup", "⇀"),
        ("leftharpoondown", "↽"),
        ("rightharpoondown", "⇁"),
        ("rightleftharpoons", "⇌"),
        ("longleftarrow", "⟵"),
        ("longrightarrow", "⟶"),
        ("longleftrightarrow", "⟷"),
        ("Longleftarrow", "⟸"),
        ("Longrightarrow", "⟹"),
        ("Longleftrightarrow", "⟺"),
        ("implies", "⟹"),
        ("impliedby", "⟸"),
        ("iff", "⟺"),
        ("nearrow", "↗"),
        ("searrow", "↘"),
        ("swarrow", "↙"),
        ("nwarrow", "↖"),
        ("nleftarrow", "↚"),
        ("nrightarrow", "↛"),
        ("leadsto", "⇝"),
        ("rightsquigarrow", "⇝"),
        ("leftrightsquigarrow", "↭"),
        ("twoheadrightarrow", "↠"),
        ("twoheadleftarrow", "↞"),
        ("leftrightharpoons", "⇋"),
        ("rightleftarrows", "⇄"),
        ("leftrightarrows", "⇆"),
        ("leftleftarrows", "⇇"),
        ("rightrightarrows", "⇉"),
        ("upuparrows", "⇈"),
        ("downdownarrows", "⇊"),
        ("circlearrowleft", "↺"),
        ("circlearrowright", "↻"),
        ("curvearrowleft", "↶"),
        ("curvearrowright", "↷"),
        ("dashleftarrow", "⇠"),
        ("dashrightarrow", "⇢"),
        ("Lleftarrow", "⇚"),
        ("Rrightarrow", "⇛"),
        ("leftarrowtail", "↢"),
        ("rightarrowtail", "↣"),
        ("looparrowleft", "↫"),
        ("looparrowright", "↬"),
        ("multimap", "⊸"),
        // Miscellaneous
        ("infty", "∞"),
        ("partial", "∂"),
        ("nabla", "∇"),
        ("forall", "∀"),
        ("exists", "∃"),
        ("nexists", "∄"),
        ("emptyset", "∅"),
        ("varnothing", "∅"),
        ("neg", "¬"),
        ("lnot", "¬"),
        ("top", "⊤"),
        ("bot", "⊥"),
        ("angle", "∠"),
        ("measuredangle", "∡"),
        ("sphericalangle", "∢"),
        ("aleph", "ℵ"),
        ("beth", "ℶ"),
        ("gimel", "ℷ"),
        ("daleth", "ℸ"),
        ("hbar", "ℏ"),
        ("hslash", "ℏ"),
        ("ell", "ℓ"),
        ("imath", "ı"),
        ("jmath", "ȷ"),
        ("wp", "℘"),
        ("Re", "ℜ"),
        ("Im", "ℑ"),
        ("mho", "℧"),
        ("complement", "∁"),
        ("surd", "√"),
        ("flat", "♭"),
        ("natural", "♮"),
        ("sharp", "♯"),
        ("clubsuit", "♣"),
        ("diamondsuit", "♦"),
        ("heartsuit", "♥"),
        ("spadesuit", "♠"),
        ("clubs", "♣"),
        ("diamonds", "♦"),
        ("hearts", "♥"),
        ("spades", "♠"),
        ("therefore", "∴"),
        ("because", "∵"),
        ("checkmark", "✓"),
        ("maltese", "✠"),
        ("dag", "†"),
        ("ddag", "‡"),
        ("S", "§"),
        ("P", "¶"),
        ("copyright", "©"),
        ("circledR", "®"),
        ("pounds", "£"),
        ("yen", "¥"),
        ("euro", "€"),
        ("degree", "°"),
        ("prime", "′"),
        ("backprime", "‵"),
        ("colon", ":"),
        ("semicolon", ";"),
        ("neper", "₪"),
        ("square", "□"),
        ("Box", "□"),
        ("blacksquare", "■"),
        ("lozenge", "◊"),
        ("blacklozenge", "⧫"),
        ("triangle", "△"),
        ("blacktriangle", "▴"),
        ("blacktriangledown", "▾"),
        ("blacktriangleleft", "◂"),
        ("blacktriangleright", "▸"),
        ("diagup", "╱"),
        ("diagdown", "╲"),
        ("backepsilon", "϶"),
        ("Game", "⅁"),
        ("eth", "ð"),
        // Dots & ellipses
        ("ldots", "…"),
        ("dots", "…"),
        ("cdots", "⋯"),
        ("vdots", "⋮"),
        ("ddots", "⋱"),
        ("hdots", "…"),
        ("mathellipsis", "…"),
        ("dotsc", "…"),
        ("dotsb", "⋯"),
        ("dotsm", "⋯"),
        ("dotsi", "⋯"),
        // Delimiters
        ("langle", "⟨"),
        ("rangle", "⟩"),
        ("lceil", "⌈"),
        ("rceil", "⌉"),
        ("lfloor", "⌊"),
        ("rfloor", "⌋"),
        ("lbrace", "{"),
        ("rbrace", "}"),
        ("lbrack", "["),
        ("rbrack", "]"),
        ("vert", "|"),
        ("Vert", "‖"),
        ("lvert", "|"),
        ("rvert", "|"),
        ("lVert", "‖"),
        ("rVert", "‖"),
        ("backslash", "\\"),
        ("slash", "/"),
        ("ulcorner", "⌜"),
        ("urcorner", "⌝"),
        ("llcorner", "⌞"),
        ("lrcorner", "⌟"),
        ("lmoustache", "⎰"),
        ("rmoustache", "⎱"),
        ("lgroup", "⟮"),
        ("rgroup", "⟯"),
        ("bracevert", "⎪"),
        // Blackboard / letterlike shortcuts commonly written bare
        ("Reals", "ℝ"),
        ("Complex", "ℂ"),
        ("Natural", "ℕ"),
        ("Integer", "ℤ"),
        ("Rational", "ℚ"),
    ])
});

// ---------------------------------------------------------------------------
// Render model
// ---------------------------------------------------------------------------

/// Style carried while parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ParseStyle {
    font: Option<FontStyle>,
    bold: bool,
    italic: bool,
}

/// Style materialized onto rendered chunks.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RenderStyle {
    bold: bool,
    italic: bool,
    foreground: Option<String>,
    background: Option<String>,
}

impl Default for RenderStyle {
    fn default() -> Self {
        RenderStyle { bold: false, italic: false, foreground: None, background: None }
    }
}

/// A terminal attribute a scope can open or close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StyleAttribute {
    Bold,
    Italic,
}

/// A deferred scope boundary attached to a chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderEvent {
    /// Open `attribute` at the start of the chunk.
    ScopeStart(StyleAttribute),
    /// Close `attribute` at the end of the chunk, optionally restoring it.
    ScopeEnd { attribute: StyleAttribute, restore: bool },
}

/// A run of text sharing one render style.
#[derive(Clone, Debug)]
struct Chunk {
    text: String,
    style: RenderStyle,
    before: Option<Vec<RenderEvent>>,
    after: Option<Vec<RenderEvent>>,
}

impl Chunk {
    /// A chunk with no deferred scope events.
    fn new(text: String, style: RenderStyle) -> Self {
        Chunk { text, style, before: None, after: None }
    }
}

/// A rendered run: ordered, coalesced chunks.
type Rendered = Vec<Chunk>;

const ROOT_STYLE: ParseStyle = ParseStyle { font: None, bold: false, italic: false };

const ANSI_FG_RESET: &str = "\x1b[39m";
const ANSI_BG_RESET: &str = "\x1b[49m";
const ANSI_BOLD_ON: &str = "\x1b[1m";
const ANSI_BOLD_OFF: &str = "\x1b[22m";
const ANSI_ITALIC_ON: &str = "\x1b[3m";
const ANSI_ITALIC_OFF: &str = "\x1b[23m";

fn render_style(style: ParseStyle, foreground: Option<String>, background: Option<String>) -> RenderStyle {
    RenderStyle { bold: style.bold, italic: style.italic, foreground, background }
}

fn same_render_style(a: &RenderStyle, b: &RenderStyle) -> bool {
    a == b
}

fn append_rendered(target: &mut Rendered, source: &[Chunk]) {
    for chunk in source {
        if chunk.text.is_empty() {
            continue;
        }
        let coalesce = match target.last() {
            Some(previous) => {
                previous.after.is_none()
                    && chunk.before.is_none()
                    && chunk.after.is_none()
                    && same_render_style(&previous.style, &chunk.style)
            }
            None => false,
        };
        if coalesce {
            if let Some(previous) = target.last_mut() {
                previous.text.push_str(&chunk.text);
            }
        } else {
            target.push(chunk.clone());
        }
    }
}

fn concat_rendered(parts: impl IntoIterator<Item = Rendered>) -> Rendered {
    let mut out: Rendered = Vec::new();
    for part in parts {
        append_rendered(&mut out, &part);
    }
    out
}

fn styled_text(text: &str, style: RenderStyle) -> Rendered {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Chunk::new(text.to_string(), style)]
    }
}

fn plain_text(rendered: &[Chunk]) -> String {
    let mut text = String::new();
    for chunk in rendered {
        text.push_str(&chunk.text);
    }
    text
}

/// Map every code point of `text` through `table`; `None` if any is unmappable.
fn map_all(text: &[Chunk], table: &HashMap<char, &'static str>) -> Option<Rendered> {
    let mut out: Rendered = Vec::new();
    for chunk in text {
        let mut mapped = String::new();
        for ch in chunk.text.chars() {
            let replacement = table.get(&ch)?;
            mapped.push_str(replacement);
        }
        let mut mapped_chunk = chunk.clone();
        mapped_chunk.text = mapped;
        append_rendered(&mut out, &[mapped_chunk]);
    }
    Some(out)
}

/// Number of Unicode code points (not UTF-16 units) in rendered glyph text.
fn code_point_length(text: &[Chunk]) -> usize {
    let mut length = 0;
    for chunk in text {
        length += chunk.text.chars().count();
    }
    length
}

/// Style a single ASCII letter/digit via the math alphanumeric block.
fn style_alnum(ch: char, style: FontStyle) -> String {
    let key = format!("{}:{}", style.name(), ch);
    if let Some(hole) = ALPHA_HOLES.get(key.as_str()) {
        return (*hole).to_string();
    }
    let (upper, lower, digit) = plane(style);
    let code = ch as u32;
    if (65..=90).contains(&code) {
        return char::from_u32(upper + (code - 65)).map_or_else(|| ch.to_string(), |c| c.to_string());
    }
    if (97..=122).contains(&code) {
        return char::from_u32(lower + (code - 97)).map_or_else(|| ch.to_string(), |c| c.to_string());
    }
    if (48..=57).contains(&code) {
        if let Some(digit) = digit {
            return char::from_u32(digit + (code - 48)).map_or_else(|| ch.to_string(), |c| c.to_string());
        }
    }
    ch.to_string()
}

/// Identity, or math-alphanumeric styling when a font style is active.
fn style_char(ch: char, style: ParseStyle) -> String {
    let font = match style.font {
        Some(font) => font,
        None => return ch.to_string(),
    };
    let code = ch as u32;
    let is_alnum = (65..=90).contains(&code) || (97..=122).contains(&code) || (48..=57).contains(&code);
    if is_alnum {
        style_alnum(ch, font)
    } else {
        ch.to_string()
    }
}

fn surround_rendered(text: Rendered, left: &str, right: &str, fallback: RenderStyle) -> Rendered {
    concat_rendered([styled_text(left, fallback.clone()), text, styled_text(right, fallback)])
}

fn trim_rendered(text: &[Chunk]) -> Rendered {
    let mut out: Rendered = text.to_vec();
    while let Some(first) = out.first_mut() {
        let trimmed = first.text.trim_start().to_string();
        if trimmed.is_empty() {
            out.remove(0);
            continue;
        }
        first.text = trimmed;
        break;
    }
    while let Some(last) = out.last_mut() {
        let trimmed = last.text.trim_end().to_string();
        if trimmed.is_empty() {
            out.pop();
            continue;
        }
        last.text = trimmed;
        break;
    }
    out
}

fn scope_rendered(text: &[Chunk], attribute: StyleAttribute, parent_active: bool) -> Rendered {
    if !parent_active || text.is_empty() {
        return text.to_vec();
    }
    let mut out = text.to_vec();
    if let Some(first) = out.first_mut() {
        if style_attribute(&first.style, attribute) {
            first.before.get_or_insert_with(Vec::new).push(RenderEvent::ScopeStart(attribute));
        }
    }
    if let Some(last) = out.last_mut() {
        if style_attribute(&last.style, attribute) {
            last.after.get_or_insert_with(Vec::new).push(RenderEvent::ScopeEnd { attribute, restore: true });
        }
    }
    out
}

/// Read a boolean attribute out of a render style.
fn style_attribute(style: &RenderStyle, attribute: StyleAttribute) -> bool {
    match attribute {
        StyleAttribute::Bold => style.bold,
        StyleAttribute::Italic => style.italic,
    }
}

/// Append a combining mark after each non-space base glyph.
fn apply_combining(text: &[Chunk], mark: &str) -> Rendered {
    let mut out: Rendered = Vec::new();
    for chunk in text {
        let mut mapped = String::new();
        for ch in chunk.text.chars() {
            mapped.push(ch);
            if ch != ' ' {
                mapped.push_str(mark);
            }
        }
        let mut mapped_chunk = chunk.clone();
        mapped_chunk.text = mapped;
        append_rendered(&mut out, &[mapped_chunk]);
    }
    out
}

fn emit_style_transition(out: &mut Vec<String>, from: &RenderStyle, to: &RenderStyle) {
    if from.bold && !to.bold {
        out.push(ANSI_BOLD_OFF.to_string());
    }
    if from.italic && !to.italic {
        out.push(ANSI_ITALIC_OFF.to_string());
    }
    let foreground_changed = from.foreground != to.foreground;
    let background_changed = from.background != to.background;
    if foreground_changed && background_changed {
        if to.background.is_none() {
            out.push(ANSI_BG_RESET.to_string());
            out.push(to.foreground.clone().unwrap_or_else(|| ANSI_FG_RESET.to_string()));
        } else {
            out.push(to.foreground.clone().unwrap_or_else(|| ANSI_FG_RESET.to_string()));
            out.push(to.background.clone().unwrap_or_else(|| ANSI_BG_RESET.to_string()));
        }
    } else {
        if foreground_changed {
            out.push(to.foreground.clone().unwrap_or_else(|| ANSI_FG_RESET.to_string()));
        }
        if background_changed {
            out.push(to.background.clone().unwrap_or_else(|| ANSI_BG_RESET.to_string()));
        }
    }
    if !from.bold && to.bold {
        out.push(ANSI_BOLD_ON.to_string());
    }
    if !from.italic && to.italic {
        out.push(ANSI_ITALIC_ON.to_string());
    }
}

fn emit_style_event(out: &mut Vec<String>, event: RenderEvent, active: RenderStyle) -> RenderStyle {
    match event {
        RenderEvent::ScopeStart(attribute) => {
            out.push(scope_on(attribute).to_string());
            let mut next = active;
            set_style_attribute(&mut next, attribute, true);
            next
        }
        RenderEvent::ScopeEnd { attribute, restore } => {
            if style_attribute(&active, attribute) {
                out.push(scope_off(attribute).to_string());
            }
            if restore {
                out.push(scope_on(attribute).to_string());
            }
            let mut next = active;
            set_style_attribute(&mut next, attribute, restore);
            next
        }
    }
}

fn scope_on(attribute: StyleAttribute) -> &'static str {
    match attribute {
        StyleAttribute::Bold => ANSI_BOLD_ON,
        StyleAttribute::Italic => ANSI_ITALIC_ON,
    }
}

fn scope_off(attribute: StyleAttribute) -> &'static str {
    match attribute {
        StyleAttribute::Bold => ANSI_BOLD_OFF,
        StyleAttribute::Italic => ANSI_ITALIC_OFF,
    }
}

fn set_style_attribute(style: &mut RenderStyle, attribute: StyleAttribute, value: bool) {
    match attribute {
        StyleAttribute::Bold => style.bold = value,
        StyleAttribute::Italic => style.italic = value,
    }
}

/// Materialize terminal attributes only after all glyph transforms are complete.
fn render_ansi(text: &[Chunk]) -> String {
    let mut out = String::new();
    let mut active = RenderStyle::default();
    for chunk in text {
        let mut transitions = Vec::new();
        emit_style_transition(&mut transitions, &active, &chunk.style);
        out.push_str(&transitions.join(""));
        active = chunk.style.clone();
        if let Some(events) = &chunk.before {
            for event in events {
                let mut emitted = Vec::new();
                active = emit_style_event(&mut emitted, *event, active);
                out.push_str(&emitted.join(""));
            }
        }
        out.push_str(&chunk.text);
        if let Some(events) = &chunk.after {
            for event in events {
                let mut emitted = Vec::new();
                active = emit_style_event(&mut emitted, *event, active);
                out.push_str(&emitted.join(""));
            }
        }
    }
    let mut transitions = Vec::new();
    emit_style_transition(&mut transitions, &active, &RenderStyle::default());
    out.push_str(&transitions.join(""));
    out
}

/// Light unescape for text-mode content (`\&` → `&`, `~` → space).
fn unescape_text(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            if let Some(next) = chars.get(i + 1) {
                if "&%$#_{}".contains(*next) || next.is_whitespace() {
                    out.push(*next);
                    i += 2;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out.replace('~', " ")
}

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

/// Named colors accepted by the LaTeX color commands.
static LATEX_NAMED_COLORS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("black", "#000000"),
        ("blue", "#0000ff"),
        ("brown", "#a52a2a"),
        ("cyan", "#00ffff"),
        ("darkgray", "#404040"),
        ("darkgrey", "#404040"),
        ("gray", "#808080"),
        ("green", "#00ff00"),
        ("grey", "#808080"),
        ("lightgray", "#c0c0c0"),
        ("lightgrey", "#c0c0c0"),
        ("lime", "#00ff00"),
        ("magenta", "#ff00ff"),
        ("olive", "#808000"),
        ("orange", "#ffa500"),
        ("pink", "#ffc0cb"),
        ("purple", "#800080"),
        ("red", "#ff0000"),
        ("teal", "#008080"),
        ("violet", "#ee82ee"),
        ("white", "#ffffff"),
        ("yellow", "#ffff00"),
    ])
});

/// Terminal color depth assumed by [`latex_to_unicode`].
///
/// The TypeScript original reads `TERMINAL.trueColor` from
/// `terminal-capabilities`; the Rust crate has no capability probe yet. The
/// default is truecolor (24-bit); callers with a known 256-color terminal
/// should use [`latex_to_unicode_with`] / [`latex_color_scope_with`].
pub const DEFAULT_TRUE_COLOR: bool = true;

/// An ANSI foreground/background pair for one resolved color.
struct AnsiColor {
    foreground: String,
    background: String,
}

fn clamp01(n: f64) -> f64 {
    if n <= 0.0 {
        0.0
    } else if n >= 1.0 {
        1.0
    } else {
        n
    }
}

fn clamp_byte(n: f64) -> u8 {
    if n <= 0.0 {
        0
    } else if n >= 255.0 {
        255
    } else {
        n.round() as u8
    }
}

fn css_rgb(rgb: (f64, f64, f64)) -> String {
    format!("rgb({}, {}, {})", clamp_byte(rgb.0), clamp_byte(rgb.1), clamp_byte(rgb.2))
}

fn parse_number(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let value = match trimmed.strip_suffix('%') {
        Some(number) => number.trim().parse::<f64>().ok()? / 100.0,
        None => trimmed.parse::<f64>().ok()?,
    };
    if value.is_finite() {
        Some(value)
    } else {
        None
    }
}

fn parse_color_components(spec: &str, expected: usize) -> Option<Vec<f64>> {
    let parts: Vec<&str> = spec
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() != expected {
        return None;
    }
    let mut values = Vec::with_capacity(expected);
    for part in parts {
        values.push(parse_number(part)?);
    }
    Some(values)
}

fn rgb_from_unit(values: &[f64]) -> Option<String> {
    if values.len() != 3 {
        return None;
    }
    Some(css_rgb((
        clamp01(values[0]) * 255.0,
        clamp01(values[1]) * 255.0,
        clamp01(values[2]) * 255.0,
    )))
}

fn rgb_from_byte(values: &[f64]) -> Option<String> {
    if values.len() != 3 {
        return None;
    }
    Some(css_rgb((values[0], values[1], values[2])))
}

fn rgb_from_cmyk(values: &[f64]) -> Option<String> {
    if values.len() != 4 {
        return None;
    }
    let c = clamp01(values[0]);
    let m = clamp01(values[1]);
    let y = clamp01(values[2]);
    let k = clamp01(values[3]);
    Some(css_rgb((255.0 * (1.0 - c) * (1.0 - k), 255.0 * (1.0 - m) * (1.0 - k), 255.0 * (1.0 - y) * (1.0 - k))))
}

fn rgb_from_hsv(values: &[f64], hue_scale: f64) -> Option<String> {
    if values.len() != 3 {
        return None;
    }
    let h = ((values[0] * hue_scale) % 360.0) / 60.0;
    let s = clamp01(values[1]);
    let v = clamp01(values[2]);
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 1.0 {
        (c, x, 0.0)
    } else if h < 2.0 {
        (x, c, 0.0)
    } else if h < 3.0 {
        (0.0, c, x)
    } else if h < 4.0 {
        (0.0, x, c)
    } else if h < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Some(css_rgb(((r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0)))
}

fn rgb_from_wave(spec: &str) -> Option<String> {
    let wavelength = parse_number(spec)?;
    if wavelength < 380.0 || wavelength > 780.0 {
        return None;
    }
    let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
    if wavelength < 440.0 {
        r = -(wavelength - 440.0) / 60.0;
        b = 1.0;
    } else if wavelength < 490.0 {
        g = (wavelength - 440.0) / 50.0;
        b = 1.0;
    } else if wavelength < 510.0 {
        g = 1.0;
        b = -(wavelength - 510.0) / 20.0;
    } else if wavelength < 580.0 {
        r = (wavelength - 510.0) / 70.0;
        g = 1.0;
    } else if wavelength < 645.0 {
        r = 1.0;
        g = -(wavelength - 645.0) / 65.0;
    } else {
        r = 1.0;
    }
    let factor = if wavelength < 420.0 {
        0.3 + (0.7 * (wavelength - 380.0)) / 40.0
    } else if wavelength > 700.0 {
        0.3 + (0.7 * (780.0 - wavelength)) / 80.0
    } else {
        1.0
    };
    Some(css_rgb((r * factor * 255.0, g * factor * 255.0, b * factor * 255.0)))
}

/// Whether `spec` looks like a CSS color the converter can quantify.
fn is_css_color(spec: &str) -> bool {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return false;
    }
    if let Some(hex) = trimmed.strip_prefix('#') {
        return !hex.is_empty() && hex.len() <= 8 && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    let lower = trimmed.to_ascii_lowercase();
    for prefix in ["rgb(", "rgba(", "hsl(", "hsla("] {
        if lower.starts_with(prefix) {
            return trimmed.ends_with(')');
        }
    }
    false
}

fn normalize_css_color(spec: &str, allow_mix: bool) -> Option<String> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }
    if allow_mix && trimmed.contains('!') {
        if let Some(mixed) = resolve_mixed_color(trimmed) {
            return Some(mixed);
        }
    }
    if let Some(named) = LATEX_NAMED_COLORS.get(trimmed) {
        return Some((*named).to_string());
    }
    if let Some(named) = LATEX_NAMED_COLORS.get(trimmed.to_ascii_lowercase().as_str()) {
        return Some((*named).to_string());
    }
    if is_css_color(trimmed) {
        return Some(trimmed.to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower != trimmed && is_css_color(&lower) {
        return Some(lower);
    }
    None
}

fn resolve_modeled_color(model: &str, spec: &str) -> Option<String> {
    let trimmed_model = model.trim();
    if trimmed_model.is_empty() || trimmed_model == "named" {
        return normalize_css_color(spec, true);
    }
    if trimmed_model == "HTML" || trimmed_model == "Html" || trimmed_model == "html" {
        let hex = spec.trim().strip_prefix('#').unwrap_or(spec.trim());
        return if !hex.is_empty() && hex.len() <= 8 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            Some(format!("#{hex}"))
        } else {
            None
        };
    }
    if trimmed_model == "wave" {
        return rgb_from_wave(spec);
    }
    let lower = trimmed_model.to_ascii_lowercase();
    if trimmed_model == "RGB" {
        return rgb_from_byte(&parse_color_components(spec, 3).unwrap_or_default());
    }
    if lower == "rgb" {
        return rgb_from_unit(&parse_color_components(spec, 3).unwrap_or_default());
    }
    if lower == "cmyk" {
        return rgb_from_cmyk(&parse_color_components(spec, 4).unwrap_or_default());
    }
    if lower == "gray" || lower == "grey" {
        let value = parse_color_components(spec, 1)?.first().copied()?;
        let unit = if trimmed_model == "Gray" || trimmed_model == "Grey" { value / 15.0 } else { value };
        let byte = clamp01(unit) * 255.0;
        return Some(css_rgb((byte, byte, byte)));
    }
    if lower == "hsb" || lower == "hsv" {
        let values = parse_color_components(spec, 3)?;
        return rgb_from_hsv(&values, if trimmed_model == "Hsb" || trimmed_model == "HSV" { 1.0 } else { 360.0 });
    }
    normalize_css_color(spec, true)
}

fn resolve_latex_color(model: Option<&str>, spec: &str) -> Option<String> {
    let unescaped = unescape_text(spec).trim().to_string();
    if unescaped.is_empty() {
        return None;
    }
    match model {
        None => normalize_css_color(&unescaped, true),
        Some(model) => resolve_modeled_color(model, &unescaped),
    }
}

fn resolve_mixed_color(spec: &str) -> Option<String> {
    let parts: Vec<&str> = spec.split('!').collect();
    if parts.len() < 2 {
        return None;
    }
    let first = normalize_css_color(parts.first().copied().unwrap_or(""), false)?;
    let mut current = css_to_rgb(&first)?;
    let mut i = 1;
    while i < parts.len() {
        let percent = parse_number(parts[i])?;
        let next_spec = parts.get(i + 1).copied().unwrap_or("white");
        let next_color = normalize_css_color(next_spec, false)?;
        let next = css_to_rgb(&next_color)?;
        let t = clamp01(percent / 100.0);
        current = (
            current.0 * t + next.0 * (1.0 - t),
            current.1 * t + next.1 * (1.0 - t),
            current.2 * t + next.2 * (1.0 - t),
        );
        i += 2;
    }
    Some(css_rgb(current))
}

fn ansi_color(true_color: bool, model: Option<&str>, spec: &str) -> Option<AnsiColor> {
    let css = resolve_latex_color(model, spec)?;
    let foreground = to_ansi(&css, true_color)?;
    if !foreground.starts_with("\x1b[38;") {
        return None;
    }
    let background = foreground.replacen("\x1b[38;", "\x1b[48;", 1);
    Some(AnsiColor { foreground, background })
}

/// A painter for a LaTeX color scope: paints already-rendered text with the
/// scope's foreground, re-asserting it after embedded foreground resets so
/// nested color runs restore to the scope color.
#[derive(Clone, Debug)]
pub struct ColorScope {
    foreground: String,
}

impl ColorScope {
    /// Paint `text`, restoring the scope color after any embedded reset.
    pub fn paint(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + (self.foreground.len() + 4) * 2);
        out.push_str(&self.foreground);
        out.push_str(&text.replace(ANSI_FG_RESET, &self.foreground));
        out.push_str(ANSI_FG_RESET);
        out
    }
}

/// Painter for a LaTeX color scope (optional model + spec, e.g. `rgb`/`1,0,0`
/// or `red`); `None` when the color cannot be resolved.
pub fn latex_color_scope(model: Option<&str>, spec: &str) -> Option<ColorScope> {
    latex_color_scope_with(model, spec, DEFAULT_TRUE_COLOR)
}

/// [`latex_color_scope`] with an explicit terminal color depth.
pub fn latex_color_scope_with(model: Option<&str>, spec: &str, true_color: bool) -> Option<ColorScope> {
    let color = ansi_color(true_color, model, spec)?;
    Some(ColorScope { foreground: color.foreground })
}

fn hex_to_rgb(hex: &str) -> Option<(f64, f64, f64)> {
    let digits = hex.strip_prefix('#')?;
    let expand = |s: &str| -> Option<f64> {
        if s.len() == 1 {
            let v = s.to_ascii_lowercase().parse::<u32>().ok()?;
            Some((v * 17) as f64)
        } else if s.len() == 2 {
            let v = u32::from_str_radix(s, 16).ok()?;
            Some(v as f64)
        } else {
            None
        }
    };
    match digits.len() {
        3 | 4 => {
            let r = expand(&digits[0..1])?;
            let g = expand(&digits[1..2])?;
            let b = expand(&digits[2..3])?;
            Some((r, g, b))
        }
        6 | 8 => {
            let r = expand(&digits[0..2])?;
            let g = expand(&digits[2..4])?;
            let b = expand(&digits[4..6])?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn parse_component(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if let Some(percent) = trimmed.strip_suffix('%') {
        Some(percent.trim().parse::<f64>().ok()? / 100.0 * 255.0)
    } else {
        Some(trimmed.parse::<f64>().ok()?)
    }
}

fn parse_rgb_fn(spec: &str) -> Option<(f64, f64, f64)> {
    let lower = spec.to_ascii_lowercase();
    let body = if lower.starts_with("rgba(") {
        spec.strip_prefix("rgba(")?.strip_suffix(')')?
    } else {
        let stripped = spec.strip_prefix("rgb(")?;
        stripped.strip_suffix(')')?
    };
    let parts: Vec<&str> = body
        .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    Some((parse_component(parts[0])?, parse_component(parts[1])?, parse_component(parts[2])?))
}

fn parse_hsl_fn(spec: &str) -> Option<(f64, f64, f64)> {
    let lower = spec.to_ascii_lowercase();
    let body = if lower.starts_with("hsla(") {
        spec.strip_prefix("hsla(")?.strip_suffix(')')?
    } else {
        spec.strip_prefix("hsl(")?.strip_suffix(')')?
    };
    let parts: Vec<&str> = body
        .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    let h = parts[0].trim().parse::<f64>().ok()?;
    let s = parts[1].trim().strip_suffix('%')?.trim().parse::<f64>().ok()? / 100.0;
    let l = parts[2].trim().strip_suffix('%')?.trim().parse::<f64>().ok()? / 100.0;
    let hue = h.rem_euclid(360.0) / 360.0;
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        (hue_to_rgb(p, q, hue + 1.0 / 3.0), hue_to_rgb(p, q, hue), hue_to_rgb(p, q, hue - 1.0 / 3.0))
    };
    Some((r * 255.0, g * 255.0, b * 255.0))
}

fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

/// Quantize an RGB triple to the xterm-256 palette index.
fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    if r == g && g == b {
        if r < 8 {
            return 16;
        }
        if r > 248 {
            return 231;
        }
        return (232 + ((r as u32 - 8) * 24 / 247)) as u8;
    }
    let index = |v: u8| -> u32 { (v as u32 * 5 + 127) / 255 };
    (16 + 36 * index(r) + 6 * index(g) + index(b)) as u8
}

fn to_ansi(css: &str, true_color: bool) -> Option<String> {
    let (r, g, b) = css_to_rgb(css)?;
    let (r, g, b) = (clamp_byte(r), clamp_byte(g), clamp_byte(b));
    if true_color {
        Some(format!("\x1b[38;2;{};{};{}m", r, g, b))
    } else {
        Some(format!("\x1b[38;5;{}m", rgb_to_256(r, g, b)))
    }
}

fn css_to_rgb(css: &str) -> Option<(f64, f64, f64)> {
    let trimmed = css.trim();
    if let Some(hex) = LATEX_NAMED_COLORS.get(trimmed) {
        return hex_to_rgb(hex);
    }
    if let Some(hex) = LATEX_NAMED_COLORS.get(trimmed.to_ascii_lowercase().as_str()) {
        return hex_to_rgb(hex);
    }
    if let Some(hex) = trimmed.strip_prefix('#') {
        return hex_to_rgb(&format!("#{hex}"));
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("rgb") {
        return parse_rgb_fn(trimmed);
    }
    if lower.starts_with("hsl") {
        return parse_hsl_fn(trimmed);
    }
    None
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// One parsed argument: rendered text plus whether it came from a `{…}` group
/// (which affects fraction/script parentheses).
struct Argument {
    text: Rendered,
    group: bool,
}

static BIG_DELIM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[bB]igg?|[bB]igg?[lrm])$").unwrap());

/// Extensible-arrow commands and their base glyph.
static EXTENSIBLE_ARROWS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("xleftarrow", "←"),
        ("xrightarrow", "→"),
        ("xleftrightarrow", "↔"),
        ("xLeftarrow", "⇐"),
        ("xRightarrow", "⇒"),
        ("xLeftrightarrow", "⇔"),
        ("xhookleftarrow", "↩"),
        ("xhookrightarrow", "↪"),
        ("xtwoheadleftarrow", "↞"),
        ("xtwoheadrightarrow", "↠"),
        ("xmapsto", "↦"),
        ("xrightharpoonup", "⇀"),
        ("xrightharpoondown", "⇁"),
        ("xleftharpoonup", "↼"),
        ("xleftharpoondown", "↽"),
        ("xrightleftharpoons", "⇌"),
        ("xleftrightharpoons", "⇋"),
    ])
});

/// Recursive-descent parser over a LaTeX math fragment.
struct LatexParser {
    s: Vec<char>,
    i: usize,
    foreground: Option<String>,
    background: Option<String>,
    true_color: bool,
}

impl LatexParser {
    /// Create a parser over `src`.
    fn new(src: &str, true_color: bool) -> Self {
        LatexParser {
            s: src.chars().collect(),
            i: 0,
            foreground: None,
            background: None,
            true_color,
        }
    }

    /// Render the whole fragment to an ANSI string.
    fn render(&mut self) -> String {
        render_ansi(&self.parse(ROOT_STYLE, false))
    }

    /// Parse a run until end-of-input, or until `}` when `stop_at_brace`.
    fn parse(&mut self, style: ParseStyle, stop_at_brace: bool) -> Rendered {
        let mut out: Rendered = Vec::new();
        while self.i < self.s.len() {
            if self.s[self.i] == '}' {
                if stop_at_brace {
                    break;
                }
                self.i += 1; // stray close brace
                continue;
            }
            let node = self.node(style);
            append_rendered(&mut out, &node);
        }
        out
    }

    fn render_style(&self, style: ParseStyle) -> RenderStyle {
        render_style(style, self.foreground.clone(), self.background.clone())
    }

    /// Parse one node at the cursor.
    fn node(&mut self, style: ParseStyle) -> Rendered {
        let c = match self.s.get(self.i) {
            Some(&c) => c,
            None => return Vec::new(),
        };
        match c {
            '\\' => self.command(style),
            '{' => self.group(style),
            '^' => {
                self.i += 1;
                self.script(style, true)
            }
            '_' => {
                self.i += 1;
                self.script(style, false)
            }
            '$' => {
                self.i += 1;
                Vec::new() // stray delimiter
            }
            '~' => {
                self.i += 1;
                styled_text(" ", self.render_style(style)) // non-breaking space
            }
            '&' => {
                self.i += 1;
                styled_text("  ", self.render_style(style)) // column separator
            }
            '\'' => {
                let mut k = 0;
                while self.s.get(self.i) == Some(&'\'') {
                    k += 1;
                    self.i += 1;
                }
                let text = if k <= 4 { PRIMES[k].to_string() } else { PRIMES[1].repeat(k) };
                styled_text(&text, self.render_style(style))
            }
            '%' => {
                let newline = self.s[self.i..].iter().position(|&ch| ch == '\n').map(|p| self.i + p);
                self.i = match newline {
                    Some(p) => p + 1,
                    None => self.s.len(),
                };
                Vec::new()
            }
            _ => {
                self.i += 1;
                styled_text(&style_char(c, style), self.render_style(style))
            }
        }
    }

    /// Parse a command (the cursor is on the backslash).
    fn command(&mut self, style: ParseStyle) -> Rendered {
        self.i += 1; // past backslash
        let c = match self.s.get(self.i) {
            Some(&c) => c,
            None => return Vec::new(),
        };
        if !c.is_ascii_alphabetic() {
            self.i += 1;
            let current = self.render_style(style);
            return match c {
                '\\' => styled_text("\n", current),
                '{' | '}' | '$' | '%' | '&' | '#' | '_' | ' ' | '.' => {
                    styled_text(&c.to_string(), current)
                }
                ',' | ':' | ';' | '>' => styled_text(" ", current), // spacing
                '!' | '/' | '(' | ')' | '[' | ']' => Vec::new(),
                '|' => styled_text("‖", current),
                _ => styled_text(&c.to_string(), current),
            };
        }
        let mut name = String::new();
        while let Some(&ch) = self.s.get(self.i) {
            if ch.is_ascii_alphabetic() {
                name.push(ch);
                self.i += 1;
            } else {
                break;
            }
        }
        if self.s.get(self.i) == Some(&'*') {
            self.i += 1; // starred variants (operatorname*, …)
        }
        self.apply_command(&name, style)
    }

    fn apply_command(&mut self, name: &str, style: ParseStyle) -> Rendered {
        let current = self.render_style(style);
        if let Some(&font) = FONTS.get(name) {
            let mut styled = style;
            styled.font = Some(font);
            return self.argument(styled).text;
        }
        if let Some(&text_style) = TEXT_STYLE_COMMANDS.get(name) {
            let mut styled = style;
            if let Some(bold) = text_style.bold {
                styled.bold = bold;
            }
            if let Some(italic) = text_style.italic {
                styled.italic = italic;
            }
            let text = self.argument(styled).text;
            let attribute = if text_style.bold == Some(true) {
                Some(StyleAttribute::Bold)
            } else if text_style.italic == Some(true) {
                Some(StyleAttribute::Italic)
            } else {
                None
            };
            return match attribute {
                Some(attribute) => {
                    let parent = style_attribute(&current, attribute);
                    scope_rendered(&text, attribute, parent)
                }
                None => text,
            };
        }
        if name == "textnormal" {
            let mut styled = style;
            styled.bold = false;
            styled.italic = false;
            return styled_text(&unescape_text(&self.raw_argument()), self.render_style(styled));
        }
        if TEXT_COMMANDS.contains(name) {
            return styled_text(&unescape_text(&self.raw_argument()), current);
        }
        if name == "operatorname" {
            let function = unescape_text(&self.raw_argument());
            return styled_text(&format!("{}{}", function, self.space_before_arg()), current);
        }
        if let Some(&accent) = ACCENTS.get(name) {
            return apply_combining(&self.argument(style).text, accent);
        }
        if matches!(name, "frac" | "dfrac" | "tfrac" | "cfrac") {
            let num = self.argument(style);
            let den = self.argument(style);
            return fraction(&num, &den, current);
        }
        if name == "genfrac" {
            let left = self.argument(style).text;
            let right = self.argument(style).text;
            self.raw_argument(); // rule thickness
            self.raw_argument(); // math style
            let num = self.argument(style);
            let den = self.argument(style);
            let middle = fraction(&num, &den, current);
            return concat_rendered([left, middle, right]);
        }
        if matches!(name, "binom" | "dbinom" | "tbinom") {
            let n = self.argument(style);
            let k = self.argument(style);
            return concat_rendered([
                styled_text("C(", current.clone()),
                n.text,
                styled_text(", ", current.clone()),
                k.text,
                styled_text(")", current),
            ]);
        }
        if name == "sqrt" {
            return self.sqrt(style);
        }
        if name == "not" {
            let arg = self.argument(style);
            let plain = plain_text(&arg.text);
            return match NOT_MAP.get(plain.as_str()) {
                Some(&mapped) => {
                    let style = arg.text.first().map(|chunk| chunk.style.clone()).unwrap_or(current);
                    styled_text(mapped, style)
                }
                None => apply_combining(&arg.text, "\u{0338}"),
            };
        }
        if name == "overset" || name == "stackrel" {
            return self.scripted_above(style);
        }
        if name == "underset" {
            return self.scripted_below(style);
        }
        if name == "prescript" {
            return self.prescript(style);
        }
        if let Some(&arrow) = EXTENSIBLE_ARROWS.get(name) {
            return self.extensible_arrow(style, arrow);
        }
        if name == "boxed" || name == "fbox" {
            return surround_rendered(self.argument(style).text, "[", "]", current);
        }
        if name == "overbrace" {
            return surround_rendered(self.argument(style).text, "⏞(", ")", current);
        }
        if name == "underbrace" {
            return surround_rendered(self.argument(style).text, "⏟(", ")", current);
        }
        if name == "overbracket" {
            return surround_rendered(self.argument(style).text, "⎴(", ")", current);
        }
        if name == "underbracket" {
            return surround_rendered(self.argument(style).text, "⎵(", ")", current);
        }
        if name == "overparen" {
            return surround_rendered(self.argument(style).text, "⏜(", ")", current);
        }
        if name == "underparen" {
            return surround_rendered(self.argument(style).text, "⏝(", ")", current);
        }
        if name == "cancel" {
            return apply_combining(&self.argument(style).text, "\u{0338}");
        }
        if name == "bcancel" {
            return apply_combining(&self.argument(style).text, "\u{20E5}");
        }
        if name == "xcancel" {
            return apply_combining(&apply_combining(&self.argument(style).text, "\u{0338}"), "\u{20E5}");
        }
        if name == "sout" {
            return apply_combining(&self.argument(style).text, "\u{0336}");
        }
        if name == "substack" {
            let arg = self.argument(style).text;
            return arg
                .into_iter()
                .map(|mut chunk| {
                    chunk.text = chunk.text.replace('\n', ",");
                    chunk
                })
                .collect();
        }
        if name == "left" || name == "right" || name == "middle" {
            return self.delimiter(style);
        }
        if BIG_DELIM.is_match(name) {
            return self.delimiter(style);
        }
        if name == "begin" {
            return self.environment(style);
        }
        if name == "end" {
            self.raw_argument();
            return Vec::new();
        }
        if name == "bmod" {
            return styled_text(" mod ", current);
        }
        if name == "pmod" {
            return surround_rendered(self.argument(style).text, "(mod ", ")", current);
        }
        if name == "pod" {
            return surround_rendered(self.argument(style).text, "(", ")", current);
        }
        if name == "tag" {
            return surround_rendered(self.argument(style).text, "(", ")", current);
        }
        if name == "label" {
            self.raw_argument();
            return Vec::new();
        }
        if name == "ref" || name == "eqref" {
            return styled_text(&format!("({})", unescape_text(&self.raw_argument())), current);
        }
        if name == "url" {
            return styled_text(&unescape_text(&self.raw_argument()), current);
        }
        if name == "href" {
            self.raw_argument();
            return self.argument(style).text;
        }
        if name == "textcolor" {
            let color = self.read_ansi_color();
            return self.scoped_foreground(color, style);
        }
        if name == "colorbox" {
            let color = self.read_ansi_color();
            return self.scoped_background(color, style);
        }
        if name == "fcolorbox" {
            return self.fcolorbox(style);
        }
        if name == "color" {
            return self.set_foreground();
        }
        if name == "normalcolor" {
            self.foreground = None;
            return Vec::new();
        }
        if name == "phantom" || name == "hphantom" {
            let arg = self.argument(style).text;
            return styled_text(&" ".repeat(code_point_length(&arg)), current);
        }
        if name == "vphantom" {
            self.argument(style);
            return Vec::new();
        }
        if FUNCTIONS.contains(name) {
            return styled_text(&format!("{}{}", name, self.space_before_arg()), current);
        }
        if let Some(&symbol) = SYMBOLS.get(name) {
            return styled_text(symbol, current);
        }
        match name {
            "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" | "limits"
            | "nolimits" | "nonumber" | "notag" => Vec::new(),
            "quad" => styled_text("  ", current),
            "qquad" => styled_text("    ", current),
            "thinspace" | "enspace" | "medspace" | "thickspace" | "space" => {
                styled_text(" ", current)
            }
            "negthinspace" | "negmedspace" | "negthickspace" => Vec::new(),
            _ => styled_text(name, current),
        }
    }

    fn group(&mut self, style: ParseStyle) -> Rendered {
        self.i += 1;
        let outer_foreground = self.foreground.clone();
        let outer_background = self.background.clone();
        let inner = self.parse(style, true);
        if self.s.get(self.i) == Some(&'}') {
            self.i += 1;
        }
        self.foreground = outer_foreground;
        self.background = outer_background;
        inner
    }

    fn read_ansi_color(&mut self) -> Option<AnsiColor> {
        let model = self.optional_raw_argument();
        let spec = self.raw_argument();
        ansi_color(self.true_color, model.as_deref(), &spec)
    }

    fn set_foreground(&mut self) -> Rendered {
        if let Some(color) = self.read_ansi_color() {
            self.foreground = Some(color.foreground);
        }
        Vec::new()
    }

    fn scoped_foreground(&mut self, color: Option<AnsiColor>, style: ParseStyle) -> Rendered {
        let outer = self.foreground.clone();
        let color = match color {
            Some(color) => color,
            None => return self.argument(style).text,
        };
        self.foreground = Some(color.foreground);
        let arg = self.argument(style).text;
        self.foreground = outer;
        arg
    }

    fn scoped_background(&mut self, color: Option<AnsiColor>, style: ParseStyle) -> Rendered {
        let outer = self.background.clone();
        let color = match color {
            Some(color) => color,
            None => return self.argument(style).text,
        };
        self.background = Some(color.background);
        let arg = self.argument(style).text;
        self.background = outer;
        arg
    }

    fn fcolorbox(&mut self, style: ParseStyle) -> Rendered {
        let frame_model = self.optional_raw_argument();
        let frame_spec = self.raw_argument();
        let frame = ansi_color(self.true_color, frame_model.as_deref(), &frame_spec);
        let background_model = self.optional_raw_argument().or(frame_model);
        let background_spec = self.raw_argument();
        let background = ansi_color(self.true_color, background_model.as_deref(), &background_spec);
        let body = self.scoped_background(background, style);
        let frame_foreground = frame.map(|color| color.foreground).or_else(|| self.foreground.clone());
        let frame_style = render_style(style, frame_foreground, self.background.clone());
        concat_rendered([styled_text("[", frame_style.clone()), body, styled_text("]", frame_style)])
    }

    /// Read one argument: a `{…}` group, a single command, or a single char.
    fn argument(&mut self, style: ParseStyle) -> Argument {
        while self.s.get(self.i) == Some(&' ') {
            self.i += 1;
        }
        let c = match self.s.get(self.i) {
            Some(&c) => c,
            None => return Argument { text: Vec::new(), group: false },
        };
        if c == '{' {
            self.i += 1;
            let inner = self.parse(style, true);
            if self.s.get(self.i) == Some(&'}') {
                self.i += 1;
            }
            return Argument { text: inner, group: true };
        }
        if c == '\\' {
            return Argument { text: self.command(style), group: false };
        }
        if c == '^' || c == '_' {
            self.i += 1;
            let sup = c == '^';
            return Argument { text: self.script(style, sup), group: false };
        }
        self.i += 1;
        Argument { text: styled_text(&style_char(c, style), self.render_style(style)), group: false }
    }

    /// Read a raw (unparsed) argument, returning its literal source text.
    fn raw_argument(&mut self) -> String {
        while self.s.get(self.i) == Some(&' ') {
            self.i += 1;
        }
        if self.s.get(self.i) != Some(&'{') {
            let c = match self.s.get(self.i) {
                Some(&c) => c,
                None => return String::new(),
            };
            if c == '\\' {
                let mut token = String::from("\\");
                self.i += 1;
                if let Some(&ch) = self.s.get(self.i) {
                    if ch.is_ascii_alphabetic() {
                        while let Some(&ch) = self.s.get(self.i) {
                            if ch.is_ascii_alphabetic() {
                                token.push(ch);
                                self.i += 1;
                            } else {
                                break;
                            }
                        }
                    } else {
                        token.push(ch);
                        self.i += 1;
                    }
                }
                return token;
            }
            self.i += 1;
            return c.to_string();
        }
        self.i += 1; // past {
        let mut depth = 1;
        let mut out = String::new();
        while self.i < self.s.len() && depth > 0 {
            let c = self.s[self.i];
            if c == '\\' {
                out.push('\\');
                if let Some(&next) = self.s.get(self.i + 1) {
                    out.push(next);
                }
                self.i += 2;
                continue;
            }
            if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    self.i += 1;
                    break;
                }
            }
            out.push(c);
            self.i += 1;
        }
        out
    }

    fn script(&mut self, style: ParseStyle, sup: bool) -> Rendered {
        let current = self.render_style(style);
        let arg = self.argument(style);
        if sup {
            to_superscript(&arg.text, arg.group, current)
        } else {
            to_subscript(&arg.text, arg.group, current)
        }
    }

    fn scripted_above(&mut self, style: ParseStyle) -> Rendered {
        let current = self.render_style(style);
        let above = self.argument(style);
        let base = self.argument(style);
        concat_rendered([base.text, to_superscript(&above.text, true, current)])
    }

    fn scripted_below(&mut self, style: ParseStyle) -> Rendered {
        let current = self.render_style(style);
        let below = self.argument(style);
        let base = self.argument(style);
        concat_rendered([base.text, to_subscript(&below.text, true, current)])
    }

    fn prescript(&mut self, style: ParseStyle) -> Rendered {
        let current = self.render_style(style);
        let sup = self.argument(style);
        let sub = self.argument(style);
        let base = self.argument(style);
        concat_rendered([
            to_superscript(&sup.text, true, current.clone()),
            to_subscript(&sub.text, true, current),
            base.text,
        ])
    }

    fn extensible_arrow(&mut self, style: ParseStyle, arrow: &str) -> Rendered {
        let current = self.render_style(style);
        let below = self.optional_argument(style);
        let above = self.argument(style);
        let mut parts = vec![styled_text(arrow, current.clone())];
        parts.push(to_superscript(&above.text, true, current.clone()));
        if let Some(below) = below {
            parts.push(to_subscript(&below.text, true, current));
        }
        concat_rendered(parts)
    }

    fn delimiter(&mut self, style: ParseStyle) -> Rendered {
        while self.s.get(self.i) == Some(&' ') {
            self.i += 1;
        }
        let c = match self.s.get(self.i) {
            Some(&c) => c,
            None => return Vec::new(),
        };
        let current = self.render_style(style);
        if c == '.' {
            self.i += 1;
            return Vec::new();
        }
        if c != '\\' {
            self.i += 1;
            return styled_text(&style_char(c, style), current);
        }
        self.i += 1;
        let d = match self.s.get(self.i) {
            Some(&d) => d,
            None => return Vec::new(),
        };
        if !d.is_ascii_alphabetic() {
            self.i += 1;
            return match d {
                '.' => Vec::new(),
                '{' => styled_text("{", current),
                '}' => styled_text("}", current),
                '|' => styled_text("‖", current),
                _ => styled_text(&d.to_string(), current),
            };
        }
        let mut name = String::new();
        while let Some(&ch) = self.s.get(self.i) {
            if ch.is_ascii_alphabetic() {
                name.push(ch);
                self.i += 1;
            } else {
                break;
            }
        }
        styled_text(SYMBOLS.get(name.as_str()).copied().unwrap_or(name.as_str()), current)
    }

    fn optional_argument(&mut self, style: ParseStyle) -> Option<Argument> {
        let source = self.optional_raw_argument()?;
        let mut parser = LatexParser::new(&source, self.true_color);
        parser.foreground = self.foreground.clone();
        parser.background = self.background.clone();
        Some(Argument { text: parser.parse(style, false), group: true })
    }

    fn optional_raw_argument(&mut self) -> Option<String> {
        while self.s.get(self.i) == Some(&' ') {
            self.i += 1;
        }
        if self.s.get(self.i) != Some(&'[') {
            return None;
        }
        self.i += 1;
        let mut bracket_depth = 1;
        let mut brace_depth = 0;
        let mut out = String::new();
        while self.i < self.s.len() && bracket_depth > 0 {
            let c = self.s[self.i];
            if c == '\\' {
                out.push('\\');
                if let Some(&next) = self.s.get(self.i + 1) {
                    out.push(next);
                }
                self.i += 2;
                continue;
            }
            if c == '{' {
                brace_depth += 1;
            } else if c == '}' && brace_depth > 0 {
                brace_depth -= 1;
            } else if brace_depth == 0 && c == '[' {
                bracket_depth += 1;
            } else if brace_depth == 0 && c == ']' {
                bracket_depth -= 1;
                if bracket_depth == 0 {
                    self.i += 1;
                    break;
                }
            }
            out.push(c);
            self.i += 1;
        }
        Some(out)
    }

    fn sqrt(&mut self, style: ParseStyle) -> Rendered {
        while self.s.get(self.i) == Some(&' ') {
            self.i += 1;
        }
        let current = self.render_style(style);
        let mut radical = styled_text("√", current.clone());
        let index = self.optional_argument(style);
        if let Some(index) = index {
            let index_text = plain_text(&index.text);
            let index_matches_style =
                index.text.iter().all(|chunk| same_render_style(&chunk.style, &current));
            radical = if index_matches_style && index_text == "2" {
                styled_text("√", current.clone())
            } else if index_matches_style && index_text == "3" {
                styled_text("∛", current.clone())
            } else if index_matches_style && index_text == "4" {
                styled_text("∜", current.clone())
            } else {
                concat_rendered([
                    to_superscript(&index.text, true, current.clone()),
                    styled_text("√", current.clone()),
                ])
            };
        }
        let radicand = self.argument(style).text;
        let tail = if code_point_length(&radicand) > 1 {
            surround_rendered(radicand, "(", ")", current.clone())
        } else {
            radicand
        };
        concat_rendered([radical, tail])
    }

    fn environment(&mut self, style: ParseStyle) -> Rendered {
        let env = self.raw_argument().trim().to_string();
        if matches!(env.as_str(), "array" | "tabular" | "array*" | "tabular*") {
            self.optional_raw_argument();
            if self.s.get(self.i) == Some(&'{') {
                self.raw_argument(); // column spec
            }
        } else if matches!(
            env.as_str(),
            "alignedat" | "alignedat*" | "alignat" | "alignat*" | "gatheredat"
        ) {
            self.optional_raw_argument();
            if self.s.get(self.i) == Some(&'{') {
                self.raw_argument(); // column count
            }
        }
        let outer_foreground = self.foreground.clone();
        let outer_background = self.background.clone();
        let mut body: Rendered = Vec::new();
        while self.i < self.s.len() {
            if self.s[self.i..].starts_with(&['\\', 'e', 'n', 'd']) {
                self.i += 4;
                self.raw_argument();
                break;
            }
            let node = self.node(style);
            append_rendered(&mut body, &node);
        }
        self.foreground = outer_foreground;
        self.background = outer_background;
        let mut rendered_body = trim_rendered(&body);
        if matches!(
            env.as_str(),
            "cases" | "cases*" | "dcases" | "dcases*" | "rcases" | "drcases"
        ) {
            for chunk in &mut rendered_body {
                chunk.text = CASES_NEWLINE.replace_all(&chunk.text, "; ").to_string();
                chunk.text = CASES_SPACES.replace_all(&chunk.text, "  ").to_string();
            }
        }
        let delims = match ENV_DELIMS.get(env.as_str()) {
            Some(delims) => *delims,
            None => return rendered_body,
        };
        let current = self.render_style(style);
        concat_rendered([
            styled_text(delims.0, current.clone()),
            rendered_body,
            styled_text(delims.1, current),
        ])
    }

    /// A separator space when the next glyph is alphanumeric or a command.
    fn space_before_arg(&self) -> &'static str {
        match self.s.get(self.i) {
            Some(c) if c.is_ascii_alphanumeric() || *c == '\\' => " ",
            _ => "",
        }
    }
}

fn to_superscript(text: &[Chunk], group: bool, fallback: RenderStyle) -> Rendered {
    if text.is_empty() {
        return Vec::new();
    }
    if let Some(mapped) = map_all(text, &SUPERSCRIPT) {
        return mapped;
    }
    let left = if group { "^(" } else { "^" };
    let right = if group { ")" } else { "" };
    concat_rendered([
        styled_text(left, fallback.clone()),
        text.to_vec(),
        styled_text(right, fallback),
    ])
}

fn to_subscript(text: &[Chunk], group: bool, fallback: RenderStyle) -> Rendered {
    if text.is_empty() {
        return Vec::new();
    }
    if let Some(mapped) = map_all(text, &SUBSCRIPT) {
        return mapped;
    }
    let left = if group { "_(" } else { "_" };
    let right = if group { ")" } else { "" };
    concat_rendered([
        styled_text(left, fallback.clone()),
        text.to_vec(),
        styled_text(right, fallback),
    ])
}

fn wrap_frac(arg: &Argument, fallback: RenderStyle) -> Rendered {
    if !arg.group || code_point_length(&arg.text) <= 1 {
        return arg.text.clone();
    }
    surround_rendered(arg.text.clone(), "(", ")", fallback)
}

fn fraction(num: &Argument, den: &Argument, fallback: RenderStyle) -> Rendered {
    let num_text = plain_text(&num.text);
    let den_text = plain_text(&den.text);
    let first = num.text.first().map(|chunk| chunk.style.clone()).unwrap_or_else(|| fallback.clone());
    let same = num
        .text
        .iter()
        .chain(den.text.iter())
        .all(|chunk| same_render_style(&chunk.style, &first));
    let key = format!("{}/{}", num_text, den_text);
    if same {
        if let Some(&vulgar) = VULGAR.get(key.as_str()) {
            return styled_text(vulgar, first);
        }
    }
    concat_rendered([
        wrap_frac(num, fallback.clone()),
        styled_text("/", fallback.clone()),
        wrap_frac(den, fallback),
    ])
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

static NEWLINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n+").unwrap());
static CASES_NEWLINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]*\n+[ \t]*").unwrap());
static CASES_SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" {3,}").unwrap());
static ENV_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\(?:begin|end)\{([^}]*)\}").unwrap());
static PREV_LINE_TAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:=|\(|\[|\{)\s*$").unwrap());

static BARE_MATH_LINE_COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\\(?:operatorname|frac|dfrac|tfrac|cfrac|genfrac|sqrt|sum|prod|coprod|int|iint|iiint|lim|alpha|beta|gamma|delta|epsilon|varepsilon|theta|lambda|mu|sigma|phi|varphi|pi|omega|infty|partial|nabla|forall|exists|mathbb|mathcal|mathscr|mathbf|mathrm|left|right|begin|phantom|hphantom|vphantom|cdots|ldots|dots|to|rightarrow|leftarrow|leq|geq|neq|times|cdot|overline|underline|vec|hat|bar|textbf|textit|textsl|emph|textmd|textup|texttt|textsf|textcolor|color|normalcolor|colorbox|fcolorbox)\b",
    )
    .unwrap()
});

/// Display-math environments eligible for delimiter-less ("bare") rendering in
/// prose. Deliberately excludes text-mode table/list/float environments
/// (`tabular`, `itemize`, `verbatim`, `document`, …) so ordinary LaTeX quoted in
/// prose or fenced code stays verbatim instead of being mangled.
static BARE_MATH_ENVIRONMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "matrix",
        "smallmatrix",
        "pmatrix",
        "bmatrix",
        "Bmatrix",
        "vmatrix",
        "Vmatrix",
        "cases",
        "dcases",
        "rcases",
        "drcases",
        "aligned",
        "alignedat",
        "align",
        "alignat",
        "split",
        "gathered",
        "gatheredat",
        "gather",
        "multline",
        "equation",
        "eqnarray",
        "array",
        "subarray",
    ])
});

/// True when `env` is a math environment safe to auto-render without `$`/`\[`
/// delimiters. The trailing `*` of starred variants (`align*`, `equation*`) is
/// ignored; text-mode environments (`tabular`, `itemize`, …) return false.
pub fn is_bare_math_environment(env: &str) -> bool {
    let base = env.strip_suffix('*').unwrap_or(env);
    BARE_MATH_ENVIRONMENTS.contains(base)
}

/// Collapse every run of newlines to a single space.
fn collapse_newlines(text: &str) -> String {
    NEWLINES.replace_all(text, " ").to_string()
}

/// Convert a bare LaTeX math fragment (no surrounding `$`/`\(` delimiters) to
/// its best-effort Unicode rendering. Unknown commands degrade to their bare
/// name; `\\` becomes a newline. Always returns a string (never panics).
pub fn latex_to_unicode(src: &str) -> String {
    latex_to_unicode_with(src, DEFAULT_TRUE_COLOR)
}

/// [`latex_to_unicode`] with an explicit terminal color depth.
pub fn latex_to_unicode_with(src: &str, true_color: bool) -> String {
    if src.is_empty() {
        return src.to_string();
    }
    LatexParser::new(src, true_color).render()
}

/// The first `\begin{…}` / `\end{…}` environment name in `text`, if any.
fn bare_env_name(text: &str) -> Option<String> {
    ENV_NAME.captures(text).map(|captures| captures[1].to_string())
}

fn should_render_bare_math_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || !trimmed.contains('\\') {
        return false;
    }
    if let Some(env) = bare_env_name(trimmed) {
        if !is_bare_math_environment(&env) {
            return false;
        }
    }
    if !BARE_MATH_LINE_COMMAND.is_match(trimmed) {
        return false;
    }
    trimmed.starts_with('\\') || ['=', '<', '>', '^', '_', '{', '}', '&'].iter().any(|c| trimmed.contains(*c))
}

fn render_bare_math_lines(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut line_start = 0;
    for i in 0..=chars.len() {
        if i != chars.len() && chars[i] != '\n' {
            continue;
        }
        let line: String = chars[line_start..i].iter().collect();
        if should_render_bare_math_line(&line) {
            out.push_str(&collapse_newlines(&latex_to_unicode(&line)));
        } else {
            out.push_str(&line);
        }
        if i != chars.len() {
            out.push('\n');
        }
        line_start = i + 1;
    }
    out
}

/// Index of the last `\n` at or before `upto`, plus one (line start).
fn line_start_before(chars: &[char], upto: usize) -> usize {
    match chars[..=upto].iter().rposition(|&c| c == '\n') {
        Some(position) => position + 1,
        None => 0,
    }
}

/// Convert delimiter-less math in prose: whole `\begin{env}…\end{env}` math
/// blocks (optionally pulling in a preceding `lhs =` line) plus standalone
/// math-shaped lines. A non-math environment is emitted verbatim — wrappers
/// *and* body — so a quoted `\begin{verbatim}…\frac…\end{verbatim}` is never
/// touched.
fn render_bare_math_in_text(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let begin_token = ['\\', 'b', 'e', 'g', 'i', 'n', '{'];
    let mut out = String::new();
    let mut i = 0;
    loop {
        let begin = match find_sub(&chars, &begin_token, i) {
            Some(position) => position,
            None => return out + &render_bare_math_lines(&chars[i..].iter().collect::<String>()),
        };
        let env_start = begin + begin_token.len();
        let env_end = match chars[env_start..].iter().position(|&c| c == '}') {
            Some(offset) => env_start + offset,
            None => return out + &render_bare_math_lines(&chars[i..].iter().collect::<String>()),
        };
        let env: String = chars[env_start..env_end].iter().collect();
        let close_token: Vec<char> = format!("\\end{{{env}}}").chars().collect();
        let close = match find_sub(&chars, &close_token, env_end + 1) {
            Some(position) => position,
            None => {
                // Unterminated `\begin`: convert lines up to it, then rescan past it.
                out.push_str(&render_bare_math_lines(&chars[i..=env_end].iter().collect::<String>()));
                i = env_end + 1;
                continue;
            }
        };
        let block_end = close + close_token.len();
        if !is_bare_math_environment(&env) {
            out.push_str(&render_bare_math_lines(&chars[i..begin].iter().collect::<String>()));
            out.push_str(&chars[begin..block_end].iter().collect::<String>());
            i = block_end;
            continue;
        }
        let line_start = if begin == 0 { 0 } else { line_start_before(&chars, begin - 1) };
        let prefix: String = chars[line_start..begin].iter().collect();
        let mut start = if prefix.contains('\\') || prefix.contains('=') { line_start } else { begin };
        if start == begin && prefix.trim().is_empty() && line_start > 0 {
            let previous_line_end = line_start - 1;
            let previous_line_start = if previous_line_end == 0 {
                0
            } else {
                line_start_before(&chars, previous_line_end - 1)
            };
            let previous_line: String = chars[previous_line_start..previous_line_end].iter().collect();
            if PREV_LINE_TAIL.is_match(&previous_line) {
                start = previous_line_start;
            }
        }
        out.push_str(&render_bare_math_lines(&chars[i..start].iter().collect::<String>()));
        let block: String = chars[start..block_end].iter().collect();
        out.push_str(&collapse_newlines(&latex_to_unicode(&block)));
        i = block_end;
    }
}

/// Find `needle` in `haystack` at or after `from`.
fn find_sub(haystack: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (from..=haystack.len() - needle.len()).find(|&at| &haystack[at..at + needle.len()] == needle)
}

// ---------------------------------------------------------------------------
// Math delimiter grammar
// ---------------------------------------------------------------------------

/// Opening delimiter. Each closer (`$`, `$$`, `\)`, `\]`) is as wide as its opener.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MathOpener {
    Dollar,
    DollarDollar,
    Paren,
    Bracket,
}

impl MathOpener {
    /// Width of the delimiter in code points.
    fn len(self) -> usize {
        match self {
            MathOpener::Dollar => 1,
            MathOpener::DollarDollar | MathOpener::Paren | MathOpener::Bracket => 2,
        }
    }
}

/// A closed math span found in the source.
struct MathSpan {
    /// Offset one past the closing delimiter.
    end: usize,
    /// Source between the delimiters, verbatim.
    body: String,
}

fn math_opener_at(source: &[char], at: usize) -> Option<MathOpener> {
    match source.get(at)? {
        '$' => {
            if source.get(at + 1) == Some(&'$') {
                Some(MathOpener::DollarDollar)
            } else {
                Some(MathOpener::Dollar)
            }
        }
        '\\' => match source.get(at + 1)? {
            '(' => Some(MathOpener::Paren),
            '[' => Some(MathOpener::Bracket),
            _ => None,
        },
        _ => None,
    }
}

/// An odd run of backslashes back to `from` escapes the delimiter at `index`.
fn escaped_at(source: &[char], index: usize, from: usize) -> bool {
    let mut backslashes = 0;
    let mut at = index;
    while at > from && source.get(at - 1) == Some(&'\\') {
        backslashes += 1;
        at -= 1;
    }
    backslashes % 2 == 1
}

/// Whether the `$` at `open` can open a span: it is followed by something other
/// than a space, tab, line break or `$`.
fn dollar_opens(source: &[char], open: usize) -> bool {
    match source.get(open + 1) {
        None => false,
        Some(' ') | Some('\t') | Some('\n') | Some('$') => false,
        Some(_) => true,
    }
}

/// Offset of the `$` that closes an inline span opened at `open`, or `-1`, and
/// where the scan stopped. Pandoc's anti-currency heuristics: the opener must
/// not be followed by whitespace, the closer must not be preceded by whitespace
/// nor followed by a digit, `\$` is a literal dollar, and the span may not
/// cross a newline — so "$5 and $10" is prose, not math.
fn scan_dollar(source: &[char], open: usize) -> (i64, usize) {
    if !dollar_opens(source, open) {
        return (-1, open + 1);
    }
    let mut at = open + 1;
    while at < source.len() {
        let ch = source[at];
        if ch == '\\' {
            at += 2;
            continue;
        }
        if ch == '\n' {
            return (-1, at);
        }
        if ch != '$' {
            at += 1;
            continue;
        }
        let before = if at >= 1 { source.get(at - 1).copied() } else { None };
        if before == Some(' ') || before == Some('\t') {
            return (-1, at);
        }
        if let Some(next) = source.get(at + 1).copied() {
            if next.is_ascii_digit() {
                at += 1;
                continue; // currency: keep scanning
            }
        }
        let body: String = source[open + 1..at].iter().collect();
        let close = if body.trim().is_empty() { -1 } else { at as i64 };
        return (close, at);
    }
    (-1, source.len())
}

/// Offset of the closer matching `opener`, or `-1`.
fn closer_index(source: &[char], opener: MathOpener, from: usize) -> i64 {
    let closer: &[char] = match opener {
        MathOpener::Paren => &['\\', ')'],
        MathOpener::Bracket => &['\\', ']'],
        MathOpener::Dollar => &['$'],
        MathOpener::DollarDollar => &['$', '$'],
    };
    let mut at = from;
    while at + closer.len() <= source.len() {
        if &source[at..at + closer.len()] == closer && !escaped_at(source, at, from) {
            return at as i64;
        }
        at += 1;
    }
    -1
}

/// The span opened at `at`, or `None` when the run is not math — including an
/// opener the source escaped, so `\$x$` and `\\(x\)` are literal text.
fn math_span_at(source: &[char], at: usize, from: usize) -> Option<MathSpan> {
    let opener = math_opener_at(source, at)?;
    if escaped_at(source, at, from) {
        return None;
    }
    let body_start = at + opener.len();
    let close_at = match opener {
        MathOpener::Dollar => scan_dollar(source, at).0,
        _ => closer_index(source, opener, body_start),
    };
    if close_at < 0 {
        return None;
    }
    let close_at = close_at as usize;
    let body: String = source[body_start..close_at].iter().collect();
    if opener == MathOpener::DollarDollar && body.trim().is_empty() {
        return None;
    }
    Some(MathSpan { end: close_at + opener.len(), body })
}

/// Scan prose for math spans — `$$…$$`, `\[…\]` (display) and `$…$`, `\(…\)`
/// (inline) — and replace each with its Unicode rendering, leaving everything
/// else verbatim. Newlines inside a span collapse to spaces so the result
/// stays single-line-safe.
///
/// Inline `$…$` uses pandoc's anti-currency heuristics: the opener must not be
/// followed by whitespace, the closer must not be preceded by whitespace nor
/// followed by a digit, and `\$` is treated as a literal dollar — so "$5 and
/// $10" is left untouched.
pub fn render_math_in_text(text: &str) -> String {
    if text.is_empty() {
        return text.to_string();
    }
    if !text.contains('$')
        && !text.contains("\\(")
        && !text.contains("\\[")
        && !text.contains("\\begin")
        && !BARE_MATH_LINE_COMMAND.is_match(text)
    {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let n = chars.len();
    while i < n {
        let c = chars[i];
        if c == '\\' {
            let d = chars.get(i + 1).copied();
            if d == Some('\\') {
                // Escaped backslash: emit verbatim so a following `(`/`[` is plain text.
                out.push_str("\\\\");
                i += 2;
                continue;
            }
            if d == Some('$') {
                out.push('$');
                i += 2;
                continue;
            }
            // `i` as the escape bound: the walk above already consumed `\\` and `\$`.
            if let Some(span) = math_span_at(&chars, i, i) {
                out.push_str(&collapse_newlines(&latex_to_unicode(&span.body)));
                i = span.end;
                continue;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if c == '$' {
            if let Some(span) = math_span_at(&chars, i, i) {
                out.push_str(&collapse_newlines(&latex_to_unicode(&span.body)));
                i = span.end;
                continue;
            }
            if chars.get(i + 1) == Some(&'$') {
                out.push_str("$$");
                i += 2;
                continue;
            }
            out.push('$');
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    render_bare_math_in_text(&out)
}

/// Render the math span opening at byte offset `at` in `text`, returning the
/// Unicode replacement and the byte offset just past the closing delimiter.
/// `None` when no span opens there (literal `$`, escaped delimiter, currency).
pub fn render_math_at(text: &str, at: usize) -> Option<(String, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let char_at = text[..at].chars().count();
    let span = math_span_at(&chars, char_at, char_at)?;
    let end = text
        .char_indices()
        .nth(span.end)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len());
    Some((collapse_newlines(&latex_to_unicode(&span.body)), end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_greek_and_operators() {
        assert_eq!(latex_to_unicode("\\alpha\\beta\\sum\\int"), "αβ∑∫");
        assert_eq!(latex_to_unicode("\\leq\\geq\\neq\\times\\cdot"), "≤≥≠×⋅");
        assert_eq!(latex_to_unicode("\\rightarrow\\leftarrow\\infty\\partial"), "→←∞∂");
    }

    #[test]
    fn converts_scripts_primes_and_fractions() {
        assert_eq!(latex_to_unicode("x^2 + y_1 = 0"), "x² + y₁ = 0");
        assert_eq!(latex_to_unicode("f''"), "f″");
        assert_eq!(latex_to_unicode("\\frac{1}{2}"), "½");
        assert_eq!(latex_to_unicode("\\frac{a+b}{c}"), "(a+b)/c");
        assert_eq!(latex_to_unicode("\\frac12"), "½");
    }

    fn fg(color: &str) -> String {
        let (r, g, b) = css_to_rgb(color).expect("known color");
        let (r, g, b) = (clamp_byte(r), clamp_byte(g), clamp_byte(b));
        format!("\x1b[38;2;{};{};{}m", r, g, b)
    }

    #[test]
    fn renders_ansi_colors() {
        assert_eq!(latex_to_unicode("\\textcolor{red}{x}"), format!("{}x{}", fg("#ff0000"), ANSI_FG_RESET));
        assert_eq!(
            latex_to_unicode("\\textcolor[HTML]{C5FFD6}{x}"),
            format!("{}x{}", fg("#C5FFD6"), ANSI_FG_RESET)
        );
        assert_eq!(
            latex_to_unicode("\\textcolor[RGB]{128,64,32}{x}"),
            format!("{}x{}", fg("rgb(128, 64, 32)"), ANSI_FG_RESET)
        );
        assert_eq!(
            latex_to_unicode("\\textcolor{red!50!blue}{x}"),
            format!("{}x{}", fg("rgb(128, 0, 128)"), ANSI_FG_RESET)
        );
    }

    #[test]
    fn keeps_color_scopes_in_brace_groups() {
        assert_eq!(
            latex_to_unicode("{\\color{red}x}y"),
            format!("{}x{}y", fg("#ff0000"), ANSI_FG_RESET)
        );
        assert_eq!(
            latex_to_unicode("{\\color{red}a {\\color{blue}b} c} d"),
            format!(
                "{}a {}b{} c{} d",
                fg("#ff0000"),
                fg("#0000ff"),
                fg("#ff0000"),
                ANSI_FG_RESET
            )
        );
    }

    #[test]
    fn renders_colorbox_and_fcolorbox() {
        let yellow_background = fg("#ffff00").replacen("\x1b[38;", "\x1b[48;", 1);
        assert_eq!(
            latex_to_unicode("\\colorbox{yellow}{x}"),
            format!("{}x{}", yellow_background, ANSI_BG_RESET)
        );
        assert_eq!(
            latex_to_unicode("\\fcolorbox{red}{yellow}{x}"),
            format!("{}[{}{}x{}{}]{}", fg("#ff0000"), ANSI_FG_RESET, yellow_background, ANSI_BG_RESET, fg("#ff0000"), ANSI_FG_RESET)
        );
    }

    #[test]
    fn keeps_synthetic_wrappers_in_caller_style() {
        assert_eq!(
            latex_to_unicode("\\textbf{\\frac{\\textit{ab}}{c}}"),
            "\x1b[1m(\x1b[3mab\x1b[23m)/c\x1b[22m"
        );
    }

    #[test]
    fn keeps_script_markers_in_script_style() {
        assert_eq!(
            latex_to_unicode("\\textit{x^{\\textcolor{red}{q}}}"),
            format!("\x1b[3mx^({}q{})\x1b[23m", fg("#ff0000"), ANSI_FG_RESET)
        );
    }

    #[test]
    fn restores_color_after_environment_body() {
        assert_eq!(
            latex_to_unicode("\\begin{pmatrix}\\color{red}a\\end{pmatrix}b"),
            format!("({}a{})b", fg("#ff0000"), ANSI_FG_RESET)
        );
    }

    #[test]
    fn resets_bold_and_italic_inside_textnormal() {
        assert_eq!(
            latex_to_unicode("\\textbf{\\textit{A\\textnormal{B}C}}"),
            "\x1b[1m\x1b[3mA\x1b[22m\x1b[23mB\x1b[1m\x1b[3mC\x1b[22m\x1b[23m"
        );
    }

    #[test]
    fn maps_text_styles_to_terminal_attributes() {
        for (command, on, off) in [
            ("textbf", "\x1b[1m", "\x1b[22m"),
            ("textit", "\x1b[3m", "\x1b[23m"),
            ("textsl", "\x1b[3m", "\x1b[23m"),
            ("emph", "\x1b[3m", "\x1b[23m"),
        ] {
            assert_eq!(latex_to_unicode(&format!("\\{command}{{x}}")), format!("{on}x{off}"));
        }
    }

    #[test]
    fn reopens_nested_terminal_styles() {
        let rendered = latex_to_unicode("\\textbf{outer \\textbf{inner} outer}");
        assert!(rendered.contains("\x1b[22m\x1b[1m outer"));
    }

    #[test]
    fn keeps_attributes_out_of_glyph_transforms() {
        assert_eq!(latex_to_unicode("\\hat{\\textbf{x}}"), "\x1b[1mx̂\x1b[22m");
        assert_eq!(latex_to_unicode("\\frac{\\textbf{a}}{b}"), "\x1b[1ma\x1b[22m/b");
        assert_eq!(latex_to_unicode("\\sqrt{\\textbf{x}}"), "√\x1b[1mx\x1b[22m");
    }

    #[test]
    fn keeps_wrapper_glyphs_at_caller_style() {
        assert_eq!(
            latex_to_unicode("\\boxed{\\textcolor{red}{x}}"),
            format!("[{}x{}]", fg("#ff0000"), ANSI_FG_RESET)
        );
        assert_eq!(latex_to_unicode("\\pmod{\\textbf{x}}"), "(mod \x1b[1mx\x1b[22m)");
    }

    #[test]
    fn keeps_fallback_script_glyphs_at_script_style() {
        assert_eq!(latex_to_unicode("x_{\\textit{word}}"), "x_(\x1b[3mword\x1b[23m)");
        assert_eq!(latex_to_unicode("x^{\\textbf{foo!}}"), "x^(\x1b[1mfoo!\x1b[22m)");
    }

    #[test]
    fn cancels_enclosing_style_for_nested_overrides() {
        assert_eq!(
            latex_to_unicode("\\textbf{A\\textmd{B}C}"),
            "\x1b[1mA\x1b[22mB\x1b[1mC\x1b[22m"
        );
        assert_eq!(
            latex_to_unicode("\\textit{A\\textup{B}C}"),
            "\x1b[3mA\x1b[23mB\x1b[3mC\x1b[23m"
        );
    }

    #[test]
    fn leaves_terminal_fallbacks_as_plain_text() {
        assert_eq!(latex_to_unicode("\\texttt{x}\\textsf{y}"), "xy");
    }

    #[test]
    fn maps_ams_relations_and_vulgar_fractions() {
        assert_eq!(latex_to_unicode("\\subseteqq"), "⫅");
        assert_eq!(latex_to_unicode("\\supseteqq"), "⫆");
        assert_eq!(latex_to_unicode("\\frac{1}{2}"), "½");
    }

    #[test]
    fn bare_math_environments() {
        for env in ["cases", "align", "align*", "pmatrix", "array", "gather", "equation*", "aligned"] {
            assert!(is_bare_math_environment(env), "{env}");
        }
        for env in ["tabular", "tabular*", "itemize", "enumerate", "verbatim", "document", "figure"] {
            assert!(!is_bare_math_environment(env), "{env}");
        }
    }

    #[test]
    fn render_math_in_text_converts_spans_only() {
        assert_eq!(render_math_in_text("see $x^2$ here"), "see x² here");
        assert_eq!(render_math_in_text("I paid \\$5 for it."), "I paid $5 for it.");
        assert_eq!(render_math_in_text("$5 and $10"), "$5 and $10");
        assert_eq!(render_math_in_text("no math at all"), "no math at all");
    }

    #[test]
    fn render_math_in_text_bare_environments() {
        let out = render_math_in_text("\\begin{align}\na &= b + c \\\\\nx &= y\n\\end{align}");
        assert!(!out.contains("\\begin{align}"));
        assert!(out.contains("="));
        let out = render_math_in_text("f(x) =\n\\begin{cases}\n1 & x > 0 \\\\\n0 & x \\le 0\n\\end{cases}");
        assert!(!out.contains("\\begin{cases}"));
        assert!(out.contains("f(x) ="));
        assert!(!out.contains("f(x) =\n"));
        let verbatim = "\\begin{verbatim}\n\\frac{a}{b}\n\\end{verbatim}";
        assert_eq!(render_math_in_text(verbatim), verbatim);
        let list = "Here:\n\\begin{itemize}\n\\item first\n\\item second\n\\end{itemize}\nDone.";
        assert_eq!(render_math_in_text(list), list);
    }

    #[test]
    fn converts_bare_color_command_lines() {
        let rendered = render_math_in_text("\\textcolor{red}{alert}");
        assert!(rendered.contains("alert"));
        assert!(rendered.contains(&fg("#ff0000")));
    }

    #[test]
    fn handles_malformed_input_without_panicking() {
        assert_eq!(latex_to_unicode(""), "");
        assert_eq!(latex_to_unicode("{{{"), "");
        assert_eq!(latex_to_unicode("\\"), "");
        assert_eq!(latex_to_unicode("\\frac{"), "/");
        assert_eq!(latex_to_unicode("\\unknowncommand"), "unknowncommand");
        assert_eq!(render_math_in_text(""), "");
    }
}
