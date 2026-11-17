//! Port of `l/test/thinking-display.test.ts`.

use maple_tui::chat::thinking_display::{
	canonicalize_message, format_thinking_for_display, reset_thinking_display_cache_for_tests,
};

#[test]
fn canonicalize_returns_empty_for_blank_input() {
	assert_eq!(canonicalize_message(None), "");
	assert_eq!(canonicalize_message(Some("")), "");
	assert_eq!(canonicalize_message(Some("   ")), "");
	assert_eq!(canonicalize_message(Some("\n\n")), "");
}

#[test]
fn canonicalize_returns_empty_for_dot_only_content() {
	assert_eq!(canonicalize_message(Some(".")), "");
	assert_eq!(canonicalize_message(Some("...")), "");
	assert_eq!(canonicalize_message(Some(" . ")), "");
	assert_eq!(canonicalize_message(Some("\n.")), "");
	assert_eq!(canonicalize_message(Some("…")), "");
}

#[test]
fn canonicalize_keeps_actual_prose() {
	assert_eq!(canonicalize_message(Some("hello")), "hello");
	assert_eq!(canonicalize_message(Some("hello.")), "hello.");
	assert_eq!(canonicalize_message(Some(". hello .")), ". hello .");
	assert_eq!(canonicalize_message(Some("a")), "a");
}

const STREAM_FIXTURES: [&str; 14] = [
	"hello",
	"**Headline**\n\nSome reasoning with `inline` code.\n\nAnother thought.",
	"line one\nline two\n\n\n",
	"```js\nconst x = 1;\nconsole.log(x);\n```\nafter fence",
	"```\nnever closed\nstill open",
	"intro\n```js\ncode\n```\ntail with trailing.\n",
	"Step 1.\n\n<!-- -->\nStep 2.\n",
	"Step.\n\n<!--\nmore",
	"a\nb\nc\n",
	"start ```\n```\nend",
	"```\n```",
	"~~~\ntilde fence\n~~~\n",
	"```\n\n```",
	"`x\n```y\nz",
];

fn assert_stream_fixture(fixture: &str, prose_only: bool, fixture_index: usize) {
	let n = fixture.len();
	let mut expected = Vec::with_capacity(n + 1);
	for i in 0..=n {
		reset_thinking_display_cache_for_tests();
		expected.push(format_thinking_for_display(&fixture[..i], prose_only));
	}
	for i in 1..=n {
		reset_thinking_display_cache_for_tests();
		for j in 1..=i {
			assert_eq!(
				format_thinking_for_display(&fixture[..j], prose_only),
				expected[j],
				"fixture {fixture_index} mode {} split {j}/{i}",
				if prose_only { "prose" } else { "raw" }
			);
		}
	}
}

#[test]
fn streaming_output_matches_cold_recompute_at_every_split_point_raw() {
	for (index, fixture) in STREAM_FIXTURES.iter().enumerate() {
		assert_stream_fixture(fixture, false, index);
	}
}

#[test]
fn streaming_output_matches_cold_recompute_at_every_split_point_prose() {
	for (index, fixture) in STREAM_FIXTURES.iter().enumerate() {
		assert_stream_fixture(fixture, true, index);
	}
}

const CHUNKS: [&str; 10] = [
	"step ",
	"through the ",
	"plan:\n",
	"```js\n",
	"value += 1;\n",
	"```\n",
	"next idea\n",
	"with detail ",
	"and more.\n",
	"<!-- note\n",
];
const TICKS: usize = 5000;

fn streamed_texts() -> Vec<String> {
	let mut parts = String::new();
	let mut texts = Vec::with_capacity(TICKS);
	for i in 0..TICKS {
		parts.push_str(CHUNKS[i % CHUNKS.len()]);
		texts.push(parts.clone());
	}
	texts
}

#[test]
fn append_stream_stays_byte_identical_to_a_cold_recompute() {
	let texts = streamed_texts();
	for prose_only in [false, true] {
		let mut last_out = String::new();
		for text in &texts {
			last_out = format_thinking_for_display(text, prose_only);
		}
		reset_thinking_display_cache_for_tests();
		assert_eq!(
			last_out,
			format_thinking_for_display(&texts[TICKS - 1], prose_only)
		);
	}
}

#[test]
fn exact_repeats_keep_returning_the_identical_display() {
	let texts = streamed_texts();
	let final_text = &texts[TICKS - 1];
	for prose_only in [false, true] {
		let out = format_thinking_for_display(final_text, prose_only);
		for _ in 0..2000 {
			assert_eq!(format_thinking_for_display(final_text, prose_only), out);
		}
	}
}

