# AGENTS.md — Rust Repo Guidelines for AI Coding Agents

This document defines how to work in this repository. Follow it exactly.

## Goals

- Keep changes small, correct, and idiomatic Rust.
- Preserve existing public APIs unless explicitly asked to change them.
- Prefer clarity and safety over cleverness.

## Repo Orientation

- Start by reading: `README.md`, `Cargo.toml`, and module-level docs in `src/`.
- Identify crate type (bin/lib), feature flags, and MSRV (minimum supported Rust version).
- Do not introduce new workspace members or split crates without request.

## Build, Test, and Lint

Always run the narrowest relevant checks first, then the full suite.

### Fast checks (use first)

- `cargo fmt`
- `cargo clippy --all-targets --all-features -D warnings`
- `cargo test --all-features`

### If workspace / multiple crates

- `cargo test --workspace --all-features`
- `cargo clippy --workspace --all-targets --all-features -D warnings`

### Benchmarks (only if requested)

- `cargo bench`

If CI uses a specific toolchain, use it (see `rust-toolchain.toml` if present).

## Rust Style and Conventions

- Formatting: `rustfmt` defaults. Do not hand-format.
- Linting: treat clippy warnings as errors. If you must allow a lint, scope it narrowly and explain why.
- Use idiomatic types:
  - Prefer `&str` over `String` in parameters when ownership isn’t needed.
  - Prefer iterators over indexing where practical.
  - Prefer `Option<T>` / `Result<T, E>` over sentinel values.

## Error Handling

- Don’t use `unwrap()` / `expect()` in library code.
- In binaries, `expect()` is acceptable only with a helpful message when failure is unrecoverable.
- Prefer `thiserror` for library error enums if already in use.
- If the repo uses `anyhow`, use it for application-level error aggregation only.
- Propagate errors with `?` and keep context meaningful.

## Dependencies

- Do not add new dependencies unless necessary.
- Before adding a crate:
  - Check if the repo already has a preferred crate for the purpose.
  - Prefer widely used, maintained crates.
  - Keep dependency surface minimal (avoid heavy transitive deps).
- Never add networked build steps or downloading in build scripts.

## Performance and Allocation

- Do not prematurely optimize, but avoid obvious regressions.
- Avoid unnecessary allocations:
  - Use `Cow<'a, str>` where appropriate for borrowed/owned flexibility.
  - Use `&[T]` for slices rather than `Vec<T>` params.
- If touching hot paths, include a short note explaining performance impact.

## Concurrency and Async

Follow existing patterns in the repo.

- If async is used:
  - Do not mix runtimes (Tokio/async-std) without request.
  - Prefer `tokio::spawn` patterns already used.
- For sync concurrency:
  - Prefer message passing or scoped concurrency if already established.
- Avoid introducing deadlocks (watch lock ordering, avoid holding locks across `.await`).

## Unsafe Code

- Avoid `unsafe` unless there is no reasonable alternative.
- If you add `unsafe`:
  - Minimize the unsafe block.
  - Add a comment explaining invariants and why it’s sound.
  - Prefer encapsulating unsafe behind a safe API.

## Logging and Instrumentation

- Use the repo’s existing logging stack (`log`, `tracing`, etc.).
- Do not add verbose logs by default.
- Prefer structured fields with `tracing` if used.

## Testing Guidance

- Add tests for new behavior and bug fixes.
- Prefer:
  - Unit tests near the module (`src/...` with `#[cfg(test)]`)
  - Integration tests in `tests/` for public API behavior
- Tests must be deterministic:
  - Avoid relying on wall-clock time, random seeds without fixed seeds, or network.
- If changing parsing/formatting:
  - Include round-trip tests and edge cases.

## Documentation

When behavior changes, update:

- `README.md` (if user-facing)
- rustdoc examples (if present)
- module docs (`//!`) where applicable

## What to Provide in Your Output

When implementing a change, include:

1. A brief summary of what changed and why.
2. Commands to run to verify (`cargo fmt`, `cargo clippy ...`, `cargo test ...`).
3. Any follow-ups or risks (if applicable).

## What NOT to Do

- Do not reformat unrelated code.
- Do not do broad refactors unless requested.
- Do not change licensing, CI, or toolchain config unless requested.
- Do not add telemetry, network calls, or external services.

## File Layout Expectations (default)

If the repo doesn’t already specify layout, prefer:

- `src/lib.rs` as the crate root for libs
- `src/main.rs` for bins
- `src/bin/*.rs` for multiple binaries
- `src/<module>.rs` or `src/<module>/mod.rs` consistently (match existing style)

## If Instructions Conflict

- Follow the repo’s existing conventions over generic Rust advice.
- Follow CI/tooling configurations in the repo over this document.
- If still ambiguous, choose the smallest safe change and document assumptions.
