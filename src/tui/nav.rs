//! Navigation vocabulary: separators, chips, and key hints.

/// Metric separator used by omp (`cost · req · tools · tokens · age`).
pub const SEP_DOT: &str = " · ";

/// Hub tabs: roster list and the recent-entry feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Agents,
    Activity,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Tab::Agents => CHIP_AGENTS,
            Tab::Activity => CHIP_ACTIVITY,
        }
    }

    pub fn other(self) -> Self {
        match self {
            Tab::Agents => Tab::Activity,
            Tab::Activity => Tab::Agents,
        }
    }
}

pub const CHIP_AGENTS: &str = "Agents";
pub const CHIP_ACTIVITY: &str = "Activity";

pub fn chip(label: &str, selected: bool) -> String {
    if selected {
        format!("[ {label} ]")
    } else {
        format!("  {label}  ")
    }
}

pub fn hub_hints() -> &'static str {
    "j/k select · t tree · a ask · Tab activity · / filter · Esc close · q quit"
}

pub fn input_hints() -> &'static str {
    "Enter send · Esc cancel · / command"
}

pub fn normal_hints() -> &'static str {
    "i edit · h hub · / command · q quit"
}

pub fn picker_hints() -> &'static str {
    "type to filter · Enter resume · Esc cancel"
}

pub fn ask_hints() -> &'static str {
    "Enter send · Esc cancel"
}
