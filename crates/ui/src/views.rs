use gpui::{App, IntoElement, RenderOnce, Window, div, prelude::*, rgb};
use gpui_assistant_core::{MessagePart, Role, Thread};

#[derive(Clone, Debug, IntoElement)]
pub struct AssistantView {
  thread: Thread,
}

impl AssistantView {
  pub fn new(thread: Thread) -> Self {
    Self { thread }
  }
}

impl RenderOnce for AssistantView {
  fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
    div()
      .flex()
      .flex_col()
      .gap_3()
      .size_full()
      .bg(rgb(0xffffff))
      .text_color(rgb(0x111827))
      .child(ThreadView::new(self.thread))
  }
}

#[derive(Clone, Debug, IntoElement)]
pub struct ThreadView {
  thread: Thread,
}

impl ThreadView {
  pub fn new(thread: Thread) -> Self {
    Self { thread }
  }
}

impl RenderOnce for ThreadView {
  fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
    let streaming = self.thread.streaming_message_id().cloned();

    self.thread.messages.into_iter().fold(
      div().flex().flex_col().gap_2().size_full(),
      |element, message| {
        let is_streaming = streaming.as_ref() == Some(&message.id);

        element.child(MessageView::new(message.role, message.parts).streaming(is_streaming))
      },
    )
  }
}

#[derive(Clone, Debug, IntoElement)]
pub struct MessageView {
  role: Role,
  parts: Vec<MessagePart>,
  is_streaming: bool,
}

impl MessageView {
  pub fn new(role: Role, parts: Vec<MessagePart>) -> Self {
    Self {
      role,
      parts,
      is_streaming: false,
    }
  }

  pub fn streaming(mut self, is_streaming: bool) -> Self {
    self.is_streaming = is_streaming;
    self
  }
}

impl RenderOnce for MessageView {
  fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
    let label = match self.role {
      Role::User => "You",
      Role::Assistant => "Assistant",
      Role::System => "System",
      Role::Tool => "Tool",
    };

    let mut body = self
      .parts
      .into_iter()
      .map(|part| match part {
        MessagePart::Text { text } => text,
        MessagePart::Thinking { text, .. } => format!("Thinking: {text}"),
        MessagePart::RedactedThinking { .. } => "Redacted thinking".to_string(),
        MessagePart::ToolCall(call) => format!("Tool call: {}", call.name),
        MessagePart::ToolResult(result) => result.output,
        MessagePart::Attachment(attachment) => format!("Attachment: {}", attachment.name),
      })
      .collect::<Vec<_>>()
      .join("\n");

    if self.is_streaming {
      body.push('▌');
    }

    div()
      .flex()
      .flex_col()
      .gap_1()
      .p_3()
      .rounded_md()
      .border_1()
      .border_color(rgb(0xe5e7eb))
      .bg(rgb(0xf9fafb))
      .child(div().text_sm().text_color(rgb(0x6b7280)).child(label))
      .child(div().text_color(rgb(0x111827)).child(body))
  }
}
