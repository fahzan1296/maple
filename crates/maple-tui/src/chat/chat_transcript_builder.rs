//! Rebuilds transcript components from persisted session entries — the
//! file/remote-backed counterpart to the live chat append path.
//!
//! Reference: `l/src/chat/chat-transcript-builder.ts`.
//!
//! Unlike incremental transcript sync, [`ChatTranscriptBuilder::rebuild`]
//! discards prior components and rebuilds the whole transcript from the
//! supplied entries, so a growing transcript costs O(n) but cannot duplicate
//! or misorder rows.
//!
//! Deviations:
//! - The crate's container exposes no mutable child access, so a call whose
//!   result has not arrived is held outside the container and inserted at its
//!   recorded index when the result does arrive. A call still unresolved when
//!   the pass flushes is materialized without a result.
//! - The transcript-wide expansion toggle applies to blocks built afterwards:
//!   already-inserted blocks cannot take it.
//! - Late diagnostics always draw standalone; they are not routed back into
//!   edit/write frames.
//! - Tool aliases come from [`ChatTranscriptBuilderDeps::tool_aliases`]; there
//!   is no live tool registry here.
//! - `row_for_entry` returns the block's container index: rows are only known
//!   after a paint, and the crate maps rows by child pointer.
//! - IRC, parent-IRC, file-mention and photo/table-chart extension cards are
//!   not modeled by [`AgentMessage`]; they fall through to custom messages.
//! - No served-model tracker: the builder takes no provider identity.

use std::collections::HashMap;

use serde_json::Value;

use crate::chat::advisor_message::AdvisorMessageCard;
use crate::chat::assistant_message::AssistantMessageComponent;
use crate::chat::background_tan_message::create_background_tan_dispatch_block;
use crate::chat::bash_execution::{BashCompletion, BashExecutionComponent};
use crate::chat::cache_invalidation_marker::detect_cache_invalidation;
use crate::chat::collab_prompt_message::CollabPromptMessage;
use crate::chat::compaction_summary_message::create_handoff_summary_message_component;
use crate::chat::custom_message::CustomMessageComponent;
use crate::chat::eval_execution::{EvalCompletion, EvalExecutionComponent, EvalExecutionLanguage};
use crate::chat::late_diagnostics_message::{LateDiagnosticsFile, LateDiagnosticsMessage};
use crate::tools::output_meta::{MinimizedMeta, TruncationMeta};
use crate::chat::messages::{
	is_user_turn_initiator, AdvisorMessageDetails, AdvisorNote, AdvisorSeverity, CustomMessage,
	BACKGROUND_TAN_DISPATCH_MESSAGE_TYPE, COLLAB_PROMPT_MESSAGE_TYPE,
	LSP_LATE_DIAGNOSTIC_MESSAGE_TYPE, SKILL_PROMPT_MESSAGE_TYPE,
};
use crate::chat::read_tool_group::{
	grouped_read_usage_call_ids, read_args_collapse_into_group, ReadToolGroupComponent,
	ReadToolGroupOptions,
};
use crate::chat::reply_write::{is_reply_write, ReplyWriteComponent, ReplyWriteOptions};
use crate::chat::skill_message::SkillMessageComponent;
use crate::chat::tool_execution::{
	tool_render_name, ToolExecutionComponent, ToolExecutionHandle, ToolExecutionOptions,
	ToolResultBlock, ToolResultFrame,
};
use crate::chat::transcript_entry::{transcript_entry_message, AgentMessage, TranscriptEntryLike};
use crate::chat::transcript_render_helpers::{
	assistant_has_visible_content, assistant_usage_is_billed, build_async_result_block,
	build_launch_completion_block, split_assistant_message_tool_timeline, AsyncResultDetails,
};
use crate::chat::user_message::{UserBubbleOptions, UserMessageComponent};
use crate::chrome::transcript_container::TranscriptContainer;
use crate::component::Component;
use crate::components::text::Text;
use crate::overlays::usage_row::{
	format_usage_row, turn_elapsed_ms, TurnUsageTally, Usage,
};
use crate::theme::theme_class::Theme;
use crate::tools::daemon::{DaemonSnapshot, DaemonState};

