# 08 — Provider Gateway and Model Capability System

## 1. Responsibility

The gateway provides a provider-neutral interface for model requests, streaming, cancellation, usage accounting, and error normalization. The agent orchestrator must not know provider-specific JSON or SDK types.

## 2. Core interfaces

Conceptual ports:
- `ProviderCatalog::list()`
- `CredentialResolver::resolve(provider_id)`
- `ModelCatalog::capabilities(model_id)`
- `ModelGateway::complete(request, cancellation, deadline)`
- `StreamDecoder::decode(bytes) -> ProviderEvent`
- `UsageNormalizer::normalize(raw_usage)`

## 3. Canonical request

Fields:
- request ID;
- provider/model ID;
- messages with role and provenance;
- tool definitions;
- sampling parameters;
- output token cap;
- response mode;
- metadata;
- deadline;
- idempotency key when supported.

The canonical request must not contain raw secret values. Credential injection occurs inside the adapter immediately before transport.

## 4. Canonical stream events

- `StreamStarted`
- `TextDelta`
- `ReasoningDelta` only where provider exposes it and policy allows display/storage
- `ToolCallStarted`
- `ToolArgumentDelta`
- `ToolCallReady`
- `UsageUpdated`
- `StreamWarning`
- `StreamCompleted`
- `StreamFailed`

Partial tool arguments are never executable. Execution begins only after a complete call is assembled and schema-validated.

## 5. Provider adapter contract

Each adapter must define:
- supported roles and content blocks;
- tool calling behavior;
- streaming framing;
- context limits;
- output limits;
- cancellation behavior;
- retryable status/error classes;
- rate-limit behavior;
- usage fields and confidence;
- known semantic deviations.

## 6. Retry policy

Retry only when:
- the failure is classified retryable;
- the request is safe to retry or provider idempotency is available;
- the deadline and retry budget allow it;
- no user-visible duplicate side effect can occur.

Use bounded exponential backoff with jitter. Do not retry malformed requests, invalid credentials, policy refusals, or completed streams.

## 7. Error taxonomy

`Authentication`, `Authorization`, `RateLimited`, `Unavailable`, `Timeout`, `Connection`, `MalformedResponse`, `UnsupportedCapability`, `ContextTooLarge`, `ProviderRefusal`, `Cancelled`, `Unknown`.

Map provider-specific details into stable error codes while retaining a redacted diagnostic cause.

## 8. Credential handling

Credentials are resolved from supported secure sources. They must not be:
- placed in model context;
- written to session journal;
- included in debug logs;
- copied into child process environment unless explicitly required;
- returned in error messages.

## 9. Model capability negotiation

Capability flags include streaming, tool calls, parallel tool calls, structured output, image input, audio input, context window, output ceiling, and cancellation support. The orchestrator plans only features supported by the selected model.

## 10. Contract tests

Every provider adapter runs the same fake-server suite: request serialization, stream chunk boundaries, malformed frames, disconnect mid-stream, cancellation, rate limits, usage normalization, and secret redaction.
