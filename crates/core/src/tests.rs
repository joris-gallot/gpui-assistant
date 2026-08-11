use super::*;
use futures_util::StreamExt;

fn thread(messages: Vec<Message>) -> Thread {
  Thread {
    id: ThreadId("thread".into()),
    messages,
    ..Default::default()
  }
}

fn assistant_message(id: &str) -> Message {
  Message::new(id, Role::Assistant)
}

fn collect(stream: AssistantEventStream) -> Vec<AssistantEvent> {
  futures_executor::block_on(stream.collect::<Vec<_>>())
}

#[test]
fn apply_event_appends_started_message() {
  let mut thread = thread(Vec::new());

  thread.apply_event(AssistantEvent::MessageStarted {
    message: assistant_message("message-1"),
  });

  assert_eq!(thread.messages.len(), 1);
  assert_eq!(thread.messages[0].id, MessageId("message-1".into()));
  assert_eq!(thread.messages[0].role, Role::Assistant);
}

#[test]
fn apply_event_coalesces_adjacent_text_deltas() {
  let mut thread = thread(vec![assistant_message("message-1")]);

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
  let mut thread = thread(vec![assistant_message("message-1")]);

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
  let mut thread = thread(vec![assistant_message("message-1")]);

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
fn thread_starts_idle() {
  let thread = thread(Vec::new());

  assert_eq!(thread.status, ThreadStatus::Idle);
  assert!(!thread.is_generating());
}

#[test]
fn apply_event_marks_thread_generating_until_finished() {
  let mut thread = thread(Vec::new());

  thread.apply_event(AssistantEvent::MessageStarted {
    message: assistant_message("message-1"),
  });
  assert_eq!(thread.status, ThreadStatus::Generating);

  thread.apply_event(AssistantEvent::TextDelta {
    message_id: MessageId("message-1".into()),
    delta: "Hi".into(),
  });
  assert_eq!(thread.status, ThreadStatus::Generating);

  thread.apply_event(AssistantEvent::MessageFinished {
    message_id: MessageId("message-1".into()),
  });
  assert_eq!(thread.status, ThreadStatus::Idle);
}

#[test]
fn apply_event_records_error_and_clears_it_on_next_turn() {
  let mut thread = thread(Vec::new());

  thread.apply_event(AssistantEvent::Error {
    message: "boom".into(),
  });
  assert_eq!(
    thread.status,
    ThreadStatus::Error {
      message: "boom".into()
    }
  );

  thread.apply_event(AssistantEvent::MessageStarted {
    message: assistant_message("message-1"),
  });
  assert_eq!(thread.status, ThreadStatus::Generating);
}

#[test]
fn streaming_message_id_only_targets_trailing_assistant_message() {
  let mut thread = thread(vec![Message::user("user-1", "Hello")]);

  assert_eq!(thread.streaming_message_id(), None);

  thread.status = ThreadStatus::Generating;
  assert_eq!(thread.streaming_message_id(), None);

  thread.apply_event(AssistantEvent::MessageStarted {
    message: assistant_message("message-1"),
  });
  assert_eq!(
    thread.streaming_message_id(),
    Some(&MessageId("message-1".into()))
  );

  thread.apply_event(AssistantEvent::MessageFinished {
    message_id: MessageId("message-1".into()),
  });
  assert_eq!(thread.streaming_message_id(), None);
}

fn permission_request(id: &str, call_id: &str) -> PermissionRequest {
  PermissionRequest {
    id: PermissionRequestId(id.into()),
    call_id: ToolCallId(call_id.into()),
    options: vec![
      PermissionOption {
        id: PermissionOptionId("allow".into()),
        name: "Allow".into(),
        kind: PermissionOptionKind::AllowOnce,
      },
      PermissionOption {
        id: PermissionOptionId("reject".into()),
        name: "Reject".into(),
        kind: PermissionOptionKind::RejectOnce,
      },
    ],
  }
}

#[test]
fn apply_event_parks_the_thread_until_every_permission_is_resolved() {
  let mut thread = thread(vec![assistant_message("message-1")]);

  thread.apply_event(AssistantEvent::PermissionRequested {
    request: permission_request("permission-1", "call-1"),
  });
  thread.apply_event(AssistantEvent::PermissionRequested {
    request: permission_request("permission-2", "call-2"),
  });
  assert_eq!(thread.status, ThreadStatus::WaitingForApproval);
  assert_eq!(thread.pending_permissions.len(), 2);

  thread.apply_event(AssistantEvent::PermissionResolved {
    request_id: PermissionRequestId("permission-1".into()),
  });
  assert_eq!(thread.status, ThreadStatus::WaitingForApproval);

  thread.apply_event(AssistantEvent::PermissionResolved {
    request_id: PermissionRequestId("permission-2".into()),
  });
  assert_eq!(thread.status, ThreadStatus::Generating);
  assert!(thread.pending_permissions.is_empty());
}

#[test]
fn apply_event_drops_pending_permissions_when_the_turn_ends() {
  let mut thread = thread(vec![assistant_message("message-1")]);

  thread.apply_event(AssistantEvent::PermissionRequested {
    request: permission_request("permission-1", "call-1"),
  });
  thread.apply_event(AssistantEvent::Error {
    message: "boom".into(),
  });

  assert!(thread.pending_permissions.is_empty());
  assert_eq!(
    thread.status,
    ThreadStatus::Error {
      message: "boom".into()
    }
  );
}

#[test]
fn streaming_message_id_is_none_while_waiting_for_approval() {
  let mut thread = thread(vec![assistant_message("message-1")]);

  thread.apply_event(AssistantEvent::PermissionRequested {
    request: permission_request("permission-1", "call-1"),
  });

  assert!(!thread.is_generating());
  assert_eq!(thread.streaming_message_id(), None);
}

#[test]
fn echo_runtime_streams_response_events() {
  let runtime = EchoRuntime::default();
  let events = collect(runtime.send(UserInput::text("thread", "Hello")));

  assert_eq!(
    events,
    vec![
      AssistantEvent::MessageStarted {
        message: Message::new("thread-assistant-0", Role::Assistant),
      },
      AssistantEvent::TextDelta {
        message_id: MessageId("thread-assistant-0".into()),
        delta: "Echo: Hello".into(),
      },
      AssistantEvent::MessageFinished {
        message_id: MessageId("thread-assistant-0".into()),
      },
    ]
  );
}

#[test]
fn echo_runtime_mints_a_distinct_message_id_per_turn() {
  let runtime = EchoRuntime::default();
  let mut thread = thread(Vec::new());

  for event in collect(runtime.send(UserInput::text("thread", "first"))) {
    thread.apply_event(event);
  }
  for event in collect(runtime.send(UserInput::text("thread", "second"))) {
    thread.apply_event(event);
  }

  assert_eq!(thread.messages.len(), 2);
  assert_eq!(
    thread.messages[1].parts,
    vec![MessagePart::Text {
      text: "Echo: second".into()
    }]
  );
}
