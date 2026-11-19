use maple_tui::render::sixel::{
    contains_sixel_sequence, get_sixel_line_mask, is_xtsmgraphics_reply,
    parse_xtsmgraphics_reply, sanitize_with_sixel_passthrough, SixelProbe, XTSMGRAPHICS_QUERY,
};

#[test]
fn query_asks_xtsmgraphics_then_da1_sentinel() {
    assert!(XTSMGRAPHICS_QUERY.contains("\x1b[?2;1;0S"));
    assert!(XTSMGRAPHICS_QUERY.contains("\x1b[c"));
    let mut probe = SixelProbe::new();
    assert!(!probe.supported());
    assert!(!probe.da1_seen());
}

#[test]
fn supported_reply_reports_maximum_geometry() {
    let reply = "\x1b[?2;0;1692;432S";
    assert!(is_xtsmgraphics_reply(reply));
    assert_eq!(parse_xtsmgraphics_reply(reply), Some((1692, 432)));

    let mut probe = SixelProbe::new();
    assert!(probe.feed(reply));
    assert!(probe.supported());
    assert_eq!(probe.max(), Some((1692, 432)));
    probe.stop();
    assert!(!probe.supported());
    assert_eq!(probe.max(), None);
}

#[test]
fn da1_only_reply_resolves_as_unsupported() {
    let mut probe = SixelProbe::new();
    assert!(probe.feed("\x1b[?62;c"));
    assert!(probe.da1_seen());
    assert!(!probe.supported());
    assert!(!probe.feed("plain text"));
}

#[test]
fn non_zero_status_is_unsupported() {
    assert_eq!(parse_xtsmgraphics_reply("\x1b[?2;1;0;0S"), None);
    let mut probe = SixelProbe::new();
    assert!(probe.feed("\x1b[?2;1;0;0S"));
    assert!(!probe.supported());
}

#[test]
fn multi_line_sixel_blocks_are_masked() {
    let lines = vec![
        "before".to_string(),
        "\x1bPqpayload".to_string(),
        "middle".to_string(),
        "\x1b\\after".to_string(),
    ];
    assert_eq!(
        get_sixel_line_mask(&lines),
        vec![false, true, true, true]
    );
    assert!(contains_sixel_sequence(&lines[1]));
}

#[test]
fn passthrough_preserves_complete_sequences_only() {
    let sequence = "\x1bPqabc\x1b\\";
    let text = format!("keep {sequence} end");
    let strip = |input: &str| input.replace('\x1b', "").replace('\x07', "");

    let preserved = sanitize_with_sixel_passthrough(text.as_str(), true, strip);
    assert!(preserved.contains(sequence), "preserved: {preserved:?}");
    assert!(preserved.contains("keep"));

    let stripped = sanitize_with_sixel_passthrough(text.as_str(), false, strip);
    assert!(!stripped.contains(sequence));
    assert!(stripped.contains("keep"));
    assert!(stripped.contains("end"));
}
