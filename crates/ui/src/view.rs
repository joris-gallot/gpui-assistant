use std::ops::Range;

use gpui::{
  AnyElement, Context, Entity, IntoElement, ListAlignment, ListState, Render, SharedString,
  Subscription, Window, div, list, prelude::*, px,
};
use gpui_assistant_core::{PermissionRequest, ThreadStatus};

use crate::{AssistantThread, message::MessageView, style::AssistantColors};

const LIST_OVERDRAW: f32 = 256.;

pub struct AssistantView {
  assistant: Entity<AssistantThread>,
  list: ListState,
  message_count: usize,
  #[cfg(feature = "gpui-component")]
  composer: Entity<gpui_component::input::InputState>,
  _subscriptions: Vec<Subscription>,
}

impl AssistantView {
  pub fn new(
    assistant: Entity<AssistantThread>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let message_count = assistant.read(cx).thread().messages.len();
    let list = ListState::new(message_count, ListAlignment::Bottom, px(LIST_OVERDRAW));

    let mut subscriptions = Vec::new();
    subscriptions.push(cx.observe(&assistant, Self::sync_messages));

    #[cfg(feature = "gpui-component")]
    let composer = {
      let composer = cx.new(|cx| {
        gpui_component::input::InputState::new(window, cx)
          .multi_line(true)
          .submit_on_enter(true)
          .auto_grow(1, 8)
          .placeholder("Send a message")
      });

      subscriptions.push(cx.subscribe_in(&composer, window, Self::on_composer_event));

      composer
    };

    #[cfg(not(feature = "gpui-component"))]
    let _ = window;

    Self {
      assistant,
      list,
      message_count,
      #[cfg(feature = "gpui-component")]
      composer,
      _subscriptions: subscriptions,
    }
  }

  pub fn assistant(&self) -> &Entity<AssistantThread> {
    &self.assistant
  }

  fn sync_messages(&mut self, assistant: Entity<AssistantThread>, cx: &mut Context<Self>) {
    let count = assistant.read(cx).thread().messages.len();
    let (range, spliced) = spliced_range(self.message_count, count);

    self.list.splice(range, spliced);
    self.message_count = count;
    cx.notify();
  }

  fn status_bar(&self, colors: &AssistantColors, cx: &Context<Self>) -> impl IntoElement {
    let (text, color) = match &self.assistant.read(cx).thread().status {
      ThreadStatus::Idle => ("Idle".to_string(), colors.muted_foreground),
      ThreadStatus::Generating => ("Generating".to_string(), colors.muted_foreground),
      ThreadStatus::WaitingForApproval => {
        ("Waiting for approval".to_string(), colors.muted_foreground)
      }
      ThreadStatus::Error { message } => (format!("Error: {message}"), colors.danger),
    };

    div().text_sm().text_color(color).child(text)
  }

  fn permissions_bar(
    &self,
    colors: &AssistantColors,
    cx: &mut Context<Self>,
  ) -> Option<AnyElement> {
    let assistant = self.assistant.read(cx);
    let requests = assistant.thread().pending_permissions.clone();

    if requests.is_empty() {
      return None;
    }

    let rows = requests
      .into_iter()
      .map(|request| self.permission_row(request, colors, cx))
      .collect::<Vec<_>>();

    Some(
      div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(colors.radius)
        .border_1()
        .border_color(colors.border)
        .children(rows)
        .into_any_element(),
    )
  }

  fn permission_row(
    &self,
    request: PermissionRequest,
    colors: &AssistantColors,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let buttons = request
      .options
      .iter()
      .map(|option| {
        let request_id = request.id.clone();
        let option_id = option.id.clone();
        let (background, foreground) = if option.kind.is_allow() {
          (colors.foreground, colors.background)
        } else {
          (colors.muted, colors.foreground)
        };

        div()
          .id(SharedString::from(format!(
            "{}-{}",
            request.id.0, option.id.0
          )))
          .px_2()
          .py_1()
          .rounded(colors.radius)
          .bg(background)
          .text_color(foreground)
          .text_sm()
          .cursor_pointer()
          .child(option.name.clone())
          .on_click(cx.listener(move |this, _, _window, cx| {
            let request_id = request_id.clone();
            let option_id = option_id.clone();

            this.assistant.update(cx, |assistant, cx| {
              assistant.respond_to_permission(&request_id, Some(option_id), cx)
            });
          }))
      })
      .collect::<Vec<_>>();

    div()
      .flex()
      .flex_col()
      .gap_1()
      .child(div().text_sm().child(format!("Allow {}?", request.label)))
      .child(div().flex().gap_2().children(buttons))
      .into_any_element()
  }

