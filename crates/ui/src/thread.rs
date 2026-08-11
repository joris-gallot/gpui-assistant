use std::sync::Arc;

use futures_util::StreamExt;
use gpui::{Context, Task};
use gpui_assistant_core::{
  AssistantRuntime, Message, PermissionOptionId, PermissionRequestId, Thread, ThreadStatus,
  UserInput,
};

pub struct AssistantThread {
  thread: Thread,
  runtime: Arc<dyn AssistantRuntime>,
  generation: Option<Task<()>>,
  next_message_id: u64,
}

impl AssistantThread {
  pub fn new(thread: Thread, runtime: Arc<dyn AssistantRuntime>) -> Self {
    let next_message_id = thread.messages.len() as u64;

    Self {
      thread,
      runtime,
      generation: None,
      next_message_id,
    }
  }

  pub fn thread(&self) -> &Thread {
    &self.thread
  }

  pub fn send(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
    let text = text.into();
    let message_id = self.take_message_id();

    self.stop_generation();
    self.thread.messages.push(Message::user(message_id, &*text));
    self.thread.status = ThreadStatus::Generating;

    let events = self
      .runtime
      .send(UserInput::text(self.thread.id.0.clone(), text));

    self.generation = Some(cx.spawn(async move |this, cx| {
      let mut events = events;

      while let Some(event) = events.next().await {
        let applied = this.update(cx, |this, cx| {
          this.thread.apply_event(event);
          cx.notify();
        });

        if applied.is_err() {
          return;
        }
      }

      // A runtime may end its stream without ever emitting MessageFinished.
      let _ = this.update(cx, |this, cx| {
        if this.thread.is_generating() {
          this.thread.status = ThreadStatus::Idle;
          cx.notify();
        }
      });
    }));

    cx.notify();
  }

  /// The runtime answers the agent, then reports back through the event stream, which is
  /// what clears the request from the thread.
  pub fn respond_to_permission(
    &mut self,
    request_id: &PermissionRequestId,
    option: Option<PermissionOptionId>,
    cx: &mut Context<Self>,
  ) {
    self.runtime.respond_to_permission(request_id, option);
    cx.notify();
  }

  pub fn cancel(&mut self, cx: &mut Context<Self>) {
    self.stop_generation();
    self.thread.status = ThreadStatus::Idle;
    cx.notify();
  }

  // Never call from inside the generation task: dropping a Task cancels it.
  fn stop_generation(&mut self) {
    if self.generation.take().is_some() {
      self.runtime.cancel(&self.thread.id);
    }
  }

  fn take_message_id(&mut self) -> String {
    let message_id = format!("{}-user-{}", self.thread.id.0, self.next_message_id);
    self.next_message_id += 1;

    message_id
  }
}

