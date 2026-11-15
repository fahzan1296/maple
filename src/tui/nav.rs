//! Navigation vocabulary: separators, chips, and key hints.

/// Metric separator used by omp (`cost · req · tools · tokens · age`).
pub const SEP_DOT: &str = " · ";

/// Footer chips shown in the hub strip.
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
    "j/k select · t tree · / filter · Esc close · q quit"
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