  #[cfg(feature = "gpui-component")]
  fn composer_bar(&self, colors: &AssistantColors, cx: &mut Context<Self>) -> Option<AnyElement> {
    use gpui_component::{button::Button, input::Input};

    let button = if self.assistant.read(cx).thread().is_generating() {
      Button::new("stop")
        .label("Stop")
        .on_click(cx.listener(|this, _, _window, cx| {
          this
            .assistant
            .update(cx, |assistant, cx| assistant.cancel(cx));
        }))
    } else {
      Button::new("send")
        .label("Send")
        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
    };

    Some(
      div()
        .flex()
        .gap_2()
        .items_end()
        .border_t_1()
        .border_color(colors.border)
        .pt_2()
        .child(div().flex_1().child(Input::new(&self.composer)))
        .child(button)
        .into_any_element(),
    )
  }

  #[cfg(not(feature = "gpui-component"))]
  fn composer_bar(&self, _colors: &AssistantColors, _cx: &mut Context<Self>) -> Option<AnyElement> {
    None
  }

  #[cfg(feature = "gpui-component")]
  fn on_composer_event(
    &mut self,
    _composer: &Entity<gpui_component::input::InputState>,
    event: &gpui_component::input::InputEvent,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    if matches!(
      event,
      gpui_component::input::InputEvent::PressEnter { shift: false, .. }
    ) {
      self.submit(window, cx);
    }
  }

  #[cfg(feature = "gpui-component")]
  fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let text = self.composer.read(cx).value().to_string();

    if text.trim().is_empty() {
      return;
    }

    self
      .composer
      .update(cx, |composer, cx| composer.set_value("", window, cx));
    self
      .assistant
      .update(cx, |assistant, cx| assistant.send(text, cx));
  }
}

/// The range to hand `ListState::splice`, given the previous and current message counts.
///
/// Streaming only mutates the trailing message, so that message is always re-spliced:
/// `ListState` caches measured heights and would otherwise keep the stale one.
fn spliced_range(previous: usize, current: usize) -> (Range<usize>, usize) {
  let start = previous.min(current.saturating_sub(1));

  (start..previous, current - start)
}

impl Render for AssistantView {
  fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let colors = AssistantColors::new(cx);
    let assistant = self.assistant.clone();

    div()
      .flex()
      .flex_col()
      .gap_2()
      .size_full()
      // A single wide message would otherwise widen the column past the window.
      .overflow_hidden()
      .bg(colors.background)
      .text_color(colors.foreground)
      .child(
        list(self.list.clone(), move |ix, _window, cx| {
          let thread = assistant.read(cx).thread();
          let Some(message) = thread.messages.get(ix) else {
            return div().into_any_element();
          };
          let is_streaming = thread.streaming_message_id() == Some(&message.id);

          MessageView::new(
            SharedString::from(message.id.0.clone()),
            message.role,
            message.parts.clone(),
          )
          .streaming(is_streaming)
          .into_any_element()
        })
        .flex_1()
        .w_full(),
      )
      .children(self.permissions_bar(&colors, cx))
      .child(self.status_bar(&colors, cx))
      .children(self.composer_bar(&colors, cx))
  }
}

#[cfg(test)]
mod splice_tests {
  use super::spliced_range;

  #[test]
  fn the_first_message_is_inserted() {
    assert_eq!(spliced_range(0, 1), (0..0, 1));
  }

  #[test]
  fn an_appended_message_does_not_touch_the_earlier_ones() {
    assert_eq!(spliced_range(1, 2), (1..1, 1));
  }