const STREAMS: [&[&str]; 5] = [
	&["Plan:\n", "check the ", "fence\n", "```ts\n", "let x = 1;\n", "```\n", "done.\n", "<!-- -->\n"],
	&["Plan:", "\n", "other ", "branch\n", "<!--", " -->\n", "~~~\n", "tilde\n"],
	&["plain ", "comment-free ", "raw text\n", "```\n", "code\n", "```\n", "more\n", "end"],
	&["<!--\n", "note\n", "**Head**\n", "\n", "<!-- -->\n", "body ", "text.\n", "tail"],
	&["x", "y", "z\n", "```\n", "a\n", "b\n", "```\n", "."],
];

#[test]
fn interleaved_blocks_match_cold_recompute() {
	for prose_only in [false, true] {
		let prefixes: Vec<Vec<String>> = STREAMS
			.iter()
			.map(|chunks| {
				(1..=chunks.len())
					.map(|i| chunks[..i].concat())
					.collect()
			})
			.collect();
		let cold: Vec<Vec<String>> = prefixes
			.iter()
			.map(|texts| {
				texts
					.iter()
					.map(|text| {
						reset_thinking_display_cache_for_tests();
						format_thinking_for_display(text, prose_only)
					})
					.collect()
			})
			.collect();
		reset_thinking_display_cache_for_tests();
		for tick in 0..8 {
			for (stream, chunks) in prefixes.iter().enumerate() {
				let text = &chunks[tick];
				assert_eq!(
					format_thinking_for_display(text, prose_only),
					cold[stream][tick]
				);
				assert_eq!(
					format_thinking_for_display(text, prose_only),
					cold[stream][tick]
				);
			}
		}
	}
}

fn mutate_byte(seed: &str, position: usize) -> String {
	let bytes = seed.as_bytes();
	let replacement = if bytes[position] == b'Z' { b'Q' } else { b'Z' };
	let mut mutant = String::with_capacity(seed.len() + 5);
	mutant.push_str(&seed[..position]);
	mutant.push(replacement as char);
	mutant.push_str(&seed[position + 1..]);
	mutant.push_str(" more");
	mutant
}

#[test]
fn gap_divergent_mutations_match_cold_recompute_raw() {
	let seed = format!("I{}\n<!-- -->\nplain tail", "B".repeat(40));
	let seam = 42;
	for position in 1..seam {
		let mutant = mutate_byte(&seed, position);
		reset_thinking_display_cache_for_tests();
		let cold = format_thinking_for_display(&mutant, false);
		reset_thinking_display_cache_for_tests();
		format_thinking_for_display(&seed, false);
		assert_eq!(format_thinking_for_display(&mutant, false), cold, "position {position}");
	}
}

#[test]
fn gap_divergent_mutations_match_cold_recompute_prose() {
	let seed = format!("I{}\n```\ncode\nX", "B".repeat(40));
	let seam = 42;
	for position in 1..seam {
		let mutant = mutate_byte(&seed, position);
		reset_thinking_display_cache_for_tests();
		let cold = format_thinking_for_display(&mutant, true);
		reset_thinking_display_cache_for_tests();
		format_thinking_for_display(&seed, true);
		assert_eq!(format_thinking_for_display(&mutant, true), cold, "position {position}");
	}
}

#[test]
fn raw_identity_shortcut_records_the_slot_without_a_resumable_checkpoint() {
	let base = "intro\n```\ncode line";
	let grown = format!("{base}\n<!-- -->");
	reset_thinking_display_cache_for_tests();
	let cold_grown = format_thinking_for_display(&grown, false);
	assert!(cold_grown.contains("<!-- -->"));
	reset_thinking_display_cache_for_tests();
	format_thinking_for_display(base, false);
	assert_eq!(format_thinking_for_display(&grown, false), cold_grown);

	reset_thinking_display_cache_for_tests();
	let once = format_thinking_for_display(base, false);
	assert_eq!(format_thinking_for_display(base, false), once);
}

fn seam_transition_battery_matches_cold_recompute_raw() {
	let fixture = format!(
		"{}\n```js\nstep one\nstep two\n```\ntail prose.\n<!-- -->\nappended",
		"x".repeat(8300)
	);
	let n = fixture.len();
	let mut refs = Vec::with_capacity(n + 1);
	for i in 0..=n {
		reset_thinking_display_cache_for_tests();
		refs.push(format_thinking_for_display(&fixture[..i], false));
	}
	reset_thinking_display_cache_for_tests();
	for i in 1..=n {
		assert_eq!(
			format_thinking_for_display(&fixture[..i], false),
			refs[i],
			"split {i}"
		);
	}
}

#[test]
fn seam_transition_battery_matches_cold_recompute_prose() {
	let fixture = format!(
		"{}\n```js\nstep one\nstep two\n```\ntail prose.\n<!-- -->\nappended",
		"x".repeat(8300)
	);
	let n = fixture.len();
	let mut refs = Vec::with_capacity(n + 1);
	for i in 0..=n {
		reset_thinking_display_cache_for_tests();
		refs.push(format_thinking_for_display(&fixture[..i], true));
	}
	reset_thinking_display_cache_for_tests();
	for i in 1..=n {
		assert_eq!(
			format_thinking_for_display(&fixture[..i], true),
			refs[i],
			"split {i}"
		);
	}
}

