use super::*;

fn text_chunk(text: &str) -> acp::ContentChunk {
  acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text)))
}

fn acp_tool_call(id: &str, title: &str) -> acp::ToolCall {
  acp::ToolCall::new(acp::ToolCallId::new(id), title)
}

fn tool_call_update(id: &str, status: acp::ToolCallStatus) -> acp::ToolCallUpdate {
  acp::ToolCallUpdate::new(
    acp::ToolCallId::new(id),
    acp::ToolCallUpdateFields::new().status(status),
  )
}

#[test]
fn the_first_chunk_of_a_turn_opens_an_assistant_message() {
  let mut turn = Turn::new("session");

  let events = turn.apply(acp::SessionUpdate::AgentMessageChunk(text_chunk("Hel")));

  assert_eq!(
    events,
    vec![
      AssistantEvent::MessageStarted {
        message: Message::new("session-assistant-0", Role::Assistant),
      },
      AssistantEvent::TextDelta {
        message_id: MessageId("session-assistant-0".into()),
        delta: "Hel".into(),
      },
    ]
  );

  let events = turn.apply(acp::SessionUpdate::AgentMessageChunk(text_chunk("lo")));

  assert_eq!(
    events,
    vec![AssistantEvent::TextDelta {
      message_id: MessageId("session-assistant-0".into()),
      delta: "lo".into(),
    }]
  );
}

#[test]
fn each_turn_opens_its_own_message() {
  let mut turn = Turn::new("session");

  turn.apply(acp::SessionUpdate::AgentMessageChunk(text_chunk("first")));
  assert_eq!(
    turn.end(acp::StopReason::EndTurn),
    vec![AssistantEvent::MessageFinished {
      message_id: MessageId("session-assistant-0".into()),
    }]
  );

  let events = turn.apply(acp::SessionUpdate::AgentMessageChunk(text_chunk("second")));

  assert_eq!(
    events.first(),
    Some(&AssistantEvent::MessageStarted {
      message: Message::new("session-assistant-1", Role::Assistant),
    })
  );
}

#[test]
fn thought_chunks_map_to_thinking_deltas() {
  let mut turn = Turn::new("session");

  let events = turn.apply(acp::SessionUpdate::AgentThoughtChunk(text_chunk("hmm")));

  assert_eq!(
    events.last(),
    Some(&AssistantEvent::ThinkingDelta {
      message_id: MessageId("session-assistant-0".into()),
      delta: "hmm".into(),
    })
  );
}

#[test]
fn user_message_chunks_are_dropped() {
  let mut turn = Turn::new("session");

  assert!(
    turn
      .apply(acp::SessionUpdate::UserMessageChunk(text_chunk("Hello")))
      .is_empty()
  );
}

#[test]
fn a_partial_update_merges_onto_the_call_snapshot() {
  let mut turn = Turn::new("session");

  turn.apply(acp::SessionUpdate::ToolCall(acp_tool_call(
    "call-1", "read",
  )));
  let events = turn.apply(acp::SessionUpdate::ToolCallUpdate(tool_call_update(
    "call-1",
    acp::ToolCallStatus::InProgress,
  )));

  assert_eq!(
    events,
    vec![AssistantEvent::ToolCallUpdated {
      message_id: MessageId("session-assistant-0".into()),
      call: ToolCall {
        id: ToolCallId("call-1".into()),
        // Kept from the original call: the update only carried a status.
        name: "read".into(),
        input: String::new(),
        status: ToolCallStatus::Running,
      },
    }]
  );
}

#[test]
fn a_completed_update_also_emits_the_tool_result() {
  let mut turn = Turn::new("session");

  turn.apply(acp::SessionUpdate::ToolCall(acp_tool_call(
    "call-1", "read",
  )));

  let update = acp::ToolCallUpdate::new(
    acp::ToolCallId::new("call-1"),
    acp::ToolCallUpdateFields::new()
      .status(acp::ToolCallStatus::Completed)
      .content(vec![acp::ToolCallContent::from(acp::ContentBlock::Text(
        acp::TextContent::new("file contents"),
      ))]),
  );

  let events = turn.apply(acp::SessionUpdate::ToolCallUpdate(update));

  assert_eq!(
    events.last(),
    Some(&AssistantEvent::ToolCallFinished {
      message_id: MessageId("session-assistant-0".into()),
      result: ToolResult {
        call_id: ToolCallId("call-1".into()),
        output: "file contents".into(),
        is_error: false,
      },
    })
  );
}

#[test]
fn a_failed_update_marks_the_result_as_an_error() {
  let mut turn = Turn::new("session");

  turn.apply(acp::SessionUpdate::ToolCall(acp_tool_call(
    "call-1", "read",
  )));
  let events = turn.apply(acp::SessionUpdate::ToolCallUpdate(tool_call_update(
    "call-1",
    acp::ToolCallStatus::Failed,
  )));

  assert!(matches!(
    events.last(),
    Some(AssistantEvent::ToolCallFinished { result, .. }) if result.is_error
  ));
}

#[test]
fn cancelling_ends_the_turn_without_an_error() {
  let mut turn = Turn::new("session");

  turn.apply(acp::SessionUpdate::AgentMessageChunk(text_chunk("partial")));

  assert_eq!(
    turn.end(acp::StopReason::Cancelled),
    vec![AssistantEvent::MessageFinished {
      message_id: MessageId("session-assistant-0".into()),
    }]
  );
}

#[test]
fn a_refusal_ends_the_turn_with_an_error() {
  let mut turn = Turn::new("session");

  turn.apply(acp::SessionUpdate::AgentMessageChunk(text_chunk("partial")));

  assert!(matches!(
    turn.end(acp::StopReason::Refusal).as_slice(),
    [AssistantEvent::Error { .. }]
  ));
}

#[test]
fn permission_options_keep_their_ids_and_kinds() {
  let request = acp::RequestPermissionRequest::new(
    acp::SessionId::new("session"),
    acp::ToolCallUpdate::new(
      acp::ToolCallId::new("call-1"),
      acp::ToolCallUpdateFields::new(),
    ),
    vec![
      acp::PermissionOption::new(
        acp::PermissionOptionId::new("allow"),
        "Allow",
        acp::PermissionOptionKind::AllowOnce,
      ),
      acp::PermissionOption::new(
        acp::PermissionOptionId::new("reject"),
        "Reject",
        acp::PermissionOptionKind::RejectAlways,
      ),
    ],
  );

  let request = permission_request(PermissionRequestId("permission-0".into()), &request);

  assert_eq!(request.call_id, ToolCallId("call-1".into()));
  assert_eq!(
    request.options,
    vec![
      PermissionOption {
        id: PermissionOptionId("allow".into()),
        name: "Allow".into(),
        kind: PermissionOptionKind::AllowOnce,
      },
      PermissionOption {
        id: PermissionOptionId("reject".into()),
        name: "Reject".into(),
        kind: PermissionOptionKind::RejectAlways,
      },
    ]
  );
}
