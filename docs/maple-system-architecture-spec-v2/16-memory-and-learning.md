# 16 — Memory, Skills, and Autolearn Pipeline

## 1. Separation of memory classes

- session memory: facts relevant only to one session;
- project memory: approved durable knowledge for a workspace;
- user memory: explicitly user-scoped preferences, where supported;
- skill definition: reusable procedural guidance with version and trust metadata;
- candidate memory: unapproved learning awaiting review.

Do not mix these stores or silently promote one scope into another.

## 2. Candidate extraction

Candidates may be proposed from completed turns, but extraction is not acceptance. Each candidate records source transcript entries, evidence, confidence, scope, expiry, and sensitivity classification.

## 3. Two-phase consolidation

Phase A — prepare:
1. collect candidate;
2. validate provenance;
3. redact secrets and personal data;
4. deduplicate;
5. detect contradiction;
6. classify scope;
7. produce reviewable diff.

Phase B — commit:
1. apply approval policy;
2. write versioned memory record;
3. append audit event;
4. update retrieval index;
5. retain previous version for rollback.

## 4. Trust model

Model-generated memory is untrusted until validated. Peer-originated memory is untrusted. Tool output is evidence, not instruction. A memory item cannot grant tool permissions or weaken safety policy.

## 5. Retrieval

Retrieval returns snippets with source IDs, scope, trust, age, and relevance. The context assembler decides whether and how to include them. Retrieval must be bounded and deterministic enough for testing.

## 6. Expiry and correction

Memory items may expire, be superseded, or be retracted. Retraction creates an audit record; it does not rewrite transcript history. Contradictory facts remain distinguishable by source and timestamp.

## 7. Privacy

Never store credentials, access tokens, private keys, or raw secret material. Avoid storing unnecessary personal data. Memory export and deletion must be scope-aware and documented.

## 8. Skill lifecycle

A skill has ID, version, description, allowed tools, trust level, content digest, source, and review status. Skills are data, not executable plugins. Any executable extension requires a separate security design.

## 9. Verification

Test secret redaction, provenance retention, duplicate detection, contradiction handling, approval gates, scope isolation, expiry, rollback, and prompt-injection resistance.
