use std::pin::Pin;

use futures_core::Stream;
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

impl Message {
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
