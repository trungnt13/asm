# ASM repository instructions

Read [the fork guide](docs/porting-from-codex.md) first. It is authoritative for fork intent, workflow, validation, platforms, CI, releases, and installation; it overrides conflicting upstream instructions. Keep fork-specific intent there rather than duplicating it here.

## Rust

Rust code lives in `codex-rs`; crate names use the `codex-` prefix.

- Never add or modify code related to `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` or `CODEX_SANDBOX_ENV_VAR`.
- Inline `format!` variables, collapse nested `if` statements, and prefer method references over redundant closures where applicable.
- Prefer exhaustive `match` statements over wildcard arms.
- Keep modules private and export only the public API needed by callers. Avoid test-only public helpers.
- Avoid opaque boolean or `Option` parameters; prefer enums, named methods, or newtypes when they clarify calls.
- For positional `None`, boolean, or numeric literals, use an exact `/*param_name*/` comment. A sole non-self argument is exempt when method and parameter names match. String and character literals need no comment unless it adds clarity. `just argument-comment-lint` checks this convention; run it only when relevant.
- Document new traits: explain their purpose and how implementations should use them.
- Prefer native trait methods returning `impl Future<Output = T> + Send` over `#[async_trait]` or `#[allow(async_fn_in_trait)]`. Implementations may use `async fn` when they satisfy that contract.
- Do not create small helper methods used only once.
- Instrument async function definitions with `#[tracing::instrument(...)]`, not futures at call sites. Check whether the callee or its immediate implementation is already instrumented.
- For MCP tool mutation and calls, prefer existing abstractions in `codex-rs/codex-mcp/src/mcp_connection_manager.rs` rather than plumbing changes through unrelated layers.
- Let incremental request checks decide whether to reuse a session; do not call `reset_client_session` unnecessarily.

### Structure and change size

- Resist adding new concepts to `codex-core`; consider an existing suitable crate or a new crate first.
- Target modules under 500 lines excluding tests. Above roughly 800 lines, put new functionality in another module unless there is a strong documented reason not to.
- Keep central TUI files focused on orchestration, especially `app.rs`, `chatwidget.rs`, and `bottom_pane` composer, footer, and module files. Avoid new standalone `chatwidget.rs` methods unless trivial.
- Move related tests and type/module documentation with extracted implementation.
- Keep nonmechanical changes under 800 changed lines and complex logic changes under 500. For larger changes, identify the smallest coherent stage using actual dependencies and call sites.

### Generated files and build inputs

- After changing `ConfigToml` or nested config types, run `just write-config-schema` to update `codex-rs/core/config.schema.json`.
- After changing Rust dependencies in `Cargo.toml` or `Cargo.lock`, run `just bazel-lock-update` from the repository root and include the `MODULE.bazel.lock` update.
- For new compile-time file access (`include_str!`, `include_bytes!`, `sqlx::migrate!`, etc.), update the crate's `BUILD.bazel` inputs (`compile_data`, `build_script_data`, or test data). Cargo success does not establish Bazel correctness.

## Compatibility and model context

Check changes against app-server APIs, raw response events (`rawResponseItem/*`, including experimental events), CLI arguments, config loading, and resuming existing rollouts.

For model-visible context:

- Build history incrementally; do not rewrite it.
- Avoid frequent context changes that break caching.
- Give every injected item a hard size cap; no item may exceed 10K tokens.
- Flag new individual items that can exceed 1K tokens as P0 for additional manual review.
- Define injected fragments as structs in `core/context` implementing `ContextualUserFragment`.

Preserve inherited Linux, macOS, Windows, and mixed app-server/exec-server OS compatibility unless an approved change is explicitly platform-specific. Local checks, fork CI, and release coverage follow the fork guide; they are not proof of every supported configuration. Use the `remote-tests` skill when writing tests for mixed-OS execution.

## Validation and tests

Follow the fork guide's narrow validation rules. Use `just test -p <crate> <test-filter>`, not direct `cargo test`; do not default to whole-crate, workspace, or `--all-features` runs. Explain any coverage gap before asking to expand checks.

- After code changes, run `just fmt` from `codex-rs`. For large Rust changes, use scoped `just fix -p <crate>`; avoid workspace-wide Clippy by default.
- Allow Rust commands to finish; a build lock can explain a delay. Do not kill them by PID.
- Prefer integration tests for agent changes. Features changing agent logic require integration coverage; identify the major logic changes and user-visible behaviors to cover.
- Reuse existing helpers. Avoid tests for static values or negative tests for removed logic.
- Prefer `pretty_assertions::assert_eq` on whole objects rather than separate field assertions.
- Avoid mutating process environment in tests; pass environment-derived flags or dependencies instead.
- Put new test modules in sibling `*_tests.rs` files with explicit `#[path = "..._tests.rs"]` attributes. Do not move existing inline modules solely to enforce this convention.
- Avoid test-only functions in production implementation.

### UI snapshots

Changes to user-visible UI or text require corresponding `insta` coverage: update an existing snapshot or add a test when none covers the behavior.

