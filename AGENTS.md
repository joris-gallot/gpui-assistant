# AGENTS.md

Assistant UI primitives for GPUI apps. Cargo workspace, edition 2024.

## Layout

| Crate | Role |
| --- | --- |
| `crates/core` (`gpui-assistant-core`) | Headless: `Thread`, `Message`, `AssistantEvent`, `AssistantRuntime`, `EchoRuntime`. No GPUI dependency, and it must stay that way. |
| `crates/ui` (`gpui-assistant-ui`) | GPUI components: `AssistantThread` (entity), `AssistantView` (render), `MessageView`. |
| `crates/acp` (`gpui-assistant-acp`) | `AcpRuntime`: spawns an Agent Client Protocol agent, maps its session updates to `AssistantEvent`. |
| `crates/openai-compatible` | Stub adapter, not implemented yet. |
| `examples/basic-chat` | Echo runtime, no agent to install. |
| `examples/acp-chat` | Real ACP agent (`claude`, `codex`, or any command string). |

## Commands

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p gpui-assistant-ui --no-default-features --locked
cargo run -p basic-chat
cargo run -p acp-chat -- codex
```

CI runs exactly those four checks on Linux and macOS. Clippy warnings are errors.

## Architecture rules

- Events are the only way state changes: a runtime yields an `AssistantEventStream`, `Thread::apply_event` reduces it. Never mutate a `Thread` from the UI directly.
- `AssistantRuntime` implementors own their transport. `AcpRuntime` drives its connection on a dedicated thread and talks to it through an unbounded command channel.
- ACP callbacks run on the dispatch loop, so anything that waits (permissions, terminal creation, terminal exit) is parked with its `Responder` and answered later. Awaiting inside a callback stalls the whole connection.
- Every parked responder must be answered, including when the turn ends: an unanswered responder hangs the agent.
- `crates/ui` has an optional `gpui-component` feature, on by default. Guard every use with `#[cfg(feature = "gpui-component")]` and keep a raw-gpui fallback compiling. Colors come from `AssistantColors`, never hardcoded in a view.

## Conventions

- `rustfmt.toml` sets `tab_spaces = 2`. Run `cargo fmt` rather than hand-formatting.
- Dependencies go through `[workspace.dependencies]`, crates use `foo.workspace = true`. `gpui` comes from the Zed git repo, so anything pulling `gpui` from crates.io will not unify.
- Comments: one line, the WHY only. Write none when the code says it.
- Tests are full sentences: `a_non_zero_exit_marks_the_terminal_call_failed`. They live in a bottom `mod tests`, or in a sibling `tests.rs` when the module is large (`mapping/tests.rs`, `terminal/tests.rs`).
- GPUI tests use `#[gpui::test]` with `TestAppContext` and `cx.run_until_parked()`.
- Commits: conventional, lowercase, with a scope. `feat(acp): ...`, `fix(ui): ...`, `test(ui): ...`, `ci: ...`, `!` for a breaking change.
