# 24 — Rust Workspace and Module/Crate Map

## 1. Packaging principle

Do not create one crate per small subsystem. Crate boundaries should reflect stable dependency direction, reuse, security isolation, or build/test isolation. Internal modules are preferred for tightly coupled domain logic.

## 2. Proposed crate map

| Crate | Responsibility | Depends on |
|---|---|---|
| `maple-domain` | IDs, aggregates, value objects, invariants | foundational types |
| `maple-application` | use cases, orchestration ports | domain |
| `maple-runtime` | actors, supervisors, lifecycle | application, domain |
| `maple-storage` | journal, blobs, indexes, migrations | domain ports |
| `maple-provider` | provider-neutral gateway contracts | domain/application ports |
| `maple-provider-*` | concrete provider adapters | provider contracts |
| `maple-tools` | tool registry, policy, execution | application/domain |
| `maple-workspace` | path safety, snapshots, atomic commits | domain |
| `maple-edit` | Hashline protocol and edit planner | workspace/domain |
| `maple-mesh` | native peer coordination | domain/storage ports |
| `maple-crew` | DAG scheduler and task ledger | runtime/application |
| `maple-memory` | candidate pipeline and retrieval | domain/storage |
| `maple-resource` | budgets, permits, samplers | runtime ports |
| `maple-tui` | reusable presentation toolkit | view models only |
| `maple-cli` | CLI shell | application |
| `maple-app` | binary composition root | all selected adapters |
| `maple-interop-pi` | optional external adapter | neutral DTOs only |

Names are proposals; actual crate count should be optimized after dependency graph and compile-time measurement.

## 3. Module-level decomposition

Each crate should separate:
- `model`: domain types;
- `service`: domain/application behavior;
- `port`: abstract I/O contracts;
- `adapter`: concrete integration;
- `codec`: serialization;
- `error`: typed failures;
- `test_support`: fixtures and fakes.

Avoid a generic `utils` crate that becomes a dependency dumping ground.

## 4. Feature flags

Default feature set includes core local functionality. Optional provider adapters may be selected by build features. Pi interop is excluded by default. A feature flag must not change canonical domain semantics.

## 5. Unsafe Rust

Default to `#![forbid(unsafe_code)]` where practical. If terminal/OS adapters require unsafe:
- isolate it in a small module;
- document safety invariants;
- provide safe wrapper API;
- test boundary conditions;
- review every unsafe block.

## 6. Dependency policy

Pin minimum supported versions, audit licenses and advisories, minimize duplicate async/runtime stacks, and avoid bringing provider SDK types into domain crates. Build-time dependency growth is measured.