/// What [`ChatTranscriptBuilder`] needs from its host.
pub struct ChatTranscriptBuilderDeps {
	/// Theme every block draws with.
	pub theme: Theme,
	/// Working directory shown in tool cards.
	pub cwd: String,
	/// Session-scoped resolved destinations for model-authored links.
	pub link_targets: HashMap<String, String>,
	/// Hide finished thinking blocks.
	pub hide_thinking_block: bool,
	/// Render only prose-shaped reasoning.
	pub prose_only_thinking: bool,
	/// Keep finished thinking blocks expanded.
	pub expand_thinking_blocks: bool,
	/// Draw charts under numeric tables.
	pub table_charts: bool,
	/// Draw images when the terminal can.
	pub show_images: bool,
	/// Hide tool activity.
	pub hide_tool_activity: bool,
	/// Show content previews under read rows.
	pub read_tool_result_preview: bool,
	/// Show the per-turn usage row.
	pub show_token_usage: bool,
	/// Show prompt→yield wall time in the usage row.
	pub show_turn_time: bool,
	/// Mark a turn that lost its prompt cache.
	pub cache_miss_marker: bool,
	/// Wire name → canonical tool name.
	pub tool_aliases: HashMap<String, String>,
}

impl ChatTranscriptBuilderDeps {
	/// Defaults with `theme`, except `cwd` and the alias table.
	pub fn new(theme: Theme) -> Self {
		ChatTranscriptBuilderDeps {
			theme,
			cwd: String::new(),
			link_targets: HashMap::new(),
			hide_thinking_block: false,
			prose_only_thinking: true,
			expand_thinking_blocks: false,
			table_charts: true,
			show_images: true,
			hide_tool_activity: false,
			read_tool_result_preview: true,
			show_token_usage: true,
			show_turn_time: true,
			cache_miss_marker: true,
			tool_aliases: HashMap::new(),
		}
	}

	fn render_name(&self, wire_name: &str) -> String {
		tool_render_name(wire_name, self.tool_aliases.get(wire_name).map(String::as_str))
	}
}

/// A call awaiting its result.
struct PendingCall {
	tool_name: String,
	args: Option<Value>,
	tool_call_id: String,
	index: usize,
}

/// An open read run.
struct PendingReads {
	entries: Vec<(String, Option<Value>, Option<(String, Option<Value>, bool)>)>,
	index: usize,
	usage_ids: Vec<String>,
}

/// Deferred per-turn metrics.
struct PendingUsage {
	usage: crate::chat::messages::Usage,
	duration_ms: Option<i64>,
	ttft_ms: Option<i64>,
	timestamp: Option<i64>,
	turn_elapsed: Option<i64>,
}

/// Builds transcript components from persisted entries.
pub struct ChatTranscriptBuilder {
	deps: ChatTranscriptBuilderDeps,
	/// The transcript being built.
	pub container: TranscriptContainer,
	blocks: usize,
	pending_calls: Vec<PendingCall>,
	pending_reads: Option<PendingReads>,
	pending_usage: Option<PendingUsage>,
	turn_started_at: Option<i64>,
	turn_usage: TurnUsageTally,
	last_assistant_usage: Option<crate::chat::messages::Usage>,
	entry_blocks: HashMap<String, Vec<usize>>,
	expanded: bool,
}

impl ChatTranscriptBuilder {
	/// A builder drawing in `deps`.
	pub fn new(deps: ChatTranscriptBuilderDeps) -> Self {
		let mut builder = ChatTranscriptBuilder {
			deps,
			container: TranscriptContainer::new(),
			blocks: 0,
			pending_calls: Vec::new(),
			pending_reads: None,
			pending_usage: None,
			turn_started_at: None,
			turn_usage: TurnUsageTally::new(),
			last_assistant_usage: None,
			entry_blocks: HashMap::new(),
			expanded: false,
		};
		builder
			.container
			.set_tool_activity_visible(!builder.deps.hide_tool_activity);
		builder
	}

	/// Whether the transcript holds any rendered rows.
	pub fn is_empty(&self) -> bool {
		self.blocks == 0
	}

	/// Discard every block and rebuild from `entries`.
	pub fn rebuild(&mut self, entries: &[TranscriptEntryLike]) {
		self.reset();
		self.append(entries);
	}

	/// Append newly persisted entries, then flush the trailing turn.
	pub fn append(&mut self, entries: &[TranscriptEntryLike]) {
		for entry in entries {
			let id = entry_id(entry);
			let Some(message) = transcript_entry_message(entry) else {
				continue;
			};
			let before = self.blocks;
			self.append_message(message);
			if self.blocks > before {
				self.entry_blocks
					.entry(id)
					.or_default()
					.extend(before..self.blocks);
			}
		}
		self.seal_read_group();
		self.flush_pending_calls();
		self.flush_pending_usage();
	}

	/// Toggle transcript-wide expansion for blocks built from now on.
	pub fn set_expanded(&mut self, expanded: bool) {
		self.expanded = expanded;
	}

