//! Display-only formatting for eval code, ported from
//! `l/src/tools/eval-format/`.

pub mod javascript;
pub mod python;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::tools::eval_execution::EvalLanguage;

use self::javascript::format_javascript_for_display;
use self::python::format_python_for_display;

const DISPLAY_FORMAT_MEMO_CAPACITY: usize = 64;

type DisplayFormatMemo = Mutex<HashMap<String, String>>;

static JS_DISPLAY_MEMO: LazyLock<DisplayFormatMemo> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static PYTHON_DISPLAY_MEMO: LazyLock<DisplayFormatMemo> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn memoized_format(
    memo: &'static DisplayFormatMemo,
    source: &str,
    format: fn(&str) -> String,
) -> String {
    if let Ok(cache) = memo.lock() {
        if let Some(formatted) = cache.get(source) {
            return formatted.clone();
        }
    }
    let formatted = format(source);
    if let Ok(mut cache) = memo.lock() {
        if cache.len() >= DISPLAY_FORMAT_MEMO_CAPACITY {
            cache.clear();
        }
        cache.insert(source.to_string(), formatted.clone());
    }
    formatted
}

/// Formats an arbitrary eval-code prefix for display without changing the
/// executed source. `%`-prefixed magic passes through untouched.
pub fn format_eval_code_for_display(source: &str, language: EvalLanguage) -> String {
    if source.trim_start().starts_with('%') {
        return source.to_string();
    }
    match language {
        EvalLanguage::JavaScript => {
            memoized_format(&JS_DISPLAY_MEMO, source, format_javascript_for_display)
        }
        EvalLanguage::Python => {
            memoized_format(&PYTHON_DISPLAY_MEMO, source, format_python_for_display)
        }
    }
}
