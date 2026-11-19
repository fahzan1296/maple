# 28 — Data Contract Catalog

This catalog defines minimum canonical fields. Concrete Rust structs and serialization formats are implementation decisions constrained by these contracts.

## 1. Event envelope

| Field | Type | Required | Rule |
|---|---|---:|---|
| event_id | EventId | yes | unique |
| aggregate_id | AggregateId | yes | owner aggregate |
| sequence | u64 | yes | strictly increasing |
| schema_version | u16 | yes | explicit |
| occurred_at | UTC timestamp | yes | wall clock |
| correlation_id | CorrelationId | yes | request chain |
| causation_id | EventId? | no | originating event |
| payload | typed enum | yes | bounded |

## 2. Session manifest

`session_id`, `schema_version`, `created_at`, `workspace_id`, `working_directory_hint`, `active_branch_id`, `model_ref`, `config_digest`, `journal_format`, `last_known_sequence`.

No secret values. Paths are hints and must be revalidated on load.

## 3. Transcript entry envelope

`entry_id`, `session_id`, `branch_id`, `parent_entry_id`, `sequence`, `kind`, `payload_ref`, `provenance`, `visibility`, `created_at`.

## 4. Provider request

`request_id`, `provider_id`, `model_id`, `messages`, `tools`, `generation`, `deadline_ms`, `correlation_id`, `capability_snapshot_digest`.

## 5. Tool invocation

`tool_call_id`, `turn_id`, `tool_name`, `schema_version`, `arguments`, `workspace_id`, `requested_capabilities`, `deadline_ms`, `idempotency_key`, `approval_id`.

## 6. Tool outcome

`tool_call_id`, `status`, `summary`, `structured_output`, `stdout_ref`, `stderr_ref`, `exit_code`, `duration_ms`, `resource_usage`, `side_effect_status`, `truncated`.

## 7. Peer envelope

`protocol_version`, `message_id`, `sender_peer_id`, `sender_instance_id`, `recipient`, `kind`, `created_at`, `expires_at`, `correlation_id`, `payload_schema_version`, `payload_digest`.

## 8. Reservation

`reservation_id`, `resource_key`, `owner_peer_id`, `owner_instance_id`, `session_id`, `base_digest`, `lease_expires_at`, `heartbeat_sequence`, `purpose`.

## 9. Crew task

`task_id`, `crew_id`, `dependencies`, `input_refs`, `capabilities`, `resource_class`, `deadline`, `attempt`, `claim_owner`, `lease_expiry`, `status`, `output_refs`.

## 10. Memory record

`memory_id`, `scope`, `content`, `source_refs`, `trust`, `created_at`, `expires_at`, `version`, `status`, `content_digest`.

## 11. Blob reference

`digest`, `size_bytes`, `media_type`, `encoding_hint`, `created_at`, `integrity_state`.

## 12. Compatibility rule

External formats are mapped into these contracts. No external schema is permitted to become the canonical storage schema without an explicit architecture decision and migration plan.
