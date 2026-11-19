# Legacy V1 reference

This file is preserved for comparison only. Where it conflicts with V2, V2 is authoritative. In particular, `.pi` compatibility is optional and Maple-native behavior is canonical.

# 13 — Flat multi-session mesh protocol

## Purpose
Local peer discovery, presence, direct messaging, broadcast, activity feed, reservations and shared team memory. The mesh is file-based and compatible with `.pi/messenger`. It is not a consensus protocol or remote transport.

## Layout
```text
.pi/messenger/
 registry/<peer-name>.json
 inbox/<peer-name>/<timestamp>-<unique>.json
 feed.jsonl
 swarm.lock
 claims.json
 completions.json
 team/{memory.jsonl,decisions.md}
```

## Operations
`join`, `leave`, `list`, `whois`, `status`, `rename`, `send`, `broadcast`, `feed`, `reserve`, `release`, `team.memory.note`, `team.memory.list`.

## Presence
Status derives from PID/process marker, last heartbeat/activity, explicit status and start time. PID alive alone is insufficient due to reuse. Registry is eventually consistent; it is not authoritative proof of ownership.

## Inbox delivery
Watcher is a hint. Consumer validates size/schema/path, deduplicates message ID, persists handling, then acknowledges/deletes. On watcher overflow/error, bounded polling/rescan recovers. Delivery guarantees must be explicitly stated (at-least-once preferred with dedup).

## Feed
Append-only JSONL with event ID/cursor, type, actor, timestamp and bounded metadata. Tail reads are bounded and partial final lines are ignored until complete.

## Lock
Exclusive-create lock with owner/session/PID/process marker/time/purpose. Reclaim only after owner liveness and stale age are checked. Bounded retries; no busy loop.

## Compatibility
Use explicit wire DTOs; never serialize internal domain structs directly. Maintain fixture tests for registry, inbox, feed, claims and reservations against known compatible peers.

## Failure model
Watcher failure → polling; duplicate message → dedup; partial feed line → skip tail; missing recipient → delivery failure; unknown lock owner → deny; network filesystem → unsupported until verified.
