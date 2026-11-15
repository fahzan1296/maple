//! Glyph sets used by borders, tables, quotes, and spinners.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineSymbols {
    pub top_left: String,
    pub top_right: String,
    pub bottom_left: String,
    pub bottom_right: String,
    pub horizontal: String,
    pub vertical: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSymbols {
    pub top_left: String,
    pub top_right: String,
    pub bottom_left: String,
    pub bottom_right: String,
    pub horizontal: String,
    pub vertical: String,
    pub tee_down: String,
    pub tee_up: String,
    pub tee_left: String,
    pub tee_right: String,
    pub cross: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolTheme {
    pub box_round: LineSymbols,
    pub box_sharp: LineSymbols,
    pub table: TableSymbols,
    pub quote_border: String,
    pub hr_char: char,
    pub spinner_frames: Vec<String>,
}

impl Default for SymbolTheme {
    fn default() -> Self {
        Self::unicode()
    }
}

impl SymbolTheme {
    pub fn unicode() -> Self {
        SymbolTheme {
            box_round: LineSymbols {
                top_left: "╭".into(),
                top_right: "╮".into(),
                bottom_left: "╰".into(),
                bottom_right: "╯".into(),
                horizontal: "─".into(),
                vertical: "│".into(),
            },
            box_sharp: LineSymbols {
                top_left: "┌".into(),
                top_right: "┐".into(),
                bottom_left: "└".into(),
                bottom_right: "┘".into(),
                horizontal: "─".into(),
                vertical: "│".into(),
            },
            table: TableSymbols {
                top_left: "┌".into(),
                top_right: "┐".into(),
                bottom_left: "└".into(),
                bottom_right: "┘".into(),
                horizontal: "─".into(),
                vertical: "│".into(),
                tee_down: "┬".into(),
                tee_up: "┴".into(),
                tee_left: "├".into(),
                tee_right: "┤".into(),
                cross: "┼".into(),
            },
            quote_border: "│".into(),
            hr_char: '─',
            spinner_frames: default_spinner_frames(),
        }
    }

    pub fn ascii() -> Self {
        let mut theme = SymbolTheme::unicode();
        theme.box_round = LineSymbols {
            top_left: "+".into(),
            top_right: "+".into(),
            bottom_left: "+".into(),
            bottom_right: "+".into(),
            horizontal: "-".into(),
            vertical: "|".into(),
        };
        theme.box_sharp = theme.box_round.clone();
        theme.table = TableSymbols {
            top_left: "+".into(),
            top_right: "+".into(),
            bottom_left: "+".into(),
            bottom_right: "+".into(),
            horizontal: "-".into(),
            vertical: "|".into(),
            tee_down: "+".into(),
            tee_up: "+".into(),
            tee_left: "+".into(),
            tee_right: "+".into(),
            cross: "+".into(),
        };
        theme.quote_border = "|".into();
        theme.hr_char = '-';
        theme.spinner_frames = vec![
            "|".into(),
            "/".into(),
            "-".into(),
            "\\".into(),
        ];
        theme
    }

    pub fn set_spinner_frames(&mut self, frames: Vec<String>) {
        self.spinner_frames = frames;
    }
}

/// Braille frames, matching the classic CLI spinner.
pub fn default_spinner_frames() -> Vec<String> {
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
        .iter()
        .map(|frame| frame.to_string())
        .collect()
}
