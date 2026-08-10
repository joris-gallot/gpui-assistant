# gpui-assistant

Assistant UI primitives for GPUI apps.

This project is headless-first: the core runtime and message model are independent from GPUI, while the UI crate provides native GPUI components.

## Planned crates

- `gpui-assistant-core`: threads, messages, events, runtime traits
- `gpui-assistant-ui`: GPUI components
- `gpui-assistant-openai-compatible`: OpenAI-compatible runtime adapter
- `gpui-assistant-acp`: optional ACP adapter, planned

## Status

Experimental scaffold.