const NL_FREE_CHUNKS: [&str; 4] = ["word ", "token ", "and ", "more "];

fn build_texts(ticks: usize, chunk: Option<&str>) -> Vec<String> {
	let mut parts = String::new();
	let mut texts = Vec::with_capacity(ticks);
	for i in 0..ticks {
		parts.push_str(chunk.unwrap_or(NL_FREE_CHUNKS[i % NL_FREE_CHUNKS.len()]));
		texts.push(parts.clone());
	}
	texts
}

#[test]
fn prose_growth_through_the_resume_cap_matches_a_cold_recompute() {
	let specs: [(usize, Option<String>); 2] = [(2400, None), (1200, Some("z".repeat(16)))];
	for (ticks, chunk) in specs {
		let texts = build_texts(ticks, chunk.as_deref());
		let mut last_out = String::new();
		for text in &texts {
			last_out = format_thinking_for_display(text, true);
		}
		reset_thinking_display_cache_for_tests();
		assert_eq!(
			last_out,
			format_thinking_for_display(texts.last().expect("non-empty"), true)
		);
	}
}

#[test]
fn repeats_on_the_post_cap_text_answer_from_the_memo() {
	let texts = build_texts(2400, None);
	let final_text = texts.last().expect("non-empty");
	let mut out = String::new();
	for text in &texts {
		out = format_thinking_for_display(text, true);
	}
	reset_thinking_display_cache_for_tests();
	assert_eq!(out, format_thinking_for_display(final_text, true));
}

const FRAGMENTS: [&str; 13] = [
	"word ",
	"plan:\n",
	"```js\n",
	"code;\n",
	"```\n",
	"<!-- -->\n",
	"<!--\n",
	" -->\n",
	"tail.\n",
	"~~~\n",
	"`tick` ",
	"> quote\n",
	"\n",
];
const FUZZ_NL_FREE: [&str; 8] = ["word ", "token ", "and ", "-->", "<!--", "```", " . ", "Z"];

#[test]
fn differential_fuzz_matches_cold_recompute() {
	let mut state: u32 = 0x9e37_79b9;
	let mut rnd = move || {
		state ^= state << 13;
		state ^= state >> 17;
		state ^= state << 5;
		f64::from(state) / 4_294_967_296.0
	};
	let pick = |items: &[&str], rnd: &mut dyn FnMut() -> f64| -> String {
		items[(rnd() * items.len() as f64).floor() as usize].to_string()
	};

	let mut checks = 0usize;
	let mut failures: Vec<String> = Vec::new();
	for trial in 0..220 {
		for prose_only in [false, true] {
			let no_newline = trial % 3 == 0;
			let mut text = String::new();
			for step in 0..14 {
				let prev = text.clone();
				let roll = rnd();
				if prev.is_empty() || roll < 0.55 {
					let fragment = if no_newline && rnd() < 0.5 {
						"z".repeat(600 + (rnd() * 900.0).floor() as usize)
					} else if no_newline {
						pick(&FUZZ_NL_FREE, &mut rnd)
					} else {
						pick(&FRAGMENTS, &mut rnd)
					};
					text = prev.clone() + fragment.as_str();
				} else if roll < 0.85 && prev.len() > 2 {
					let seam = prev.rfind('\n').map_or(0, |index| index + 1);
					let limit = seam.min(prev.len()).saturating_sub(33).max(1);
					let position = 1 + (rnd() * limit as f64).floor() as usize;
					if position >= prev.len() {
						text = prev.clone() + pick(&FRAGMENTS, &mut rnd).as_str();
					} else {
						let bytes = prev.as_bytes();
						let replacement = if bytes[position] == b'Z' { b'Q' } else { b'Z' };
						text = format!(
							"{}{}{}",
							&prev[..position],
							replacement as char,
							&prev[position + 1..]
						);
					}
				} else if roll < 0.93 && prev.len() > 4 {
					text = prev[..1 + (rnd() * (prev.len() - 1) as f64).floor() as usize].to_string();
				} else {
					text = pick(&["", "```", "<!--"], &mut rnd);
				}
				if text.is_empty() {
					continue;
				}

				let actual = format_thinking_for_display(&text, prose_only);
				reset_thinking_display_cache_for_tests();
				let expected = format_thinking_for_display(&text, prose_only);
				checks += 1;
				if actual != expected && failures.len() < 5 {
					failures.push(format!(
						"trial={trial} mode={} step={step} len={}",
						if prose_only { "prose" } else { "raw" },
						text.len()
					));
				}
			}
		}
	}
	assert!(failures.is_empty(), "failures: {failures:?}");
	assert!(checks > 4000, "only {checks} checks");
}