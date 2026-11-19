use maple_tui::chat::image_loading::{
    cached_png_conversion, convert_image_to_png_shared, ensure_supported_image_input,
    image_payload_key,
};
use maple_tui::chat::messages::ImageContent;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn image(data: &str) -> ImageContent {
    ImageContent {
        data: data.to_string(),
        mime_type: "image/jpeg".to_string(),
        detail: None,
        url: None,
    }
}

#[test]
fn converts_each_payload_once_across_repeated_callers() {
    let converts = Arc::new(AtomicUsize::new(0));
    let counter = converts.clone();
    let encoder = move |bytes: &[u8]| {
        assert_eq!(bytes, &[1, 2, 3]);
        counter.fetch_add(1, Ordering::SeqCst);
        Some(vec![9, 9, 9])
    };
    let source = image("AQID");

    let first = convert_image_to_png_shared(&source, &encoder).expect("first conversion");
    let second = convert_image_to_png_shared(&source, &encoder).expect("cached conversion");

    assert_eq!(converts.load(Ordering::SeqCst), 1);
    assert_eq!(first.mime_type, "image/png");
    assert_eq!(first.data, "CQkJ");
    assert_eq!(first, second);
}

#[test]
fn failed_conversions_are_not_cached() {
    let attempts = Arc::new(Mutex::new(vec![false, true]));
    let encoder = move |bytes: &[u8]| {
        let mut plan = attempts.lock().unwrap();
        if plan.is_empty() {
            return None;
        }
        if plan.remove(0) {
            Some(bytes.to_vec())
        } else {
            None
        }
    };
    let source = image("BAUG");

    assert_eq!(convert_image_to_png_shared(&source, &encoder), None);
    let converted = convert_image_to_png_shared(&source, &encoder).expect("retry converts");
    assert_eq!(converted.mime_type, "image/png");
    assert_eq!(converted.data, "BAUG");
}

#[test]
fn cached_conversion_is_retrievable_and_keyed_by_payload() {
    let converts = Arc::new(AtomicUsize::new(0));
    let counter = converts.clone();
    let encoder = move |bytes: &[u8]| {
        counter.fetch_add(1, Ordering::SeqCst);
        Some(bytes.to_vec())
    };
    let source = image("BwgJ");

    let converted = convert_image_to_png_shared(&source, &encoder).expect("conversion");
    assert_eq!(cached_png_conversion(&source), Some(converted));

    let mut rebuilt = source.clone();
    rebuilt.url = Some("https://example.com/mirror.png".to_string());
    rebuilt.detail = Some("high".to_string());
    assert_eq!(image_payload_key(&source), image_payload_key(&rebuilt));
    assert_eq!(converts.load(Ordering::SeqCst), 1);
}

#[test]
fn supported_input_types_pass_through_unchanged() {
    let source = ImageContent {
        data: "AQID".to_string(),
        mime_type: "image/png".to_string(),
        detail: None,
        url: None,
    };
    let converts = Arc::new(AtomicUsize::new(0));
    let counter = converts.clone();
    let encoder = move |bytes: &[u8]| {
        counter.fetch_add(1, Ordering::SeqCst);
        Some(bytes.to_vec())
    };
    let ensured = ensure_supported_image_input(&source, &encoder).expect("supported");
    assert_eq!(ensured, source);
    assert_eq!(converts.load(Ordering::SeqCst), 0);
}
