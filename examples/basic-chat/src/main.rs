use std::sync::Arc;

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_assistant_core::{EchoRuntime, Thread, ThreadId};
use gpui_assistant_ui::{AssistantThread, AssistantView};
use gpui_component::Root;
use gpui_component_assets::Assets;
use gpui_platform::application;

fn main() {
  application().with_assets(Assets).run(|cx: &mut App| {
    gpui_component::init(cx);

    let bounds = Bounds::centered(None, size(px(720.), px(560.)), cx);

    cx.open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
      },
      |window, cx| {
        let thread = Thread {
          id: ThreadId("example".into()),
          ..Default::default()
        };
        let assistant = cx.new(|_| AssistantThread::new(thread, Arc::new(EchoRuntime::default())));
        let view = cx.new(|cx| AssistantView::new(assistant, window, cx));

        cx.new(|cx| Root::new(view, window, cx))
      },
    )
    .unwrap();

    cx.activate(true);
  });
}