	/// The transcript-wide expansion state.
	pub fn expanded(&self) -> bool {
		self.expanded
	}

	/// The container index where `entry_id` begins.
	pub fn row_for_entry(&self, entry_id: &str) -> Option<usize> {
		self.entry_blocks.get(entry_id).and_then(|blocks| blocks.first().copied())
	}

	/// Drop components and all build state.
	pub fn reset(&mut self) {
		self.pending_calls.clear();
		self.pending_reads = None;
		self.pending_usage = None;
		self.turn_started_at = None;
		self.turn_usage = TurnUsageTally::new();
		self.last_assistant_usage = None;
		self.entry_blocks.clear();
		self.container.clear();
		self.blocks = 0;
	}

	/// Tear down the builder.
	pub fn dispose(&mut self) {
		self.reset();
	}

	fn push<C: Component + 'static>(&mut self, child: C) -> usize {
		let index = self.blocks;
		self.container.add_child(child);
		self.blocks += 1;
		index
	}

	fn insert<C: Component + 'static>(&mut self, index: usize, child: C) {
		let index = index.min(self.blocks);
		self.container.insert_child_at(index, child);
		self.blocks += 1;
		for call in self.pending_calls.iter_mut() {
			if call.index >= index {
				call.index += 1;
			}
		}
		if let Some(reads) = self.pending_reads.as_mut() {
			if reads.index >= index {
				reads.index += 1;
			}
		}
	}

	fn append_message(&mut self, message: AgentMessage) {
		match message {
			AgentMessage::Assistant(assistant) => self.append_assistant(assistant),
			AgentMessage::User(user) => {
				self.seal_read_group();
				self.flush_pending_usage();
				self.turn_started_at = Some(user.timestamp);
				self.turn_usage = TurnUsageTally::new();
				let text = user.content.text("\n\n");
				if text.trim().is_empty() {
					return;
				}
				let component = UserMessageComponent::new(
					text,
					UserBubbleOptions {
						images: None,
						timestamp: Some(user.timestamp),
						..UserBubbleOptions::default()
					},
					&self.deps.theme,
				);
				self.push(component);
			}
			AgentMessage::Custom(message) => self.append_custom(message),
			AgentMessage::Other(value) => self.append_other(&value),
		}
	}

	fn append_assistant(&mut self, message: crate::chat::messages::AssistantMessage) {
		self.flush_pending_usage();
		let timeline = split_assistant_message_tool_timeline(&message);
		let visible = assistant_has_visible_content(&timeline.before_tools);
		let mut component = self.prose_component(timeline.before_tools, 0);
		if visible {
			self.seal_read_group();
		}
		if self.deps.cache_miss_marker {
			if let Some(info) = detect_cache_invalidation(self.last_assistant_usage.as_ref(), &message.usage)
			{
				component.set_cache_invalidation(Some(info));
			}
		}
		if message.usage.cache_read + message.usage.cache_write + message.usage.input > 0 {
			self.last_assistant_usage = Some(message.usage);
		}
		self.push(component);

		for content in &message.content {
			let Some(call) = content.as_tool_call() else {
				continue;
			};
			self.seal_read_group();
			let render_name = self.deps.render_name(&call.name);
			let args = if call.arguments.is_null() {
				None
			} else {
				Some(call.arguments.clone())
			};
			let after = timeline
				.after_tool_calls
				.iter()
				.find(|(id, _)| id == &call.id)
				.map(|(_, segment)| segment.clone());

			if render_name == "read" && read_args_collapse_into_group(args.as_ref()) {
				let index = self.blocks;
				let reads = self.pending_reads.get_or_insert_with(|| PendingReads {
					entries: Vec::new(),
					index,
					usage_ids: Vec::new(),
				});
				reads.entries.push((call.id.clone(), args, None));
				if let Some(segment) = after {
					if assistant_has_visible_content(&segment) {
						self.seal_read_group();
						let component = self.prose_component(segment, 0);
						self.push(component);
					}
				}
				continue;
			}

			if is_reply_write(&render_name, args.as_ref()) == Some(true) {
				let mut reply = ReplyWriteComponent::new(ReplyWriteOptions {
					request_render: None,
					link_targets: self.deps.link_targets.clone(),
				});
				reply.update_args(args.as_ref());
				reply.set_args_complete();
				self.push(reply);
			} else {
				self.pending_calls.push(PendingCall {
					tool_name: render_name,
					args,
					tool_call_id: call.id.clone(),
					index: self.blocks,
				});
			}
			if let Some(segment) = after {
				if assistant_has_visible_content(&segment) {
					let component = self.prose_component(segment, 0);
					self.push(component);
				}
			}
		}

		let stop = message.stop_reason.as_str();
		let elapsed = if self.deps.show_turn_time {
			turn_elapsed_ms(self.turn_started_at, Some(message.timestamp))
		} else {
			None
		};
		let _summary = self.turn_usage.add(
			&overlay_usage(&message.usage),
			stop,
			None,
			elapsed,
		);
		if self.deps.show_token_usage && assistant_usage_is_billed(&message.usage) {
			self.pending_usage = Some(PendingUsage {
				usage: message.usage,
				duration_ms: None,
				ttft_ms: None,
				timestamp: Some(message.timestamp),
				turn_elapsed: elapsed,
			});
		}
		if let Some(ids) = grouped_read_usage_call_ids(&message) {
			if let Some(reads) = self.pending_reads.as_mut() {
				reads.usage_ids = ids;
			}
		}
	}

	fn prose_component(
		&self,
		message: crate::chat::messages::AssistantMessage,
		_offset: usize,
	) -> AssistantMessageComponent {
		let mut component = AssistantMessageComponent::new(self.deps.theme.clone());
		component.update_content(message, false);
		component.set_hide_thinking_block(self.deps.hide_thinking_block);
		component.set_prose_only_thinking(self.deps.prose_only_thinking);
		component.set_expand_thinking_blocks(self.deps.expand_thinking_blocks);
		component.set_mid_stream_publication(false);
		component.set_images_visible(self.deps.show_images);
		component.set_tool_result_images_visible(!self.deps.hide_tool_activity);
		component.set_table_charts_visible(self.deps.table_charts);
		component.set_link_targets(self.deps.link_targets.clone());
		component.set_expanded(self.expanded);
		component.mark_transcript_block_finalized();
		component
	}

	fn append_other(&mut self, value: &Value) {
		let role = value.get("role").and_then(Value::as_str).unwrap_or_default();
		match role {
			"toolResult" => self.append_tool_result(value),
			"bashExecution" => self.append_bash(value),
			"pythonExecution" => self.append_eval(value),
			_ => {}
		}
	}

	fn append_tool_result(&mut self, value: &Value) {
		let tool_name = value
			.get("toolName")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.to_string();
		let tool_call_id = value
			.get("toolCallId")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.to_string();
		let content = tool_result_blocks(value);
		let details = value.get("details").cloned();
		let is_error = value.get("isError").and_then(Value::as_bool).unwrap_or(false);
		let render_name = self.deps.render_name(&tool_name);

		if render_name == "read" {
			if let Some(reads) = self.pending_reads.as_mut() {
				if let Some(entry) = reads
					.entries
					.iter_mut()
					.find(|(id, _, _)| id == &tool_call_id)
				{
					let text = content
						.iter()
						.filter_map(|block| block.text.clone())
						.collect::<Vec<_>>()
						.join("\n");
					entry.2 = Some((text, details.clone(), is_error));
					return;
				}
			}
		}

		let position = self
			.pending_calls
			.iter()
			.position(|call| call.tool_call_id == tool_call_id);
		let Some(position) = position else {
			return;
		};
		let call = self.pending_calls.remove(position);
		let mut card = self.card(call.tool_name, call.args, Some(call.tool_call_id.clone()));
		card.update_result(
			ToolResultFrame {
				content,
				details,
				is_error,
			},
			false,
			Some(tool_call_id.as_str()),
		);
		card.seal();
		self.insert(call.index, card);
	}

	fn card(
		&self,
		tool_name: String,
		args: Option<Value>,
		tool_call_id: Option<String>,
	) -> ToolExecutionComponent {
		let mut card = ToolExecutionComponent::new(
			tool_name,
			args,
			ToolExecutionOptions {
				show_images: self.deps.show_images,
				use_built_in_renderer: true,
				transcript_presenters: false,
			},
			None,
			self.deps.theme.clone(),
			tool_call_id,
		);
		card.set_expanded(self.expanded);
		card
	}

	fn append_bash(&mut self, value: &Value) {
		let command = value.get("command").and_then(Value::as_str).unwrap_or_default();
		let excluded = value
			.get("excludeFromContext")
			.and_then(Value::as_bool)
			.unwrap_or(false);
		let mut component =
			BashExecutionComponent::new(command.to_string(), self.deps.theme.clone(), excluded);
		if let Some(output) = value.get("output").and_then(Value::as_str) {
			component.append_output(output);
		}
		component.set_complete(
			value.get("exitCode").and_then(Value::as_i64),
			value.get("cancelled").and_then(Value::as_bool).unwrap_or(false),
			bash_completion(value),
			0.0,
		);
		component.set_expanded(self.expanded);
		self.push(component);
	}

	fn append_eval(&mut self, value: &Value) {
		let code = value.get("code").and_then(Value::as_str).unwrap_or_default();
		let excluded = value
			.get("excludeFromContext")
			.and_then(Value::as_bool)
			.unwrap_or(false);
		let mut component = EvalExecutionComponent::new(
			code.to_string(),
			self.deps.theme.clone(),
			excluded,
			EvalExecutionLanguage::Js,
		);
		if let Some(output) = value.get("output").and_then(Value::as_str) {
			component.append_output(output);
		}
		let meta = value.get("meta");
		component.set_complete(
			value.get("exitCode").and_then(Value::as_i64),
			value.get("cancelled").and_then(Value::as_bool).unwrap_or(false),
			EvalCompletion {
				output: None,
				truncation: meta.and_then(|meta| meta.get("truncation")).and_then(truncation_meta),
				artifact_error: meta
					.and_then(|meta| meta.get("artifactError"))
					.and_then(Value::as_str)
					.map(str::to_string),
			},
			0.0,
		);
		component.set_expanded(self.expanded);
		self.push(component);
	}

	fn append_custom(&mut self, message: CustomMessage) {
		if !message.display {
			return;
		}
		if is_user_turn_initiator(&message) {
			self.turn_started_at = Some(message.timestamp);
			self.turn_usage = TurnUsageTally::new();
		}
		self.seal_read_group();
		self.flush_pending_usage();

		let custom_type = message.custom_type.clone();
		match custom_type.as_str() {
			"async-result" => {
				let details = message
					.details
					.as_ref()
					.and_then(async_result_details)
					.unwrap_or_default();
				self.push(build_async_result_block(&self.deps.theme, &details));
			}
			LSP_LATE_DIAGNOSTIC_MESSAGE_TYPE => {
				let files = message
					.details
					.as_ref()
					.and_then(|details| details.get("files"))
					.and_then(Value::as_array)
					.map(|files| files.iter().filter_map(late_diagnostics_file).collect::<Vec<_>>())
					.unwrap_or_default();
				if files.is_empty() {
					return;
				}
				let mut component = LateDiagnosticsMessage::new(files);
				component.set_expanded(self.expanded);
				self.push(component);
			}
			COLLAB_PROMPT_MESSAGE_TYPE => {
				self.push(CollabPromptMessage::new(&message, &self.deps.theme));
			}
			SKILL_PROMPT_MESSAGE_TYPE => {
				let mut component =
					SkillMessageComponent::new(message, None, &self.deps.theme);
				component.set_expanded(self.expanded);
				self.push(component);
			}
			"advisor" => {
				let details = message
					.details
					.as_ref()
					.and_then(advisor_details)
					.unwrap_or_default();
				let card = AdvisorMessageCard::new(
					Some(&details),
					Box::new(|| false),
					self.deps.theme.clone(),
				);
				self.push(card);
			}
			"launch-completion" => {
				let daemons = message
					.details
					.as_ref()
					.and_then(|details| details.get("daemons"))
					.and_then(Value::as_array)
					.map(|daemons| daemons.iter().filter_map(daemon_snapshot).collect::<Vec<_>>())
					.unwrap_or_default();
				let content = message.content.plain_text();
				let content = if content.trim().is_empty() {
					None
				} else {
					Some(content.as_str())
				};
				self.push(build_launch_completion_block(
					&self.deps.theme,
					content,
					&daemons,
				));
			}
			BACKGROUND_TAN_DISPATCH_MESSAGE_TYPE => {
				self.push(create_background_tan_dispatch_block(
					&message,
					&self.deps.theme,
				));
			}
			_ => {
				if let Some(component) = create_handoff_summary_message_component(
					&message,
					self.expanded,
					self.deps.theme.clone(),
				) {
					self.push(component);
					return;
				}
				let mut component =
					CustomMessageComponent::new(message, None, self.deps.theme.clone());
				component.set_expanded(self.expanded);
				self.push(component);
			}
		}
	}

	fn seal_read_group(&mut self) {
		let Some(reads) = self.pending_reads.take() else {
			return;
		};
		let mut group = ReadToolGroupComponent::new(
			ReadToolGroupOptions {
				show_content_preview: self.deps.read_tool_result_preview,
				transcript_presenters: false,
			},
			self.deps.theme.clone(),
		);
		for (id, args, result) in &reads.entries {
			group.update_args_ref(args.as_ref(), Some(id.as_str()));
			if let Some((text, details, is_error)) = result {
				let parsed = details.as_ref().and_then(|value| read_details(value));
				group.update_result(id, text.clone(), parsed.as_ref(), *is_error);
			}
		}
		if let Some(usage) = &self.pending_usage {
			if !reads.usage_ids.is_empty() {
				let attached = group.attach_usage(
					&reads.usage_ids,
					&usage.usage,
					usage.duration_ms,
					usage.ttft_ms,
					usage.timestamp,
					usage.turn_elapsed,
				);
				if attached {
					self.pending_usage = None;
				}
			}
		}
		group.set_expanded(self.expanded);
		group.finalize();
		self.insert(reads.index, group);
	}

	fn flush_pending_calls(&mut self) {
		let pending = std::mem::take(&mut self.pending_calls);
		for call in pending {
			let card = self.card(call.tool_name, call.args, Some(call.tool_call_id));
			self.insert(call.index, card);
		}
	}

	fn flush_pending_usage(&mut self) {
		// The read run may take the metrics itself, so seal it first.
		self.seal_read_group();
		let Some(usage) = self.pending_usage.take() else {
			return;
		};
		let row = format_usage_row(
			&overlay_usage(&usage.usage),
			usage.duration_ms,
			usage.ttft_ms,
			usage.timestamp,
			usage.turn_elapsed,
		);
		self.push(Text::padded(format!("  {row}"), 1, 0));
	}
}

