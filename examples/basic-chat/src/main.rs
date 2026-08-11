use std::sync::Arc;

use gpui::{
  App, Bounds, Context, Entity, IntoElement, Render, Window, WindowBounds, WindowOptions, div,
  prelude::*, px, rgb, size,
};
use gpui_assistant_core::{EchoRuntime, Thread, ThreadId, ThreadStatus};
use gpui_assistant_ui::{AssistantThread, AssistantView};
use gpui_platform::application;

struct BasicChat {
  assistant: Entity<AssistantThread>,
}

impl Render for BasicChat {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let thread = self.assistant.read(cx).thread().clone();
    let status = match &thread.status {
      ThreadStatus::Idle => "Idle".to_string(),
      ThreadStatus::Generating => "Generating".to_string(),
      ThreadStatus::Error { message } => format!("Error: {message}"),
    };

    div()
      .flex()
      .flex_col()
      .gap_3()
      .size_full()
      .p_4()
      .bg(rgb(0xffffff))
      .child(div().flex_1().child(AssistantView::new(thread)))
      .child(div().text_sm().text_color(rgb(0x6b7280)).child(status))
      .child(
        div()
          .id("send")
          .px_3()
          .py_2()
          .rounded_md()
          .bg(rgb(0x111827))
          .text_color(rgb(0xffffff))
          .cursor_pointer()
          .child("Send ping")
          .on_click(cx.listener(|this, _, _window, cx| {
            this
              .assistant
              .update(cx, |assistant, cx| assistant.send("ping", cx));
          })),
      )
  }
}

fn main() {
  application().run(|cx: &mut App| {
    let bounds = Bounds::centered(None, size(px(720.), px(560.)), cx);

    cx.open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
      },
      |_, cx| {
        cx.new(|cx| {
          let thread = Thread {
            id: ThreadId("example".into()),
            ..Default::default()
          };
          let assistant =
            cx.new(|_| AssistantThread::new(thread, Arc::new(EchoRuntime::default())));

          cx.observe(&assistant, |_, _, cx| cx.notify()).detach();

          BasicChat { assistant }
        })
      },
    )
    .unwrap();

    cx.activate(true);
  });
}
