# Rust port (branch `rust-port`)

A port of dictate from C#/.NET to Rust, done core first. The C# code under
`src/` is still what ships; nothing here is wired into a release.

| Crate | Ports | State |
|---|---|---|
| `dictate-core` | `src/Dictate.Core` | Ported, with every host test from `tests/Dictate.Core.Tests` |
| `dictate-windows` | `src/Dictate.Windows` | Not started |

The rule from `CLAUDE.md` carries over: **`dictate-core` must not touch a
Windows-only API.** CI compiles and tests it on `ubuntu-latest`.

## Where it deliberately differs from the C# core

- **Synchronous.** The pipeline runs on a worker thread, one utterance at a
  time, so `reqwest::blocking` replaces async/await and there is no runtime.
  Cancellation is by timeout only. The C# `CancellationToken` was used for
  shutdown, which the Windows head will handle by dropping the worker.
- **No Anthropic SDK.** Cleanup posts to the Messages API directly. The request
  body and response parsing are pure functions with their own tests, which
  also covers FR-11.8 (temperature 0) at the host tier for the first time.
- **Config enums** are read by name, case-insensitively, as before. Numeric
  enum values, which System.Text.Json also accepted, are not.

## Running

```
cd rust
cargo test
```

There is no Rust toolchain on the development machine either; like the .NET
build, this compiles in CI (`rust-core` job in `.github/workflows/ci.yml`).