/// Trusted entry id of a persisted entry.
fn entry_id(entry: &TranscriptEntryLike) -> String {
	match entry {
		TranscriptEntryLike::Message(message) => message.id.clone(),
		TranscriptEntryLike::CustomMessage(message) => message.id.clone(),
	}
}

fn overlay_usage(usage: &crate::chat::messages::Usage) -> Usage {
	Usage {
		input: usage.input,
		output: usage.output,
		cache_read: usage.cache_read,
		cache_write: usage.cache_write,
		total_tokens: usage.total_tokens,
		cost_total: 0.0,
	}
}

fn tool_result_blocks(value: &Value) -> Vec<ToolResultBlock> {
	value
		.get("content")
		.and_then(Value::as_array)
		.map(|blocks| {
			blocks
				.iter()
				.map(|block| ToolResultBlock {
					kind: block
						.get("type")
						.and_then(Value::as_str)
						.unwrap_or("text")
						.to_string(),
					text: block.get("text").and_then(Value::as_str).map(str::to_string),
					data: block.get("data").and_then(Value::as_str).map(str::to_string),
					mime_type: block
						.get("mimeType")
						.and_then(Value::as_str)
						.map(str::to_string),
				})
				.collect()
		})
		.unwrap_or_default()
}

fn read_details(value: &Value) -> Option<crate::tools::read::ReadToolDetails> {
	let object = value.as_object()?;
	let mut details = crate::tools::read::ReadToolDetails::default();
	details.kind = object.get("kind").and_then(Value::as_str).map(str::to_string);
	details.display_target = object
		.get("displayTarget")
		.and_then(Value::as_str)
		.map(str::to_string);
	details.resolved_path = object
		.get("resolvedPath")
		.and_then(Value::as_str)
		.map(str::to_string);
	details.is_directory = object
		.get("isDirectory")
		.and_then(Value::as_bool)
		.unwrap_or(false);
	details.total_lines = object
		.get("totalLines")
		.and_then(Value::as_u64)
		.map(|lines| lines as usize);
	details.display_read_targets = object
		.get("displayReadTargets")
		.and_then(Value::as_array)
		.map(|targets| {
			targets
				.iter()
				.filter_map(Value::as_str)
				.map(str::to_string)
				.collect()
		})
		.unwrap_or_default();
	details.display_read_target_links = details
		.display_read_targets
		.iter()
		.map(|_| None)
		.collect();
	Some(details)
}

