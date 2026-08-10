use super::*;
use futures_util::StreamExt;

fn assistant_message(id: &str) -> Message {
  Message::new(id, Role::Assistant)
}

#[test]
fn apply_event_appends_started_message() {
  let mut thread = Thread {
    id: ThreadId("thread".into()),
    messages: Vec::new(),
  };

  thread.apply_event(AssistantEvent::MessageStarted {
    message: assistant_message("message-1"),
  });

  assert_eq!(thread.messages.len(), 1);
  assert_eq!(thread.messages[0].id, MessageId("message-1".into()));
  assert_eq!(thread.messages[0].role, Role::Assistant);
}

#[test]
fn apply_event_coalesces_adjacent_text_deltas() {
  let mut thread = Thread {
    id: ThreadId("thread".into()),
    messages: vec![assistant_message("message-1")],
  };

  thread.apply_event(AssistantEvent::TextDelta {
    message_id: MessageId("message-1".into()),
    delta: "Hel".into(),
  });
  thread.apply_event(AssistantEvent::TextDelta {
    message_id: MessageId("message-1".into()),
    delta: "lo".into(),
  });

  assert_eq!(
    thread.messages[0].parts,
    vec![MessagePart::Text {
      text: "Hello".into()
    }]
  );
}

#[test]
fn apply_event_keeps_text_and_thinking_as_separate_parts() {
  let mut thread = Thread {
    id: ThreadId("thread".into()),
    messages: vec![assistant_message("message-1")],
  };

  thread.apply_event(AssistantEvent::ThinkingDelta {
    message_id: MessageId("message-1".into()),
    delta: "Let me".into(),
  });
  thread.apply_event(AssistantEvent::ThinkingDelta {
    message_id: MessageId("message-1".into()),
    delta: " think".into(),
  });
  thread.apply_event(AssistantEvent::TextDelta {
    message_id: MessageId("message-1".into()),
    delta: "Done".into(),
  });

  assert_eq!(
    thread.messages[0].parts,
    vec![
      MessagePart::Thinking {
        text: "Let me think".into(),
        signature: None,
      },
      MessagePart::Text {
        text: "Done".into(),
      },
    ]
  );
}

#[test]
fn user_input_text_builds_input_without_attachments() {
  let input = UserInput::text("thread", "Hello");

  assert_eq!(input.thread_id, ThreadId("thread".into()));
  assert_eq!(input.text, "Hello");
  assert!(input.attachments.is_empty());
}

#[test]
fn apply_event_updates_tool_call_and_appends_result() {
  let mut thread = Thread {
    id: ThreadId("thread".into()),
    messages: vec![assistant_message("message-1")],
  };

  let call_id = ToolCallId("call-1".into());
  thread.apply_event(AssistantEvent::ToolCallStarted {
    message_id: MessageId("message-1".into()),
    call: ToolCall {
      id: call_id.clone(),
      name: "read".into(),
      input: "{}".into(),
      status: ToolCallStatus::Running,
    },
  });
  thread.apply_event(AssistantEvent::ToolCallUpdated {
    message_id: MessageId("message-1".into()),
    call: ToolCall {
      id: call_id.clone(),
      name: "read".into(),
      input: "{\"path\":\"README.md\"}".into(),
      status: ToolCallStatus::Running,
    },
  });
  thread.apply_event(AssistantEvent::ToolCallFinished {
    message_id: MessageId("message-1".into()),
    result: ToolResult {
      call_id: call_id.clone(),
      output: "ok".into(),
      is_error: false,
    },
  });

  assert_eq!(
    thread.messages[0].parts,
    vec![
      MessagePart::ToolCall(ToolCall {
        id: call_id.clone(),
        name: "read".into(),
        input: "{\"path\":\"README.md\"}".into(),
        status: ToolCallStatus::Finished,
      }),
      MessagePart::ToolResult(ToolResult {
        call_id,
        output: "ok".into(),
        is_error: false,
      }),
    ]
  );
}

#[test]
fn echo_runtime_streams_response_events() {
  let runtime = EchoRuntime::default();
  let events = futures_executor::block_on(async {
    runtime
      .send(UserInput::text("thread", "Hello"))
      .collect::<Vec<_>>()
      .await
  });

  assert_eq!(
    events,
    vec![
      AssistantEvent::MessageStarted {
        message: Message::new("thread-assistant", Role::Assistant),
      },
      AssistantEvent::TextDelta {
        message_id: MessageId("thread-assistant".into()),
        delta: "Echo: Hello".into(),
      },
      AssistantEvent::MessageFinished {
        message_id: MessageId("thread-assistant".into()),
      },
    ]
  );
}