- Generate snapshots with the affected focused tests, not the entire TUI suite by default.
- From the repository root, inspect pending snapshots with `cargo insta pending-snapshots --manifest-path codex-rs/tui/Cargo.toml`; read relevant `*.snap.new` files or use `cargo insta show`.
- Accept only reviewed, intended updates. Use `cargo insta accept --manifest-path codex-rs/tui/Cargo.toml --snapshot <snapshot>` for selected snapshots; omit `--snapshot` only when all pending snapshots in that crate are intended.

### Binaries and fixtures

- Resolve first-party test binaries with `codex_utils_cargo_bin::cargo_bin`, not `assert_cmd::Command::cargo_bin` or `escargot`.
- Resolve fixture resources with `codex_utils_cargo_bin::find_resource!`, not `env!("CARGO_MANIFEST_DIR")`, so Cargo and Bazel runfiles both work after directory changes.

### Core integration tests

- Use `core_test_support::responses` helpers and `TestCodexBuilder::build_with_auto_env()` by default.
- Keep the `ResponseMock` returned by `mount_sse*` to verify outbound `/responses` requests.
- Inspect `single_request()` or `requests()` through structured `ResponsesRequest` helpers rather than manual JSON digging.
- Build SSE payloads with `ev_*` constructors and `sse(...)`.
- Prefer `wait_for_event` over `wait_for_event_with_timeout`, and `mount_sse_once` over matching or sequence helpers when sufficient.

### App-server integration tests

Exercise the public JSON-RPC API. Use `TestAppServer::builder().build()` and `send_thread_start_request_with_auto_env()` by default, with response mocking as for core tests.

### Benchmarks

Use `divan` for new benchmarks. Run relevant benchmarks with `just bench`; `just bench-smoke` runs a single iteration when a benchmark smoke check is needed.

## TUI code

Follow [TUI styles](codex-rs/tui/styles.md) and applicable nested `AGENTS.md` files.

- Prefer ratatui `Stylize` helpers (`.dim()`, `.bold()`, `.cyan()`, etc.) over manual `Style` construction; chain helpers when clearer.
- Use `"text".into()` for unstyled spans and `vec![…].into()` for lines when the type is clear. Use `Span::from` or `Line::from` when inference is ambiguous.
- Computed styles may use `Span::styled` or `.set_style()`.
- Do not hardcode white; use the default foreground.
- Follow local conventions; do not churn equivalent style/conversion forms or add type annotations solely to use `.into()`.
- Prefer the form that stays on one line after formatting; when both wrap, choose fewer wrapped lines.
- Wrap plain text with `textwrap::wrap` and ratatui lines with helpers in `tui/src/wrapping.rs` (`word_wrap_line` / `word_wrap_lines`).
- Use `RtOptions` initial/subsequent indentation rather than custom wrapping logic, and `line_utils::prefix_lines` to prefix lists of lines.

## App-server protocol

These constraints apply especially to `app-server-protocol/src/protocol/common.rs` and `v2.rs`.

- Add active API surface only to v2, not v1.
- Name request, response, and notification payloads `*Params`, `*Response`, and `*Notification`.
- Use singular `<resource>/<method>` RPC names, such as `thread/read`.
- Use camelCase fields and string enum values on the wire, with aligned serde and TypeScript renames. Config RPC payloads use snake_case to match config keys; preserve explicit compatibility exceptions.
- Set `#[ts(export_to = "v2/")]` on v2 request, response, and notification types.
- Do not skip `None` fields in v2 payloads. Exception: no-params client requests may use `params: Option<()>` with `#[ts(type = "undefined")]` and `#[serde(skip_serializing_if = "Option::is_none")]`.
- Keep explicit serde/TS field and variant renames aligned. Tag discriminated unions in both serializers with `#[serde(tag = "type", ...)]` and `#[ts(tag = "type", ...)]`.
- Use plain `String` IDs at API boundaries; parse UUIDs internally when needed.
- Use integer Unix seconds (`i64`) for timestamps, named `*_at`.
- Mark experimental surface with `#[experimental("method/or/field")]`; derive `ExperimentalApi` for field-level gating and use `inspect_params: true` when only some method fields are experimental.

### Request fields and pagination

- Annotate optional request fields with `#[ts(optional = nullable)]`; do not use that annotation outside client-to-server `*Params` types.
- Optional collections use `Option<...>` plus `#[ts(optional = nullable)]`, not `#[serde(default)]` or skipped fields.
- For a boolean whose omission means false, prefer `bool` with `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`, not `Option<bool>`.
- New list methods use cursor pagination: request `cursor: Option<String>` and `limit: Option<u32>`; response `data: Vec<...>` and `next_cursor: Option<String>`.

### Schemas and coverage

After API shape changes, run `just write-app-server-schema`; also use `--experimental` when experimental fixtures are affected. Validate changed behavior with focused `codex-app-server-protocol` checks under the fork guide. Rely on schema generation and behavioral tests, not boilerplate tests checking individual experimental markers.

## Python and documentation

Use Python 3; do not add Python 2 compatibility or `__future__` imports. Check the nearest `pyproject.toml` `requires-python` when minimum-version support matters.

Do not add general product/user documentation to `docs/`. Keep fork intent in the fork guide; app-server API documentation is also permitted there.
