# 07 — Context Assembly, Token Accounting, and Compaction

## 1. Purpose

The context assembler transforms durable transcript and selected workspace/memory inputs into an immutable provider request snapshot. It is a pure planning boundary as far as possible: it does not mutate the transcript and does not execute tools.

## 2. Input categories

- system contract and safety policy;
- provider/model capability constraints;
- current user request;
- selected transcript branch;
- recent messages;
- tool schemas;
- project instructions;
- retrieved memory;
- relevant file excerpts;
- prior compaction summaries;
- reserved output budget.

Every item carries source, trust, timestamp/version, token estimate, and truncation policy.

## 3. Budget model

`available_input = model_context_limit - reserved_output - provider_overhead - safety_margin`

The assembler must not assume token count equals UTF-8 byte length. Each provider adapter supplies a tokenizer or conservative estimator. If exact tokenization is unavailable, use a documented conservative estimate and leave margin.

Budget allocation order is policy-controlled:
1. mandatory system contract;
2. current user request;
3. required tool schemas;
4. recent conversational context;
5. relevant project context;
6. retrieved memory;
7. older transcript summaries.

Never truncate the current user request silently. If it alone exceeds budget, ask for a shorter request or use a supported file/reference workflow.

## 4. Compaction

Compaction creates a derived summary with:
- summary ID;
- source entry range/list;
- source digest;
- model/provider metadata if model-generated;
- creation time;
- token estimate;
- confidence and coverage notes;
- omitted facts/uncertainties.

The original transcript is immutable. A summary is not a replacement for source records and must be traceable to them.

## 5. Retrieval

Retrieval is bounded by:
- maximum candidate count;
- maximum bytes per candidate;
- maximum total tokens;
- query deadline;
- allowed source classes;
- trust policy.

Memory retrieval must not override direct user instructions or system policy. Conflicting memory candidates are surfaced with provenance or excluded.

## 6. Prompt injection boundary

Project files, tool output, peer messages, imported documents, and memory are data, not authority. The context assembler labels these sections and preserves provenance. The model prompt must explicitly distinguish instructions from untrusted content.

## 7. Cache correctness

Context cache key includes transcript head, config digest, model capability digest, tool schema digest, workspace snapshot identifiers, memory version, and compaction version. Any relevant change invalidates the cache.

## 8. Verification

- property tests for budget never exceeding limit plus documented estimator margin;
- fixtures for truncation order;
- tests for provenance retention;
- tests proving compaction never mutates source transcript;
- adversarial tests for untrusted instructions embedded in files and tool output.
