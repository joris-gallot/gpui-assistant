use gpui::{
  AnyElement, App, ElementId, IntoElement, RenderOnce, SharedString, Window, div, prelude::*,
};
use gpui_assistant_core::{MessagePart, Role, ToolCall, ToolCallStatus, ToolResult};

use crate::style::AssistantColors;

const MAX_OUTPUT_LINES: usize = 8;

#[derive(Clone, Debug, IntoElement)]
pub struct MessageView {
  id: SharedString,
  role: Role,
  parts: Vec<MessagePart>,
  is_streaming: bool,
}

impl MessageView {
  pub fn new(id: impl Into<SharedString>, role: Role, parts: Vec<MessagePart>) -> Self {
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

impl RenderOnce for MessageView {
  fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
    let colors = AssistantColors::new(cx);
    let Self {
      id,
      role,
      parts,
      is_streaming,
    } = self;

    let label = match role {
      Role::User => "You",
      Role::Assistant => "Assistant",
      Role::System => "System",
      Role::Tool => "Tool",
    };
    let last = parts.len().saturating_sub(1);
    let blocks = parts.into_iter().enumerate().map(|(index, part)| {
      let block_id = ElementId::from(SharedString::from(format!("{id}-{index}")));

      match part {
        MessagePart::Text { mut text } => {
          if is_streaming && index == last {
            text.push('▌');
          }

          markdown(block_id, text)
        }
        MessagePart::Thinking { text, .. } => div()
          .flex()
          .flex_col()
          .gap_1()
          .text_color(colors.muted_foreground)
          .child(div().text_sm().child("Thinking"))
          .child(markdown(block_id, text))
          .into_any_element(),
        MessagePart::RedactedThinking { .. } => div()
          .text_sm()
          .text_color(colors.muted_foreground)
          .child("Redacted thinking")
          .into_any_element(),
        MessagePart::ToolCall(call) => tool_call(call, &colors),
        MessagePart::ToolResult(result) => tool_result(result, &colors),
        MessagePart::Attachment(attachment) => div()
          .text_sm()
          .text_color(colors.muted_foreground)
          .child(format!("Attachment: {}", attachment.name))
          .into_any_element(),
      }
    });

    div()
      .flex()
      .flex_col()
      .gap_2()
      .w_full()
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
      .children(blocks)
  }
}

fn tool_call(call: ToolCall, colors: &AssistantColors) -> AnyElement {
  let (marker, color) = match call.status {
    ToolCallStatus::Pending => ("…", colors.muted_foreground),
    ToolCallStatus::Running => ("▶", colors.muted_foreground),
    ToolCallStatus::Finished => ("✓", colors.foreground),
    ToolCallStatus::Failed => ("✗", colors.danger),
  };

  div()
    .flex()
    .gap_2()
    .w_full()
    .child(div().text_color(color).child(marker))
    .child(div().child(call.name))
    .into_any_element()
}

fn tool_result(result: ToolResult, colors: &AssistantColors) -> AnyElement {
  let color = if result.is_error {
    colors.danger
  } else {
    colors.muted_foreground
  };

  div()
    .w_full()
    .px_2()
    .py_1()
    .rounded(colors.radius)
    .border_1()
    .border_color(colors.border)
    .text_sm()
    .text_color(color)
    .child(clamp_output(&result.output))
    .into_any_element()
}

/// Tool output is unbounded, and a message carrying a whole file would push everything
/// else out of the viewport.
fn clamp_output(output: &str) -> String {
  let mut lines = output.lines();
  let kept = lines.by_ref().take(MAX_OUTPUT_LINES).collect::<Vec<_>>();
  let dropped = lines.count();
  let mut clamped = kept.join("\n");

  if dropped > 0 {
    clamped.push_str(&format!("\n… {dropped} more lines"));
  }

  clamped
}

#[cfg(feature = "gpui-component")]
fn markdown(id: ElementId, text: String) -> AnyElement {
  gpui_component::text::TextView::markdown(id, text).into_any_element()
}

#[cfg(not(feature = "gpui-component"))]
fn markdown(_id: ElementId, text: String) -> AnyElement {
  div().child(text).into_any_element()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn short_output_is_kept_whole() {
    assert_eq!(clamp_output("one\ntwo"), "one\ntwo");
  }

  #[test]
  fn long_output_reports_what_it_dropped() {
    let output = (1..=12)
      .map(|line| line.to_string())
      .collect::<Vec<_>>()
      .join("\n");

    assert_eq!(
      clamp_output(&output),
      "1\n2\n3\n4\n5\n6\n7\n8\n… 4 more lines"
    );
  }
}
