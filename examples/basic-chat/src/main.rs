use gpui::{
  App, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions, div, prelude::*,
  px, rgb, size,
};
use gpui_assistant_core::{Message, Thread, ThreadId};
use gpui_assistant_ui::AssistantView;
use gpui_platform::application;

struct BasicChat {
  thread: Thread,
}

impl Render for BasicChat {
  fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
    div()
      .size_full()
      .p_4()
      .bg(rgb(0xffffff))
      .child(AssistantView::new(self.thread.clone()))
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
        cx.new(|_| BasicChat {
          thread: Thread {
            id: ThreadId("example".into()),
            messages: vec![
              Message::user("user-1", "Hello GPUI assistant"),
              Message::assistant("assistant-1", "Hello. This is the initial scaffold."),
            ],
          },
        })
      },
    )
    .unwrap();

    cx.activate(true);
  });
}