fn truncation_meta(value: &Value) -> Option<TruncationMeta> {
	let object = value.as_object()?;
	let count = |key: &str| object.get(key).and_then(Value::as_u64).map(|n| n as usize);
	Some(TruncationMeta {
		direction: object
			.get("direction")
			.and_then(Value::as_str)
			.map(str::to_string),
		truncated_by: object
			.get("truncatedBy")
			.and_then(Value::as_str)
			.map(str::to_string),
		total_lines: count("totalLines").unwrap_or(0),
		total_bytes: count("totalBytes").unwrap_or(0),
		output_lines: count("outputLines").unwrap_or(0),
		output_bytes: count("outputBytes").unwrap_or(0),
		max_bytes: count("maxBytes"),
		artifact_id: object
			.get("artifactId")
			.and_then(Value::as_str)
			.map(str::to_string),
		..TruncationMeta::default()
	})
}

fn minimized_meta(value: &Value) -> Option<MinimizedMeta> {
	let object = value.as_object()?;
	let count = |key: &str| object.get(key).and_then(Value::as_u64).unwrap_or(0) as usize;
	Some(MinimizedMeta {
		filter: object
			.get("filter")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.to_string(),
		input_bytes: count("inputBytes"),
		output_bytes: count("outputBytes"),
		artifact_id: object
			.get("artifactId")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.to_string(),
	})
}