#[cfg(test)]
mod tests {
  use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
  };

  use gpui::{AppContext, TestAppContext};
  use gpui_assistant_core::{
    AssistantEventStream, EchoRuntime, MessagePart, Role, ThreadId, ThreadStatus,
  };

  use super::*;

  type Responses = Arc<Mutex<Vec<(PermissionRequestId, Option<PermissionOptionId>)>>>;

  #[derive(Clone, Default)]
  struct StallingRuntime {
    cancels: Arc<AtomicUsize>,
    responses: Responses,
  }

  impl AssistantRuntime for StallingRuntime {
    fn send(&self, _input: UserInput) -> AssistantEventStream {
      Box::pin(futures_util::stream::pending())
    }

    fn cancel(&self, _thread_id: &ThreadId) {
      self.cancels.fetch_add(1, Ordering::Relaxed);
    }

    fn respond_to_permission(
      &self,
      request_id: &PermissionRequestId,
      option: Option<PermissionOptionId>,
    ) {
      self
        .responses
        .lock()
        .unwrap()
        .push((request_id.clone(), option));
    }
  }

  struct SilentRuntime;

  impl AssistantRuntime for SilentRuntime {
    fn send(&self, _input: UserInput) -> AssistantEventStream {
      Box::pin(futures_util::stream::empty())
    }

    fn cancel(&self, _thread_id: &ThreadId) {}
  }

  fn thread() -> Thread {
    Thread {
      id: ThreadId("thread".into()),
      ..Default::default()
    }
  }

  #[gpui::test]
  async fn send_applies_runtime_events_to_the_thread(cx: &mut TestAppContext) {
    let assistant = cx.new(|_| AssistantThread::new(thread(), Arc::new(EchoRuntime::default())));

    assistant.update(cx, |assistant, cx| assistant.send("Hello", cx));
    cx.run_until_parked();

    assistant.read_with(cx, |assistant, _| {
      let thread = assistant.thread();

      assert_eq!(thread.status, ThreadStatus::Idle);
      assert_eq!(thread.messages.len(), 2);
      assert_eq!(thread.messages[0].role, Role::User);
      assert_eq!(thread.messages[1].role, Role::Assistant);
      assert_eq!(
        thread.messages[1].parts,
        vec![MessagePart::Text {
          text: "Echo: Hello".into()
        }]
      );
    });
  }

  #[gpui::test]
  async fn each_turn_appends_its_own_pair_of_messages(cx: &mut TestAppContext) {
    let assistant = cx.new(|_| AssistantThread::new(thread(), Arc::new(EchoRuntime::default())));

    assistant.update(cx, |assistant, cx| assistant.send("first", cx));
    cx.run_until_parked();
    assistant.update(cx, |assistant, cx| assistant.send("second", cx));
    cx.run_until_parked();

    assistant.read_with(cx, |assistant, _| {
      let thread = assistant.thread();

      assert_eq!(thread.messages.len(), 4);
      assert_eq!(
        thread.messages[3].parts,
        vec![MessagePart::Text {
          text: "Echo: second".into()
        }]
      );
    });
  }

  #[gpui::test]
  async fn a_stream_that_ends_without_finishing_still_settles(cx: &mut TestAppContext) {
    let assistant = cx.new(|_| AssistantThread::new(thread(), Arc::new(SilentRuntime)));

    assistant.update(cx, |assistant, cx| assistant.send("Hello", cx));
    cx.run_until_parked();

    assistant.read_with(cx, |assistant, _| {
      let thread = assistant.thread();

      assert_eq!(thread.status, ThreadStatus::Idle);
      assert_eq!(thread.messages.len(), 1);
    });
  }

  #[gpui::test]
  async fn a_new_turn_cancels_the_one_still_running(cx: &mut TestAppContext) {
    let runtime = StallingRuntime::default();
    let cancels = runtime.cancels.clone();
    let assistant = cx.new(|_| AssistantThread::new(thread(), Arc::new(runtime)));

    assistant.update(cx, |assistant, cx| assistant.send("first", cx));
    cx.run_until_parked();
    assistant.update(cx, |assistant, cx| assistant.send("second", cx));
    cx.run_until_parked();

    assert_eq!(cancels.load(Ordering::Relaxed), 1);
    assistant.read_with(cx, |assistant, _| {
      let thread = assistant.thread();

      assert!(thread.is_generating());
      assert_eq!(thread.messages.len(), 2);
    });
  }

  #[gpui::test]
  async fn responding_to_a_permission_forwards_the_choice_to_the_runtime(cx: &mut TestAppContext) {
    let runtime = StallingRuntime::default();
    let responses = runtime.responses.clone();
    let assistant = cx.new(|_| AssistantThread::new(thread(), Arc::new(runtime)));

    assistant.update(cx, |assistant, cx| {
      assistant.respond_to_permission(
        &PermissionRequestId("permission-0".into()),
        Some(PermissionOptionId("allow".into())),
        cx,
      )
    });

    assert_eq!(
      responses.lock().unwrap().as_slice(),
      [(
        PermissionRequestId("permission-0".into()),
        Some(PermissionOptionId("allow".into()))
      )]
    );
  }

  #[gpui::test]
  async fn cancel_stops_generation_and_tells_the_runtime(cx: &mut TestAppContext) {
    let runtime = StallingRuntime::default();
    let cancels = runtime.cancels.clone();
    let assistant = cx.new(|_| AssistantThread::new(thread(), Arc::new(runtime)));

    assistant.update(cx, |assistant, cx| assistant.send("Hello", cx));
    cx.run_until_parked();
    assistant.read_with(cx, |assistant, _| {
      assert!(assistant.thread().is_generating());
    });

    assistant.update(cx, |assistant, cx| assistant.cancel(cx));

    assistant.read_with(cx, |assistant, _| {
      assert_eq!(assistant.thread().status, ThreadStatus::Idle);
    });
    assert_eq!(cancels.load(Ordering::Relaxed), 1);
  }
}
