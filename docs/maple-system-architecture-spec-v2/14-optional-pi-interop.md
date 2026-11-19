# 14 — Optional Pi Interoperability Adapter

## 1. Normative status

<badge>OPTIONAL</badge>

Pi compatibility is an edge integration, not a Maple architecture dependency. Maple Core MUST NOT require:
- a Pi executable or process;
- a `.pi` directory;
- Pi environment variables;
- Pi-specific session IDs or internal structs;
- Pi-specific message storage;
- Pi-specific peer discovery;
- Pi-specific lock files;
- Pi-specific configuration to start or resume a Maple session.

The native Maple protocol remains canonical.

## 2. Supported integration shape

The adapter may provide explicitly requested import/export or bridge functionality. Candidate capabilities:
- import selected external conversation/message data;
- export a Maple transcript into an approved external representation;
- bridge messages between Maple-native peers and an external messenger format;
- inspect external metadata in read-only mode.

Each capability is independently feature-gated. The initial release SHOULD prefer offline import/export over a continuously running bridge because it has fewer lifecycle and consistency risks.

## 3. Isolation

Recommended packaging:
- `maple-interop-core`: boundary-neutral mapping DTOs, if shared mapping is needed;
- `maple-interop-pi`: parser/serializer and compatibility tests, compiled only when enabled;
- no dependency from `maple-domain`, `maple-session`, `maple-runtime`, or `maple-storage` to `maple-interop-pi`.

The default binary build excludes Pi-specific dependencies. If compile-time feature flags are used, the default feature set must not enable them.

## 4. Mapping rules

External records are mapped into Maple canonical DTOs. Preserve:
- original external identifier as provenance metadata;
- source format/version;
- import timestamp;
- original payload digest where possible;
- mapping warnings;
- unknown fields when safe and bounded.

Do not map external identity directly to a Maple primary key. Generate a new Maple ID and store the external ID as an external reference.

## 5. Import safety

Import is a read-only parse followed by validation and preview. It must not:
- execute imported tool calls;
- treat imported system messages as Maple system policy;
- overwrite an existing session without explicit confirmation;
- import secrets by default;
- trust external paths;
- create peer reservations from imported lock metadata.

Commit import as a new Maple session or a separately identified branch with imported provenance.

## 6. Export safety

Export is a projection, not a lossless guarantee unless a format-specific contract proves it. Report dropped/converted fields. Redact secrets and local paths according to export policy. Never mutate the original Maple transcript.

## 7. Bridge semantics

If a live bridge is ever implemented:
- use a supervised task;
- enforce bounded queues;
- use message IDs and dedupe;
- define delivery/ack semantics;
- avoid echo loops with origin markers;
- define shutdown and reconnect behavior;
- isolate bridge failure from core session health.

## 8. Version compatibility

Each supported external version requires:
- checked-in golden fixtures;
- parser behavior tests;
- unknown-field tests;
- malformed-input tests;
- round-trip tests where applicable;
- explicit compatibility statement.

No claim of “Pi compatible” is allowed without naming the supported feature subset and tested versions.

## 9. Disablement acceptance tests

- default build contains no Pi adapter dependency;
- default startup succeeds with no `.pi` directory;
- core session creation/resume succeeds with integration disabled;
- deleting adapter feature does not require core schema migration;
- malformed external data cannot corrupt Maple-native journals;
- adapter failure cannot block terminal restoration or core shutdown.

## 10. Decision

Pi compatibility is a MAY capability. The core team may defer it indefinitely without violating the product architecture.
