use std::pin::Pin;

use futures_core::Stream;
use futures_util::stream;
use serde::{Deserialize, Serialize};

pub type AssistantEventStream<'a> = Pin<Box<dyn Stream<Item = AssistantEvent> + Send + 'a>>;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Thread {
  pub id: ThreadId,
  pub messages: Vec<Message>,
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ThreadId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Message {
  pub id: MessageId,
  pub role: Role,
  pub parts: Vec<MessagePart>,
}

impl Thread {
  pub fn apply_event(&mut self, event: AssistantEvent) {
    match event {
      AssistantEvent::MessageStarted { message } => self.messages.push(message),
      AssistantEvent::TextDelta { message_id, delta } => {
        if let Some(message) = self.message_mut(&message_id) {
          message.push_text_delta(delta);
        }
      }
      AssistantEvent::ThinkingDelta { message_id, delta } => {
        if let Some(message) = self.message_mut(&message_id) {
          message.push_thinking_delta(delta);
        }
      }
      AssistantEvent::ToolCallStarted { message_id, call }
      | AssistantEvent::ToolCallUpdated { message_id, call } => {
        if let Some(message) = self.message_mut(&message_id) {
          message.upsert_tool_call(call);
        }
      }
      AssistantEvent::ToolCallFinished { message_id, result } => {
        if let Some(message) = self.message_mut(&message_id) {
          message.finish_tool_call(&result.call_id);
          message.parts.push(MessagePart::ToolResult(result));
        }
      }
      AssistantEvent::MessageFinished { .. } | AssistantEvent::Error { .. } => {}
    }
  }

  fn message_mut(&mut self, message_id: &MessageId) -> Option<&mut Message> {
    self
      .messages
      .iter_mut()
      .find(|message| &message.id == message_id)
  }
}

impl Message {
  pub fn new(id: impl Into<String>, role: Role) -> Self {
    Self {
      id: MessageId(id.into()),
      role,
      parts: Vec::new(),
    }
  }

  pub fn user(id: impl Into<String>, text: impl Into<String>) -> Self {
    Self {
      id: MessageId(id.into()),
      role: Role::User,
      parts: vec![MessagePart::Text { text: text.into() }],
    }
  }

  pub fn assistant(id: impl Into<String>, text: impl Into<String>) -> Self {
    Self {
      id: MessageId(id.into()),
      role: Role::Assistant,
      parts: vec![MessagePart::Text { text: text.into() }],
    }
  }

  fn push_text_delta(&mut self, delta: String) {
    match self.parts.last_mut() {
      Some(MessagePart::Text { text }) => text.push_str(&delta),
      _ => self.parts.push(MessagePart::Text { text: delta }),
    }
  }

  fn push_thinking_delta(&mut self, delta: String) {
    match self.parts.last_mut() {
      Some(MessagePart::Thinking { text, .. }) => text.push_str(&delta),
      _ => self.parts.push(MessagePart::Thinking {
        text: delta,
        signature: None,
      }),
    }
  }

  fn upsert_tool_call(&mut self, call: ToolCall) {
    if let Some(part) = self.parts.iter_mut().find(|part| match part {
      MessagePart::ToolCall(existing) => existing.id == call.id,
      _ => false,
    }) {
      *part = MessagePart::ToolCall(call);
    } else {
      self.parts.push(MessagePart::ToolCall(call));
    }
  }

  fn finish_tool_call(&mut self, call_id: &ToolCallId) {
    for part in &mut self.parts {
      let MessagePart::ToolCall(call) = part else {
        continue;
      };

      if &call.id == call_id {
        call.status = ToolCallStatus::Finished;
        break;
      }
    }
  }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct MessageId(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
  User,
  Assistant,
  System,
  Tool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagePart {
  Text {
    text: String,
  },
  Thinking {
    text: String,
    signature: Option<String>,
  },
  RedactedThinking {
    data: String,
  },
  ToolCall(ToolCall),
  ToolResult(ToolResult),
  Attachment(Attachment),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
  pub id: AttachmentId,
  pub name: String,
  pub kind: AttachmentKind,
  pub content: Option<String>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct AttachmentId(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
  File,
  Directory,
  Image,
  Url,
  Selection,
  Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
  pub id: ToolCallId,
  pub name: String,
  pub input: String,
  pub status: ToolCallStatus,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ToolCallId(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
  PendingApproval,
  Running,
  Finished,
  Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
  pub call_id: ToolCallId,
  pub output: String,
  pub is_error: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserInput {
  pub thread_id: ThreadId,
  pub text: String,
  pub attachments: Vec<Attachment>,
}

impl UserInput {
  pub fn text(thread_id: impl Into<String>, text: impl Into<String>) -> Self {
    Self {
      thread_id: ThreadId(thread_id.into()),
      text: text.into(),
      attachments: Vec::new(),
    }
  }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantEvent {
  MessageStarted {
    message: Message,
  },
  TextDelta {
    message_id: MessageId,
    delta: String,
  },
  ThinkingDelta {
    message_id: MessageId,
    delta: String,
  },
  ToolCallStarted {
    message_id: MessageId,
    call: ToolCall,
  },
  ToolCallUpdated {
    message_id: MessageId,
    call: ToolCall,
  },
  ToolCallFinished {
    message_id: MessageId,
    result: ToolResult,
  },
  MessageFinished {
    message_id: MessageId,
  },
  Error {
    message: String,
  },
}

pub trait AssistantRuntime: Send + Sync + 'static {
  fn send(&self, input: UserInput) -> AssistantEventStream<'_>;
  fn cancel(&self, thread_id: &ThreadId);
}

#[derive(Clone, Debug)]
pub struct EchoRuntime {
  response_prefix: String,
}

impl Default for EchoRuntime {
  fn default() -> Self {
    Self::new("Echo: ")
  }
}

impl EchoRuntime {
  pub fn new(response_prefix: impl Into<String>) -> Self {
    Self {
      response_prefix: response_prefix.into(),
    }
  }
}

impl AssistantRuntime for EchoRuntime {
  fn send(&self, input: UserInput) -> AssistantEventStream<'_> {
    let message_id = MessageId(format!("{}-assistant", input.thread_id.0));
    let response = format!("{}{}", self.response_prefix, input.text);

    Box::pin(stream::iter(vec![
      AssistantEvent::MessageStarted {
        message: Message::new(message_id.0.clone(), Role::Assistant),
      },
      AssistantEvent::TextDelta {
        message_id: message_id.clone(),
        delta: response,
      },
      AssistantEvent::MessageFinished { message_id },
    ]))
  }

  fn cancel(&self, _thread_id: &ThreadId) {}
}

#[cfg(test)]
mod tests;
