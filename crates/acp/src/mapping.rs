use std::collections::HashMap;

use agent_client_protocol::schema::v1 as acp;
use gpui_assistant_core::{
  AssistantEvent, Message, MessageId, PermissionOption, PermissionOptionId, PermissionOptionKind,
  PermissionRequest, PermissionRequestId, Role, ToolCall, ToolCallId, ToolCallStatus, ToolResult,
};

use crate::terminal::Terminals;

pub(crate) struct Turn {
  session: String,
  index: u64,
  message_id: Option<MessageId>,
  calls: HashMap<ToolCallId, CallState>,
}

/// ACP reports a call's content before it reports completion, so the latest content is
/// held here until the terminal update turns it into a tool result.
struct CallState {
  call: ToolCall,
  output: String,
}

impl Turn {
  pub(crate) fn new(session: impl Into<String>) -> Self {
    Self {
      session: session.into(),
      index: 0,
      message_id: None,
      calls: HashMap::new(),
    }
  }

  pub(crate) fn apply(
    &mut self,
    update: acp::SessionUpdate,
    terminals: &Terminals,
  ) -> Vec<AssistantEvent> {
    let mut events = Vec::new();

    match update {
      acp::SessionUpdate::AgentMessageChunk(chunk) => {
        let message_id = self.open(&mut events);

        events.push(AssistantEvent::TextDelta {
          message_id,
          delta: content_text(&chunk.content),
        });
      }
      acp::SessionUpdate::AgentThoughtChunk(chunk) => {
        let message_id = self.open(&mut events);

        events.push(AssistantEvent::ThinkingDelta {
          message_id,
          delta: content_text(&chunk.content),
        });
      }
      acp::SessionUpdate::ToolCall(call) => {
        let message_id = self.open(&mut events);
        let output = content_output(&call.content, terminals);
        let call = ToolCall {
          id: ToolCallId(call.tool_call_id.0.to_string()),
          name: call.title,
          input: call
            .raw_input
            .map(|input| input.to_string())
            .unwrap_or_default(),
          status: tool_call_status(call.status),
        };

        self.calls.insert(
          call.id.clone(),
          CallState {
            call: call.clone(),
            output,
          },
        );
        events.push(AssistantEvent::ToolCallStarted { message_id, call });
      }
      acp::SessionUpdate::ToolCallUpdate(update) => {
        let message_id = self.open(&mut events);
        let call_id = ToolCallId(update.tool_call_id.0.to_string());
        // ACP sends partial updates, so merge onto the snapshot we already hold.
        let state = self.calls.entry(call_id.clone()).or_insert(CallState {
          call: ToolCall {
            id: call_id.clone(),
            name: String::new(),
            input: String::new(),
            status: ToolCallStatus::Pending,
          },
          output: String::new(),
        });

        if let Some(title) = update.fields.title {
          state.call.name = title;
        }
        if let Some(status) = update.fields.status {
          state.call.status = tool_call_status(status);
        }
        if let Some(content) = update.fields.content {
          state.output = content_output(&content, terminals);
        }

        events.push(AssistantEvent::ToolCallUpdated {
          message_id: message_id.clone(),
          call: state.call.clone(),
        });

        if matches!(
          state.call.status,
          ToolCallStatus::Finished | ToolCallStatus::Failed
        ) {
          events.push(AssistantEvent::ToolCallFinished {
            message_id,
            result: ToolResult {
              call_id,
              output: state.output.clone(),
              is_error: state.call.status == ToolCallStatus::Failed,
            },
          });
        }
      }
      // Nothing to show yet for plans, modes, commands or config, and the enum is
      // non_exhaustive so unknown updates land here too.
      _ => {}
    }

    events
  }

  pub(crate) fn end(&mut self, stop_reason: acp::StopReason) -> Vec<AssistantEvent> {
    let mut events = Vec::new();

    if let Some(message) = stop_reason_error(stop_reason) {
      events.push(AssistantEvent::Error { message });
    } else if let Some(message_id) = self.message_id.clone() {
      events.push(AssistantEvent::MessageFinished { message_id });
    }

    self.reset();

    events
  }

  pub(crate) fn fail(&mut self, message: impl Into<String>) -> Vec<AssistantEvent> {
    self.reset();

    vec![AssistantEvent::Error {
      message: message.into(),
    }]
  }

  pub(crate) fn open(&mut self, events: &mut Vec<AssistantEvent>) -> MessageId {
    if let Some(message_id) = &self.message_id {
      return message_id.clone();
    }

    // ACP has no "message started" update, so the first chunk of a turn opens one.
    let message_id = MessageId(format!("{}-assistant-{}", self.session, self.index));
    events.push(AssistantEvent::MessageStarted {
      message: Message::new(message_id.0.clone(), Role::Assistant),
    });
    self.message_id = Some(message_id.clone());

    message_id
  }