fn bash_completion(value: &Value) -> BashCompletion {
	let meta = value.get("meta");
	BashCompletion {
		output: value.get("output").and_then(Value::as_str).map(str::to_string),
		truncation: meta.and_then(|meta| meta.get("truncation")).and_then(truncation_meta),
		artifact_error: meta
			.and_then(|meta| meta.get("artifactError"))
			.and_then(Value::as_str)
			.map(str::to_string),
		minimized: meta.and_then(|meta| meta.get("minimized")).and_then(minimized_meta),
		images: value
			.get("images")
			.and_then(Value::as_array)
			.map(|images| {
				images
					.iter()
					.filter_map(Value::as_str)
					.map(str::to_string)
					.collect()
			})
			.unwrap_or_default(),
	}
}

fn async_result_details(value: &Value) -> Option<AsyncResultDetails> {
	let object = value.as_object()?;
	let jobs = object
		.get("jobs")
		.and_then(Value::as_array)
		.map(|jobs| {
			jobs.iter()
				.map(|job| crate::chat::transcript_render_helpers::AsyncResultJob {
					job_id: job.get("jobId").and_then(Value::as_str).map(str::to_string),
					job_type: job.get("jobType").and_then(Value::as_str).map(str::to_string),
					label: job.get("label").and_then(Value::as_str).map(str::to_string),
					duration_ms: job.get("durationMs").and_then(Value::as_f64),
					meta: None,
				})
				.collect()
		})
		.unwrap_or_default();
	Some(AsyncResultDetails {
		job_id: object.get("jobId").and_then(Value::as_str).map(str::to_string),
		job_type: object.get("jobType").and_then(Value::as_str).map(str::to_string),
		label: object.get("label").and_then(Value::as_str).map(str::to_string),
		duration_ms: object.get("durationMs").and_then(Value::as_f64),
		jobs,
		meta: None,
	})
}

