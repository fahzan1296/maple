# 22 — CLI, Configuration, and Secret Resolution

## 1. CLI commands

Proposed command families:
- `maple` interactive session;
- `maple resume <session-id>`;
- `maple run --prompt ...` non-interactive;
- `maple sessions list|show|export|delete`;
- `maple config show|validate`;
- `maple doctor`;
- `maple storage verify|gc`;
- `maple mesh status|join|leave`;
- `maple crew inspect|cancel`;
- optional `maple interop ...` only when adapter is built/enabled.

Exact names remain OPEN until CLI compatibility and UX review.

## 2. Parsing rules

- reject unknown flags unless explicitly forward-compatible;
- bound argument lengths;
- distinguish missing from empty values;
- avoid shell parsing of user-supplied argument strings;
- provide machine-readable output mode;
- never print secret values in `config show`.

## 3. Configuration layers

Suggested precedence:
1. compiled safety floor;
2. system policy, if present;
3. project config;
4. user config;
5. documented environment variables;
6. CLI overrides.

The final effective configuration is validated as a whole and its non-secret digest is recorded in the session manifest.

## 4. Config schema

Every setting has:
- key;
- type;
- default;
- allowed range;
- source precedence;
- reload behavior;
- security impact;
- documentation;
- validation error.

Unknown keys produce warning or error according to schema version. Typos must not silently disable security controls.

## 5. Secrets

Secret references are separate from ordinary config. Support only explicitly designed sources such as environment variables or OS credential store. Never store plaintext secrets in project config, session manifest, journal, memory, or crash report.

## 6. Hot reload

Only settings explicitly marked reloadable may change during a session. Provider model, tool policy, or workspace scope changes require a controlled transition and new configuration snapshot. Resource hard limits cannot be raised below the safety floor.

## 7. Project trust

Project configuration is untrusted until user approves the workspace. Untrusted project config cannot enable network access, weaken tool approvals, or load executable extensions.

## 8. Diagnostics

`doctor` checks writable paths, storage integrity, provider config presence (without revealing values), terminal capabilities, resource limits, and optional subsystem status.
