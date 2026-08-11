use std::{env, str::FromStr, sync::Arc};

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_assistant_acp::{AcpAgent, AcpRuntime};
use gpui_assistant_core::{Thread, ThreadId};
use gpui_assistant_ui::{AssistantThread, AssistantView};
use gpui_component::Root;
use gpui_component_assets::Assets;
use gpui_platform::application;

fn agent() -> Result<AcpAgent, String> {
  match env::args().nth(1).as_deref() {
    None | Some("claude") => Ok(AcpAgent::claude_agent()),
    Some("codex") => Ok(AcpAgent::codex()),
    Some(command) => AcpAgent::from_str(command).map_err(|error| format!("{command:?}: {error}")),
  }
}

fn main() {
  let agent = match agent() {
    Ok(agent) => agent,
    Err(error) => {
      eprintln!("Invalid agent command {error}");

      return;
    }
  };
  let cwd = env::current_dir().expect("a readable working directory");

  application().with_assets(Assets).run(move |cx: &mut App| {
    gpui_component::init(cx);

    let bounds = Bounds::centered(None, size(px(720.), px(560.)), cx);

    cx.open_window(
      WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
      },
      |window, cx| {
        let thread = Thread {
          id: ThreadId("acp".into()),
          ..Default::default()
        };
        let runtime = Arc::new(AcpRuntime::spawn(agent, cwd));
        let assistant = cx.new(|_| AssistantThread::new(thread, runtime));
        let view = cx.new(|cx| AssistantView::new(assistant, window, cx));

        cx.new(|cx| Root::new(view, window, cx))
      },
    )
    .unwrap();

    cx.activate(true);
  });
}
