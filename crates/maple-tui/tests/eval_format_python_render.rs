//! Port of `l/test/eval-format-python-render.test.ts`.

use maple_tui::tools::eval_format::python::format_python_for_display;

const COMPACT: &str =
    "class Classifier:def classify(self,value):if value>0:return \"positive\";elif value<0:return \"negative\";else:return \"zero\"";

const EXPANDED_COMPACT: &str = concat!(
    "class Classifier:\n",
    "    def classify(self,value):\n",
    "        if value>0:\n",
    "            return \"positive\";\n",
    "        elif value<0:\n",
    "            return \"negative\";\n",
    "        else:\n",
    "            return \"zero\"",
);

const LEXICAL_SAFETY_SOURCE: &str = concat!(
    "if ready:# keep ; and : exactly\n",
    "    text = \"semi; escaped \\\" quote\"; result = call([1; 2], {\"k;\": 3}) # tail ; :\n",
    "else:# other\n",
    "    result = None",
);

const LEXICAL_SAFETY_EXPECTED: &str = concat!(
    "if ready:# keep ; and : exactly\n",
    "    text = \"semi; escaped \\\" quote\";\n",
    "    result = call([1; 2], {\"k;\": 3}) # tail ; :\n",
    "else:# other\n",
    "    result = None",
);

#[test]
fn expands_genuinely_compact_nested_suites_and_compound_clauses() {
    assert_eq!(format_python_for_display(COMPACT), EXPANDED_COMPACT);
}

#[test]
fn only_splits_top_level_semicolons_and_leaves_strings_delimiters_and_comments_intact() {
    assert_eq!(format_python_for_display(LEXICAL_SAFETY_SOURCE), LEXICAL_SAFETY_EXPECTED);
}

#[test]
fn keeps_unfinished_strings_and_headers_conservative() {
    let unfinished = "def build():value = \"\"\"open;:#\nstill open";
    assert_eq!(
        format_python_for_display(unfinished),
        "def build():\n    value = \"\"\"open;:#\nstill open"
    );
    assert_eq!(format_python_for_display("while waiting:"), "while waiting:");
}

#[test]
fn is_idempotent_for_compact_readable_and_incomplete_prefixes() {
    let readable = concat!(
        "def outer(value):\n",
        "    if value:\n",
        "        return {\"items\": [value; 2]}\n",
        "    else:\n",
        "        return None",
    );
    let unfinished = "def build():value = \"\"\"open;:#\nstill open";

    for source in [COMPACT, LEXICAL_SAFETY_SOURCE, readable, unfinished, "if pending:"] {
        let formatted = format_python_for_display(source);
        assert_eq!(format_python_for_display(&formatted), formatted);
    }
}

#[test]
fn never_changes_lines_committed_by_an_earlier_sequential_prefix() {
    let source = concat!(
        "async def choose(values):if values:item=\"a;b\";elif fallback:item=call([1;2]);",
        "else:item=\"\"\"open\nstill",
    );
    let expected = concat!(
        "async def choose(values):\n",
        "    if values:\n",
        "        item=\"a;b\";\n",
        "    elif fallback:\n",
        "        item=call([1;2]);\n",
        "    else:\n",
        "        item=\"\"\"open\n",
        "still",
    );
    let mut committed: Vec<&str> = Vec::new();

    for end in 0..=source.len() {
        let formatted = format_python_for_display(&source[..end]);
        let lines: Vec<&str> = formatted.split('\n').collect();
        let next_committed = &lines[..lines.len().saturating_sub(1)];
        let compared = committed.len().min(next_committed.len());
        assert_eq!(&next_committed[..compared], &committed[..compared]);
        committed = next_committed.to_vec();
    }
    assert_eq!(format_python_for_display(source), expected);
}