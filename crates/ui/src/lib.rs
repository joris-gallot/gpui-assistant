use gpui::{App, IntoElement, RenderOnce, Window, div, prelude::*};
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
    self.thread.messages.into_iter().fold(
      div().flex().flex_col().gap_2().size_full(),
      |element, message| element.child(MessageView::new(message.role, message.parts)),
    )
  }
}

#[derive(Clone, Debug, IntoElement)]
pub struct MessageView {
  role: Role,
  parts: Vec<MessagePart>,
}

impl MessageView {
  pub fn new(role: Role, parts: Vec<MessagePart>) -> Self {
    Self { role, parts }
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

    let body = self
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

    div()
      .flex()
      .flex_col()
      .gap_1()
      .p_3()
      .rounded_md()
      .border_1()
      .child(div().text_sm().child(label))
      .child(div().child(body))
  }
}
