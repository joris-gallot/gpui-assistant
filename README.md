# gpui-assistant

Assistant UI primitives for GPUI apps.

This project is headless-first: the core runtime and message model are independent from GPUI, while the UI crate provides native GPUI components.

## Crates

- `gpui-assistant-core`: threads, messages, events, runtime traits
- `gpui-assistant-ui`: GPUI components
- `gpui-assistant-acp`: runtime backed by an Agent Client Protocol agent
- `gpui-assistant-openai-compatible`: OpenAI-compatible runtime adapter, still a stub

## Examples

- `cargo run -p basic-chat`: echo runtime, no agent to install
- `cargo run -p acp-chat -- "npx -y @agentclientprotocol/claude-agent-acp@latest"`: any ACP agent command, defaulting to the Claude adapter

## Status

Experimental scaffold.
