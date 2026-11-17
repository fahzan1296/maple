//! Port of `l/test/eval-format-javascript-render.test.ts`.

use maple_tui::tools::eval_format::javascript::format_javascript_for_display;

#[test]
fn expands_compact_control_flow_and_keeps_short_objects_inline() {
    assert_eq!(
        format_javascript_for_display(
            "if (ready){const item = { value: 1 };use(item);}else{fallback();}"
        ),
        ["if (ready) {", "    const item = { value: 1 };", "    use(item);", "} else {", "    fallback();", "}"]
            .join("\n")
    );
    assert_eq!(
        format_javascript_for_display("const rows = [{ value: 1 },{ value: 2 }];"),
        "const rows = [{ value: 1 }, { value: 2 }];"
    );
}

#[test]
fn explodes_object_literals_that_exceed_the_display_width() {
    let source =
        "const box={left:rect.left+offset.left,right:rect.right+offset.right,top:rect.top+offset.top,bottom:rect.bottom+offset.bottom};";
    assert_eq!(
        format_javascript_for_display(source),
        [
            "const box = {",
            "    left: rect.left + offset.left,",
            "    right: rect.right + offset.right,",
            "    top: rect.top + offset.top,",
            "    bottom: rect.bottom + offset.bottom",
            "};",
        ]
        .join("\n")
    );
}

#[test]
fn normalizes_operator_spacing_and_keeps_for_loop_semicolons_inline() {
    assert_eq!(
        format_javascript_for_display("for(;;){tick();}for(let i=0;i<2;i++){work(i);}"),
        ["for (;;) {", "    tick();", "}", "for (let i = 0; i<2; i++) {", "    work(i);", "}"].join("\n")
    );
    // Angle brackets never get binary-operator spacing: generics would mangle.
    assert_eq!(
        format_javascript_for_display("const seen=new Map<string,number>();"),
        "const seen = new Map<string, number>();"
    );
}

#[test]
fn does_not_split_literals_templates_regexes_or_comments() {
    let double_quoted = r#""a\";{b}""#;
    let single_quoted = r"'a\';{b}'";
    let template = r#"`raw;{${fn({ value: "}" })}}`"#;
    let regex = "/[;{}]+/g";
    let line_comment = "// keep ; { }";
    let block_comment = "/* keep ; { } */";
    let source = format!(
        "const double = {};const single = {};const template = {};const regex = {}; {}\n{}done();",
        double_quoted, single_quoted, template, regex, line_comment, block_comment
    );

    assert_eq!(
        format_javascript_for_display(&source),
        [
            format!("const double = {};", double_quoted),
            format!("const single = {};", single_quoted),
            format!("const template = {};", template),
            format!("const regex = {}; {}", regex, line_comment),
            format!("{}done();", block_comment),
        ]
        .join("\n")
    );
}

#[test]
fn returns_unfinished_literals_comments_and_blocks_without_inventing_closers() {
    let samples = [
        ("const value = `raw;${call({ x: 1", "const value = `raw;${call({ x: 1"),
        ("/* unfinished ; {", "/* unfinished ; {"),
        ("// unfinished ; {", "// unfinished ; {"),
        ("const pattern = /[;{]", "const pattern = /[;{]"),
        ("if (ready){work();", "if (ready) {\n    work();"),
    ];

    for (source, expected) in samples {
        assert_eq!(format_javascript_for_display(source), expected);
    }
}

#[test]
fn is_idempotent() {
    let source = "try{const result = { ok: true };use(result);}catch(error){report(error);}finally{cleanup();}";
    let formatted = format_javascript_for_display(source);
    assert_eq!(format_javascript_for_display(&formatted), formatted);
}

#[test]
fn never_changes_committed_lines_while_a_prefix_grows() {
    let source = "if(flag){const value={text:`a;${item}`};run(value);}else{for(;;){tick();}}";
    let mut committed: Vec<&str> = Vec::new();

    for end in 1..=source.len() {
        let formatted = format_javascript_for_display(&source[..end]);
        let lines: Vec<&str> = formatted.split('\n').collect();
        let next_committed = &lines[..lines.len().saturating_sub(1)];
        assert_eq!(&next_committed[..committed.len().min(next_committed.len())], &committed[..]);
        committed = next_committed.to_vec();
    }
}

#[test]
fn tells_blocks_from_object_literals_after_class_function_and_switch_bodies() {
    assert_eq!(
        format_javascript_for_display("class A{m(){return {a:1}}}const o={b:2}"),
        ["class A {", "    m() {", "        return { a: 1 }", "    }", "}", "const o = { b: 2 }"].join("\n")
    );
    assert_eq!(
        format_javascript_for_display("function g(){switch(x){case 1:{y()}}}const z={k:[1,2]}"),
        [
            "function g() {",
            "    switch (x) {",
            "        case 1: { y() }",
            "    }",
            "}",
            "const z = { k: [1, 2] }",
        ]
        .join("\n")
    );
}