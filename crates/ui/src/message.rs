use gpui::{App, ElementId, IntoElement, RenderOnce, Window, div, prelude::*};
use gpui_assistant_core::{MessagePart, Role};

use crate::style::AssistantColors;

#[derive(Clone, Debug, IntoElement)]
pub struct MessageView {
  id: ElementId,
  role: Role,
  parts: Vec<MessagePart>,
  is_streaming: bool,
}

impl MessageView {
  pub fn new(id: impl Into<ElementId>, role: Role, parts: Vec<MessagePart>) -> Self {
    Self {
      id: id.into(),
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

#[cfg(feature = "gpui-component")]
fn body(id: ElementId, text: String) -> impl IntoElement {
  gpui_component::text::TextView::markdown(id, text)
}

#[cfg(not(feature = "gpui-component"))]
fn body(_id: ElementId, text: String) -> impl IntoElement {
  div().child(text)
}

impl RenderOnce for MessageView {
  fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
    let colors = AssistantColors::new(cx);
    let label = match self.role {
      Role::User => "You",
      Role::Assistant => "Assistant",
      Role::System => "System",
      Role::Tool => "Tool",
    };

    let mut text = self
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
      text.push('▌');
    }

    div()
      .flex()
      .flex_col()
      .gap_1()
      .p_3()
      .rounded(colors.radius)
      .border_1()
      .border_color(colors.border)
      .bg(colors.muted)
      .child(
        div()
          .text_sm()
          .text_color(colors.muted_foreground)
          .child(label),
      )
      .child(
        div()
          .text_color(colors.foreground)
          .child(body(self.id, text)),
      )
  }
}
