# 11 — Workspace and Filesystem Service

## 1. Workspace boundary

Every filesystem operation resolves against a registered workspace root. The service canonicalizes the root once, records filesystem identity where available, and applies explicit symlink and mount-boundary policy.

## 2. Path representation

Internal API uses `WorkspaceRelativePath`, not arbitrary absolute strings. Validation rejects:
- parent traversal outside root;
- NUL bytes;
- invalid platform separators;
- reserved internal storage paths;
- paths exceeding configured length/depth;
- forbidden device or special files.

Normalization must preserve platform semantics. Do not lowercase paths unless the filesystem is known case-insensitive.

## 3. Symlink policy

Default policy: reads may follow symlinks only if the resolved target remains within the allowed workspace boundary; writes reject symlink traversal unless explicitly approved. The check and open must avoid time-of-check/time-of-use races where platform APIs allow descriptor-relative operations.

## 4. Snapshot reads

A file snapshot includes:
- relative path;
- file identity;
- size;
- modification metadata;
- content digest;
- bytes or blob reference;
- encoding classification;
- read timestamp.

A digest is authoritative for content equality; mtime is only a hint.

## 5. Ignore rules

Ignore policy affects discovery and context inclusion, not security authorization. A hidden/ignored file may still be explicitly read if policy allows. Secret-like files receive stricter context and logging treatment.

## 6. Atomic writes

Preferred commit:
1. create temporary sibling file with restrictive permissions;
2. write all bytes;
3. flush file;
4. optionally sync file according to durability profile;
5. verify target base identity/digest;
6. atomically replace target where supported;
7. sync parent directory where supported;
8. record committed digest.

If atomic replacement is unavailable, use a documented platform fallback and mark weaker durability.

## 7. File types

Default mutation policy rejects sockets, devices, FIFOs, and unexpected directories. Binary files are supported only by tools that explicitly declare binary-safe behavior.

## 8. Watchers

Filesystem notifications are hints, not truth. On overflow, missed events, or watcher restart, rescan relevant paths. Debounce bursts and bound queue size.

## 9. Workspace fingerprint

Fingerprint includes normalized root identity and stable metadata, not secret file contents. It is used to detect accidental peer mismatch, not as an authorization credential.