fn late_diagnostics_file(value: &Value) -> Option<LateDiagnosticsFile> {
	let object = value.as_object()?;
	Some(LateDiagnosticsFile {
		path: object.get("path").and_then(Value::as_str).map(str::to_string),
		summary: object.get("summary").and_then(Value::as_str).map(str::to_string),
		errored: object.get("errored").and_then(Value::as_bool).unwrap_or(false),
		messages: object
			.get("messages")
			.and_then(Value::as_array)
			.map(|messages| {
				messages
					.iter()
					.filter_map(Value::as_str)
					.map(str::to_string)
					.collect()
			})
			.unwrap_or_default(),
	})
}

fn advisor_details(value: &Value) -> Option<AdvisorMessageDetails> {
	let notes = value.get("notes").and_then(Value::as_array)?;
	Some(AdvisorMessageDetails {
		notes: notes
			.iter()
			.map(|note| AdvisorNote {
				note: note
					.get("note")
					.and_then(Value::as_str)
					.unwrap_or_default()
					.to_string(),
				severity: note
					.get("severity")
					.and_then(Value::as_str)
					.and_then(AdvisorSeverity::from_str),
				advisor: note.get("advisor").and_then(Value::as_str).map(str::to_string),
				turn: note.get("turn").and_then(Value::as_u64),
				turns_ago: note.get("turnsAgo").and_then(Value::as_u64),
			})
			.collect(),
	})
}