  pub(crate) fn call_name(&self, call_id: &ToolCallId) -> Option<String> {
    self
      .calls
      .get(call_id)
      .map(|state| state.call.name.clone())
      .filter(|name| !name.is_empty())
  }

  fn reset(&mut self) {
    self.message_id = None;
    self.calls.clear();
    self.index += 1;
  }
}

pub(crate) fn permission_request(
  id: PermissionRequestId,
  label: String,
  request: &acp::RequestPermissionRequest,
) -> PermissionRequest {
  PermissionRequest {
    id,
    label,
    call_id: Some(ToolCallId(request.tool_call.tool_call_id.0.to_string())),
    options: request
      .options
      .iter()
      .map(|option| PermissionOption {
        id: PermissionOptionId(option.option_id.0.to_string()),
        name: option.name.clone(),
        kind: permission_option_kind(option.kind),
      })
      .collect(),
  }
}

/// What the user reads before allowing a command to run, so it must show the whole thing.
pub(crate) fn terminal_label(request: &acp::CreateTerminalRequest) -> String {
  std::iter::once(request.command.clone())
    .chain(request.args.iter().cloned())
    .collect::<Vec<_>>()
    .join(" ")
}

fn permission_option_kind(kind: acp::PermissionOptionKind) -> PermissionOptionKind {
  match kind {
    acp::PermissionOptionKind::AllowOnce => PermissionOptionKind::AllowOnce,
    acp::PermissionOptionKind::AllowAlways => PermissionOptionKind::AllowAlways,
    acp::PermissionOptionKind::RejectOnce => PermissionOptionKind::RejectOnce,
    acp::PermissionOptionKind::RejectAlways => PermissionOptionKind::RejectAlways,
    // Never style an unknown kind as an allow button.
    _ => PermissionOptionKind::RejectOnce,
  }
}

fn tool_call_status(status: acp::ToolCallStatus) -> ToolCallStatus {
  match status {
    acp::ToolCallStatus::Pending => ToolCallStatus::Pending,
    acp::ToolCallStatus::InProgress => ToolCallStatus::Running,
    acp::ToolCallStatus::Completed => ToolCallStatus::Finished,
    acp::ToolCallStatus::Failed => ToolCallStatus::Failed,
    _ => ToolCallStatus::Pending,
  }
}

fn stop_reason_error(stop_reason: acp::StopReason) -> Option<String> {
  match stop_reason {
    acp::StopReason::EndTurn | acp::StopReason::Cancelled => None,
    acp::StopReason::MaxTokens => Some("The agent reached its token limit".into()),
    acp::StopReason::MaxTurnRequests => Some("The agent reached its request limit".into()),
    acp::StopReason::Refusal => Some("The agent refused to continue".into()),
    other => Some(format!("The agent stopped: {other:?}")),
  }
}

fn content_text(content: &acp::ContentBlock) -> String {
  match content {
    acp::ContentBlock::Text(text) => text.text.clone(),
    acp::ContentBlock::Image(_) => "[image]".into(),
    acp::ContentBlock::Audio(_) => "[audio]".into(),
    other => format!("[{}]", block_label(other)),
  }
}

fn block_label(content: &acp::ContentBlock) -> &'static str {
  match content {
    acp::ContentBlock::Text(_) => "text",
    acp::ContentBlock::Image(_) => "image",
    acp::ContentBlock::Audio(_) => "audio",
    _ => "unsupported content",
  }
}

fn content_output(content: &[acp::ToolCallContent], terminals: &Terminals) -> String {
  content
    .iter()
    .map(|content| tool_call_content_text(content, terminals))
    .filter(|text| !text.is_empty())
    .collect::<Vec<_>>()
    .join("\n")
}

fn tool_call_content_text(content: &acp::ToolCallContent, terminals: &Terminals) -> String {
  match content {
    acp::ToolCallContent::Content(content) => content_text(&content.content),
    acp::ToolCallContent::Diff(diff) => {
      format!("{}\n{}", diff.path.display(), diff.new_text)
    }
    // The agent embeds a terminal by id; we are the one running it, so read our buffer.
    acp::ToolCallContent::Terminal(terminal) => terminals
      .text(terminal.terminal_id.0.as_ref())
      .unwrap_or_default(),
    _ => "[unsupported tool content]".into(),
  }
}

#[cfg(test)]
impl Turn {
  /// Most updates carry no terminal reference, so tests skip the registry.
  fn apply_without_terminals(&mut self, update: acp::SessionUpdate) -> Vec<AssistantEvent> {
    self.apply(update, &Terminals::default())
  }
}

#[cfg(test)]
mod tests;
