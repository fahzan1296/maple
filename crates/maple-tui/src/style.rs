//! Semantic colors and the style value applied to rendered text.

/// Terminal color: the 16 ANSI slots, 256-color palette, or RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
    Rgb(u8, u8, u8),
    Indexed(u8),
}

impl Color {
    /// SGR foreground parameter sequence.
    pub fn fg_sequence(self) -> String {
        match self {
            Color::Reset => "39".into(),
            Color::Black => "30".into(),
            Color::Red => "31".into(),
            Color::Green => "32".into(),
            Color::Yellow => "33".into(),
            Color::Blue => "34".into(),
            Color::Magenta => "35".into(),
            Color::Cyan => "36".into(),
            Color::Gray => "37".into(),
            Color::DarkGray => "90".into(),
            Color::LightRed => "91".into(),
            Color::LightGreen => "92".into(),
            Color::LightYellow => "93".into(),
            Color::LightBlue => "94".into(),
            Color::LightMagenta => "95".into(),
            Color::LightCyan => "96".into(),
            Color::White => "97".into(),
            Color::Rgb(r, g, b) => format!("38;2;{r};{g};{b}"),
            Color::Indexed(i) => format!("38;5;{i}"),
        }
    }

    /// SGR background parameter sequence.
    pub fn bg_sequence(self) -> String {
        match self {
            Color::Reset => "49".into(),
            Color::Black => "40".into(),
            Color::Red => "41".into(),
            Color::Green => "42".into(),
            Color::Yellow => "43".into(),
            Color::Blue => "44".into(),
            Color::Magenta => "45".into(),
            Color::Cyan => "46".into(),
            Color::Gray => "47".into(),
            Color::DarkGray => "100".into(),
            Color::LightRed => "101".into(),
            Color::LightGreen => "102".into(),
            Color::LightYellow => "103".into(),
            Color::LightBlue => "104".into(),
            Color::LightMagenta => "105".into(),
            Color::LightCyan => "106".into(),
            Color::White => "107".into(),
            Color::Rgb(r, g, b) => format!("48;2;{r};{g};{b}"),
            Color::Indexed(i) => format!("48;5;{i}"),
        }
    }
}

/// A complete SGR state. `None` color fields inherit the terminal default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub reverse: bool,
    pub hidden: bool,
}

impl Style {
    pub fn fg(color: Color) -> Self {
        Style {
            fg: Some(color),
            ..Style::default()
        }
    }

    pub fn bg(color: Color) -> Self {
        Style {
            bg: Some(color),
            ..Style::default()
        }
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    pub fn reverse(mut self) -> Self {
        self.reverse = true;
        self
    }

    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    /// True when no attribute is set.
    pub fn is_default(&self) -> bool {
        *self == Style::default()
    }

    /// Layer `over` on top of `base`: set fields win, unset fields fall through.
    pub fn merge(base: Style, over: Style) -> Style {
        Style {
            fg: over.fg.or(base.fg),
            bg: over.bg.or(base.bg),
            bold: base.bold || over.bold,
            dim: base.dim || over.dim,
            italic: base.italic || over.italic,
            underline: base.underline || over.underline,
            strikethrough: base.strikethrough || over.strikethrough,
            reverse: base.reverse || over.reverse,
            hidden: base.hidden || over.hidden,
        }
    }

    /// Wrap `text` in this style, closing with a full reset afterwards.
    pub fn paint(&self, text: &str) -> String {
        if self.is_default() || text.is_empty() {
            return text.to_string();
        }
        let mut params: Vec<String> = Vec::new();
        if let Some(fg) = self.fg {
            params.push(fg.fg_sequence());
        }
        if let Some(bg) = self.bg {
            params.push(bg.bg_sequence());
        }
        if self.bold {
            params.push("1".into());
        }
        if self.dim {
            params.push("2".into());
        }
        if self.italic {
            params.push("3".into());
        }
        if self.underline {
            params.push("4".into());
        }
        if self.strikethrough {
            params.push("9".into());
        }
        if self.reverse {
            params.push("7".into());
        }
        if self.hidden {
            params.push("8".into());
        }
        format!("\x1b[{}m{text}\x1b[0m", params.join(";"))
    }
}