fn daemon_snapshot(value: &Value) -> Option<DaemonSnapshot> {
	let object = value.as_object()?;
	let state = match object.get("state").and_then(Value::as_str).unwrap_or_default() {
		"starting" => DaemonState::Starting,
		"running" => DaemonState::Running,
		"ready" => DaemonState::Ready,
		"restarting" => DaemonState::Restarting,
		"stopping" => DaemonState::Stopping,
		"exited" => DaemonState::Exited,
		"failed" => DaemonState::Failed,
		_ => DaemonState::Starting,
	};
	Some(DaemonSnapshot {
		name: object
			.get("name")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.to_string(),
		state,
		exit_code: object
			.get("exitCode")
			.and_then(Value::as_i64)
			.map(|code| code as i32),
		started_at: object.get("startedAt").and_then(Value::as_f64).unwrap_or(0.0),
		exited_at: object.get("exitedAt").and_then(Value::as_f64),
		..DaemonSnapshot::default()
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::chat::messages::{
		AssistantContent, AssistantMessage, StopReason, TextContent, ToolCall,
	};
	use crate::chat::transcript_entry::{SessionMessageEntryLike, UserContent, UserMessage};
	use crate::chat::user_message::CollapsedSyntheticMessageComponent;
	use crate::theme::builtin::builtin_theme_json;
	use crate::theme::color::ColorMode;
	use crate::theme::loader::{create_theme, CreateThemeOptions};
	use serde_json::json;

	fn theme() -> Theme {
		create_theme(
			builtin_theme_json("dark").unwrap(),
			CreateThemeOptions {
				mode: Some(ColorMode::TrueColor),
				..Default::default()
			},
		)
		.unwrap()
	}

	fn builder() -> ChatTranscriptBuilder {
		ChatTranscriptBuilder::new(ChatTranscriptBuilderDeps::new(theme()))
	}

	fn entry(id: &str, message: AgentMessage) -> TranscriptEntryLike {
		TranscriptEntryLike::Message(SessionMessageEntryLike {
			id: id.to_string(),
			parent_id: None,
			timestamp: "2026-01-01T00:00:00.000Z".to_string(),
			message,
		})
	}

	fn user(id: &str, text: &str) -> TranscriptEntryLike {
		entry(
			id,
			AgentMessage::User(UserMessage {
				content: UserContent::Text(text.to_string()),
				timestamp: 1_000,
			}),
		)
	}

	fn assistant(content: Vec<AssistantContent>) -> AgentMessage {
		AgentMessage::Assistant(AssistantMessage {
			content,
			stop_reason: StopReason::ToolUse,
			timestamp: 2_000,
			..AssistantMessage::default()
		})
	}

	fn plain(rows: &[String]) -> String {
		rows.iter()
			.map(|row| crate::ansi_scan::strip_ansi(row))
			.collect::<Vec<_>>()
			.join("\n")
	}

	#[test]
	fn rebuild_renders_user_and_assistant_prose() {
		let mut builder = builder();
		builder.rebuild(&[
			user("u1", "please add a guard"),
			entry(
				"a1",
				assistant(vec![AssistantContent::Text(TextContent {
					text: "Guard added.".to_string(),
					..TextContent::default()
				})]),
			),
		]);
		assert!(!builder.is_empty());
		let rows = builder.container.render_semantic(80);
		let text = plain(&rows);
		assert!(text.contains("please add a guard"), "{text}");
		assert!(text.contains("Guard added."), "{text}");
		assert_eq!(builder.row_for_entry("u1"), Some(0));
	}

	#[test]
	fn rebuild_groups_reads_and_matches_the_result() {
		let mut builder = builder();
		builder.rebuild(&[
			user("u1", "look"),
			entry(
				"a1",
				assistant(vec![AssistantContent::ToolCall(ToolCall {
					id: "c1".to_string(),
					name: "read".to_string(),
					arguments: json!({"path": "src/lib.rs"}),
					..ToolCall::default()
				})]),
			),
			entry(
				"r1",
				AgentMessage::Other(json!({
					"role": "toolResult",
					"toolName": "read",
					"toolCallId": "c1",
					"content": [{"type": "text", "text": "1: hello"}],
					"details": {"kind": "file", "displayTarget": "src/lib.rs"},
					"isError": false
				})),
			),
		]);
		let rows = builder.container.render_semantic(80);
		let text = plain(&rows);
		assert!(text.contains("src/lib.rs"), "{text}");
	}

	#[test]
	fn unmatched_calls_seal_without_a_result() {
		let mut builder = builder();
		builder.rebuild(&[entry(
			"a1",
			assistant(vec![AssistantContent::ToolCall(ToolCall {
				id: "c9".to_string(),
				name: "bash".to_string(),
				arguments: json!({"command": "ls"}),
				..ToolCall::default()
			})]),
		)]);
		assert_eq!(builder.blocks, 2);
		builder.reset();
		assert!(builder.is_empty());
	}

	#[test]
	fn synthetic_prompts_collapse_and_real_prompts_render() {
		let mut builder = builder();
		let mut component = CollapsedSyntheticMessageComponent::new(String::new(), None, &theme());
		assert!(!component.is_expanded());
		component.set_expanded(true);
		assert!(component.is_expanded());
		builder.rebuild(&[user("u1", "hi")]);
		assert!(!builder.is_empty());
	}
}