  #[test]
  fn a_delta_re_splices_the_trailing_message() {
    // Skipping this leaves ListState holding the height measured before the delta.
    assert_eq!(spliced_range(2, 2), (1..2, 1));
  }

  #[test]
  fn a_shorter_thread_replaces_the_whole_list() {
    assert_eq!(spliced_range(3, 1), (0..3, 1));
  }

  #[test]
  fn an_emptied_thread_clears_the_list() {
    assert_eq!(spliced_range(2, 0), (0..2, 0));
  }
}

#[cfg(all(test, feature = "gpui-component"))]
mod tests {
  use std::sync::Arc;

  use gpui::{AppContext, TestAppContext, VisualTestContext};
  use gpui_assistant_core::{EchoRuntime, Thread, ThreadId};
  use gpui_component::input::InputEvent;

  use super::*;

  fn setup(cx: &mut TestAppContext) -> (Entity<AssistantView>, VisualTestContext) {
    cx.update(gpui_component::init);

    let assistant = cx.update(|cx| {
      cx.new(|_| {
        AssistantThread::new(
          Thread {
            id: ThreadId("thread".into()),
            ..Default::default()
          },
          Arc::new(EchoRuntime::default()),
        )
      })
    });

    let window = cx.add_window(|window, cx| AssistantView::new(assistant, window, cx));
    let view = window.root(cx).unwrap();

    (view, VisualTestContext::from_window(window.into(), cx))
  }

  // The splice arithmetic in sync_messages is hand-rolled: a wrong range silently stops
  // re-measuring a streaming message, or drifts the list out of sync with the thread.
  #[gpui::test]
  async fn the_list_stays_in_sync_across_appends_and_deltas(cx: &mut TestAppContext) {
    let (view, mut cx) = setup(cx);

    view.read_with(&cx, |view, _| assert_eq!(view.list.item_count(), 0));

    view.update(&mut cx, |view, cx| {
      view
        .assistant
        .update(cx, |assistant, cx| assistant.send("first", cx))
    });
    cx.run_until_parked();
    view.read_with(&cx, |view, _| assert_eq!(view.list.item_count(), 2));

    view.update(&mut cx, |view, cx| {
      view
        .assistant
        .update(cx, |assistant, cx| assistant.send("second", cx))
    });
    cx.run_until_parked();
    view.read_with(&cx, |view, cx| {
      assert_eq!(view.list.item_count(), 4);
      assert_eq!(
        view.list.item_count(),
        view.assistant.read(cx).thread().messages.len()
      );
    });
  }

  #[gpui::test]
  async fn press_enter_submits_the_composer(cx: &mut TestAppContext) {
    let (view, mut cx) = setup(cx);

    cx.update(|window, cx| {
      view.update(cx, |view, cx| {
        view
          .composer
          .update(cx, |composer, cx| composer.set_value("Hello", window, cx));
      });
    });

    view.update(&mut cx, |view, cx| {
      view.composer.update(cx, |_, cx| {
        cx.emit(InputEvent::PressEnter {
          secondary: false,
          shift: false,
        })
      });
    });
    cx.run_until_parked();

    view.read_with(&cx, |view, cx| {
      let thread = view.assistant().read(cx).thread();

      assert_eq!(thread.messages.len(), 2);
      assert_eq!(view.composer.read(cx).value(), "");
    });
  }

  #[gpui::test]
  async fn shift_enter_keeps_the_composer_untouched(cx: &mut TestAppContext) {
    let (view, mut cx) = setup(cx);

    cx.update(|window, cx| {
      view.update(cx, |view, cx| {
        view
          .composer
          .update(cx, |composer, cx| composer.set_value("Hello", window, cx));
      });
    });

    view.update(&mut cx, |view, cx| {
      view.composer.update(cx, |_, cx| {
        cx.emit(InputEvent::PressEnter {
          secondary: false,
          shift: true,
        })
      });
    });
    cx.run_until_parked();

    view.read_with(&cx, |view, cx| {
      assert!(view.assistant().read(cx).thread().messages.is_empty());
      assert_eq!(view.composer.read(cx).value(), "Hello");
    });
  }
}
