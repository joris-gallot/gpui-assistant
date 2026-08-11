mod mapping;
mod terminal;

use std::{
  collections::{HashMap, HashSet},
  path::{Path, PathBuf},
  sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
  },
  thread,
};

use agent_client_protocol::{
  Agent, Client, ConnectionTo, Responder,
  schema::{ProtocolVersion, v1 as acp},
};
use futures_channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures_util::StreamExt;
use gpui_assistant_core::{
  AssistantEvent, AssistantEventStream, AssistantRuntime, PermissionOption, PermissionOptionId,
  PermissionOptionKind, PermissionRequest, PermissionRequestId, ThreadId, ToolCallId, UserInput,
};

use crate::{
  mapping::{Turn, permission_request, terminal_label},
  terminal::Terminals,
};

pub use agent_client_protocol::{AcpAgent, AcpAgentConfig};

pub struct AcpRuntime {
  commands: UnboundedSender<Command>,
  shared: Arc<Shared>,
}

impl AcpRuntime {
  /// Spawns the agent process and drives its connection on a dedicated thread.
  pub fn spawn(agent: AcpAgent, cwd: impl Into<PathBuf>) -> Self {
    let (commands, receiver) = mpsc::unbounded();
    let cwd = cwd.into();
    let shared = Arc::new(Shared::new(cwd.clone()));
    let connection = shared.clone();

    thread::spawn(move || {
      futures_executor::block_on(run(agent, cwd, connection, receiver));
    });

    Self { commands, shared }
  }

  pub fn claude_agent(cwd: impl Into<PathBuf>) -> Self {
    Self::spawn(AcpAgent::claude_agent(), cwd)
  }
}

impl AssistantRuntime for AcpRuntime {
  fn send(&self, input: UserInput) -> AssistantEventStream {
    let (events, stream) = mpsc::unbounded();
    let command = Command::Prompt {
      thread_id: input.thread_id,
      text: input.text,
      events: events.clone(),
    };

    if self.commands.unbounded_send(command).is_err() {
      let _ = events.unbounded_send(AssistantEvent::Error {
        message: "The ACP connection is closed".into(),
      });
    }

    Box::pin(stream)
  }

  fn cancel(&self, thread_id: &ThreadId) {
    let _ = self.commands.unbounded_send(Command::Cancel {
      thread_id: thread_id.clone(),
    });
  }

  fn respond_to_permission(
    &self,
    request_id: &PermissionRequestId,
    option: Option<PermissionOptionId>,
  ) {
    self.shared.resolve_permission(request_id, option);
  }
}

enum Command {
  Prompt {
    thread_id: ThreadId,
    text: String,
    events: UnboundedSender<AssistantEvent>,
  },
  Cancel {
    thread_id: ThreadId,
  },
}

struct Shared {
  cwd: PathBuf,
  sessions: Mutex<HashMap<ThreadId, String>>,
  senders: Mutex<HashMap<String, UnboundedSender<AssistantEvent>>>,
  turns: Mutex<HashMap<String, Turn>>,
  permissions: Mutex<HashMap<PermissionRequestId, Parked>>,
  next_permission: AtomicU64,
  terminals: Terminals,
  /// Commands the user allowed for the rest of a session, keyed by session.
  approved: Mutex<HashMap<String, HashSet<String>>>,
}

/// A request from the agent that is waiting on the user.
enum Parked {
  Permission {
    session: String,
    responder: Responder<acp::RequestPermissionResponse>,
  },
  Terminal {
    session: String,
    label: String,
    allow_once: PermissionOptionId,
    allow_session: PermissionOptionId,
    // Boxed to keep the variant from dwarfing the rest of the enum.
    request: Box<acp::CreateTerminalRequest>,
    responder: Responder<acp::CreateTerminalResponse>,
  },
}

impl Parked {
  fn session(&self) -> &str {
    match self {
      Parked::Permission { session, .. } | Parked::Terminal { session, .. } => session,
    }
  }
}

impl Shared {
  fn new(cwd: PathBuf) -> Self {
    Self {
      cwd,
      sessions: Mutex::default(),
      senders: Mutex::default(),
      turns: Mutex::default(),
      permissions: Mutex::default(),
      next_permission: AtomicU64::default(),
      terminals: Terminals::default(),
      approved: Mutex::default(),
    }
  }

  fn session(&self, thread_id: &ThreadId) -> Option<String> {
    self.sessions.lock().unwrap().get(thread_id).cloned()
  }

  fn bind_session(&self, thread_id: ThreadId, session: String) {
    self.sessions.lock().unwrap().insert(thread_id, session);
  }

  fn register(&self, session: &str, events: UnboundedSender<AssistantEvent>) {
    self
      .senders
      .lock()
      .unwrap()
      .insert(session.to_string(), events);
  }

  /// Dropping the sender ends the stream `send` handed out, which settles the thread.
  fn unregister(&self, session: &str) {
    self.senders.lock().unwrap().remove(session);
  }

  fn emit(&self, session: &str, events: Vec<AssistantEvent>) {
    let senders = self.senders.lock().unwrap();
    let Some(sender) = senders.get(session) else {
      return;
    };

    for event in events {
      let _ = sender.unbounded_send(event);
    }
  }

  fn dispatch(&self, notification: acp::SessionNotification) {
    let session = notification.session_id.0.to_string();
    let events = self.with_turn(&session, |turn| {
      turn.apply(notification.update, &self.terminals)
    });

    self.emit(&session, events);
  }

  fn end_turn(&self, session: &str, stop_reason: acp::StopReason) -> Vec<AssistantEvent> {
    let mut events = self.cancel_parked(session);
    events.extend(self.with_turn(session, |turn| turn.end(stop_reason)));

    events
  }

  fn fail_turn(&self, session: &str, message: String) -> Vec<AssistantEvent> {
    let mut events = self.cancel_parked(session);
    events.extend(self.with_turn(session, |turn| turn.fail(message)));

    events
  }

  /// Nobody can answer a request once its turn is over, and an unanswered responder keeps
  /// the agent waiting, so the end of a turn cancels whatever is still parked.
  fn cancel_parked(&self, session: &str) -> Vec<AssistantEvent> {
    let mut permissions = self.permissions.lock().unwrap();
    let parked = permissions
      .iter()
      .filter(|(_, parked)| parked.session() == session)
      .map(|(request_id, _)| request_id.clone())
      .collect::<Vec<_>>();

    parked
      .into_iter()
      .filter_map(|request_id| {
        let parked = permissions.remove(&request_id)?;

        match parked {
          Parked::Permission { responder, .. } => {
            let _ = responder.respond(acp::RequestPermissionResponse::new(
              acp::RequestPermissionOutcome::Cancelled,
            ));
          }
          Parked::Terminal { responder, .. } => {
            let _ =
              responder.respond_with_internal_error("The turn ended before the user answered");
          }
        }

        Some(AssistantEvent::PermissionResolved { request_id })
      })
      .collect()
  }

  fn with_turn<R>(&self, session: &str, apply: impl FnOnce(&mut Turn) -> R) -> R {
    let mut turns = self.turns.lock().unwrap();
    let turn = turns
      .entry(session.to_string())
      .or_insert_with(|| Turn::new(session));

    apply(turn)
  }

  fn park_permission(
    &self,
    request: acp::RequestPermissionRequest,
    responder: Responder<acp::RequestPermissionResponse>,
  ) {
    let session = request.session_id.0.to_string();
    let request_id = self.next_permission_id();
    let call_id = ToolCallId(request.tool_call.tool_call_id.0.to_string());
    let label = request
      .tool_call
      .fields
      .title
      .clone()
      .or_else(|| self.with_turn(&session, |turn| turn.call_name(&call_id)))
      .unwrap_or_else(|| call_id.0.clone());

    self.permissions.lock().unwrap().insert(
      request_id.clone(),
      Parked::Permission {
        session: session.clone(),
        responder,
      },
    );

    self.emit(
      &session,
      vec![AssistantEvent::PermissionRequested {
        request: permission_request(request_id, label, &request),
      }],
    );
  }

  /// The agent asked us to run a command. Nothing else gates it, so the user does.
  fn park_terminal(
    &self,
    request: acp::CreateTerminalRequest,
    responder: Responder<acp::CreateTerminalResponse>,
  ) {
    let session = request.session_id.0.to_string();
    let label = terminal_label(&request);

    if self.is_approved(&session, &label) {
      self.spawn_terminal(&request, responder);

      return;
    }

    let request_id = self.next_permission_id();
    let allow_once = PermissionOptionId(format!("{}-once", request_id.0));
    let allow_session = PermissionOptionId(format!("{}-session", request_id.0));

    self.permissions.lock().unwrap().insert(
      request_id.clone(),
      Parked::Terminal {
        session: session.clone(),
        label: label.clone(),
        allow_once: allow_once.clone(),
        allow_session: allow_session.clone(),
        request: Box::new(request),
        responder,
      },
    );

    self.emit(
      &session,
      vec![AssistantEvent::PermissionRequested {
        request: PermissionRequest {
          id: request_id.clone(),
          label,
          call_id: None,
          options: vec![
            PermissionOption {
              id: allow_once,
              name: "Run".into(),
              kind: PermissionOptionKind::AllowOnce,
            },
            PermissionOption {
              id: allow_session,
              name: "Allow for session".into(),
              kind: PermissionOptionKind::AllowAlways,
            },
            PermissionOption {
              id: PermissionOptionId(format!("{}-reject", request_id.0)),
              name: "Reject".into(),
              kind: PermissionOptionKind::RejectOnce,
            },
          ],
        },
      }],
    );
  }

  fn spawn_terminal(
    &self,
    request: &acp::CreateTerminalRequest,
    responder: Responder<acp::CreateTerminalResponse>,
  ) {
    let _ = match self.terminals.create(request, &self.cwd) {
      Ok(id) => responder.respond(acp::CreateTerminalResponse::new(acp::TerminalId::new(id))),
      Err(error) => responder.respond_with_internal_error(error),
    };
  }

  /// Matched on the whole command line: allowing `cargo test` must not allow anything else.
  fn is_approved(&self, session: &str, label: &str) -> bool {
    self
      .approved
      .lock()
      .unwrap()
      .get(session)
      .is_some_and(|approved| approved.contains(label))
  }

  fn approve_for_session(&self, session: &str, label: String) {
    self
      .approved
      .lock()
      .unwrap()
      .entry(session.to_string())
      .or_default()
      .insert(label);
  }

  fn next_permission_id(&self) -> PermissionRequestId {
    PermissionRequestId(format!(
      "permission-{}",
      self.next_permission.fetch_add(1, Ordering::Relaxed)
    ))
  }

  fn resolve_permission(
    &self,
    request_id: &PermissionRequestId,
    option: Option<PermissionOptionId>,
  ) {
    let Some(parked) = self.permissions.lock().unwrap().remove(request_id) else {
      return;
    };

    let session = match parked {
      Parked::Permission { session, responder } => {
        let outcome = match option {
          Some(option) => acp::RequestPermissionOutcome::Selected(
            acp::SelectedPermissionOutcome::new(acp::PermissionOptionId::new(option.0)),
          ),
          None => acp::RequestPermissionOutcome::Cancelled,
        };

        let _ = responder.respond(acp::RequestPermissionResponse::new(outcome));

        session
      }
      Parked::Terminal {
        session,
        label,
        allow_once,
        allow_session,
        request,
        responder,
      } => {
        let allowed = option.as_ref().is_some_and(|option| {
          if option == &allow_session {
            self.approve_for_session(&session, label);
          }

          option == &allow_once || option == &allow_session
        });

        if allowed {
          self.spawn_terminal(&request, responder);
        } else {
          let _ = responder.respond_with_internal_error("The user rejected running this command");
        }

        session
      }
    };

    self.emit(
      &session,
      vec![AssistantEvent::PermissionResolved {
        request_id: request_id.clone(),
      }],
    );
  }
}

async fn run(
  agent: AcpAgent,
  cwd: PathBuf,
  shared: Arc<Shared>,
  mut commands: UnboundedReceiver<Command>,
) {
  let notifications = shared.clone();
  let permissions = shared.clone();
  let prompts = shared.clone();
  let creates = shared.clone();
  let outputs = shared.clone();
  let waits = shared.clone();
  let kills = shared.clone();
  let releases = shared.clone();

  let result = Client
    .builder()
    .on_receive_notification(
      async move |notification: acp::SessionNotification, _connection| {
        notifications.dispatch(notification);
        Ok(())
      },
      agent_client_protocol::on_receive_notification!(),
    )
    .on_receive_request(
      async move |request: acp::RequestPermissionRequest, responder, _connection| {
        // Parked instead of awaited: this callback blocks the dispatch loop until it returns.
        permissions.park_permission(request, responder);
        Ok(())
      },
      agent_client_protocol::on_receive_request!(),
    )
    .on_receive_request(
      async move |request: acp::CreateTerminalRequest, responder, _connection| {
        // Parked like a permission: the process only starts once the user allows it.
        creates.park_terminal(request, responder);
        Ok(())
      },
      agent_client_protocol::on_receive_request!(),
    )
    .on_receive_request(
      async move |request: acp::TerminalOutputRequest, responder, _connection| match outputs
        .terminals
        .output(request.terminal_id.0.as_ref())
      {
        Some(output) => responder.respond(output),
        None => responder.respond_with_internal_error("unknown terminal"),
      },
      agent_client_protocol::on_receive_request!(),
    )
    .on_receive_request(
      async move |request: acp::WaitForTerminalExitRequest, responder, _connection| {
        // Parked until the process exits: waiting here would stall the dispatch loop.
        waits
          .terminals
          .wait(request.terminal_id.0.as_ref(), responder);

        Ok(())
      },
      agent_client_protocol::on_receive_request!(),
    )
    .on_receive_request(
      async move |request: acp::KillTerminalRequest, responder, _connection| {
        kills.terminals.kill(request.terminal_id.0.as_ref());
        responder.respond(acp::KillTerminalResponse::new())
      },
      agent_client_protocol::on_receive_request!(),
    )
    .on_receive_request(
      async move |request: acp::ReleaseTerminalRequest, responder, _connection| {
        releases.terminals.release(request.terminal_id.0.as_ref());
        responder.respond(acp::ReleaseTerminalResponse::new())
      },
      agent_client_protocol::on_receive_request!(),
    )
    .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
      connection
        .send_request(
          acp::InitializeRequest::new(ProtocolVersion::V1)
            // Running the agent's commands ourselves is what makes their output visible.
            .client_capabilities(acp::ClientCapabilities::new().terminal(true)),
        )
        .block_task()
        .await?;

      while let Some(command) = commands.next().await {
        match command {
          Command::Prompt {
            thread_id,
            text,
            events,
          } => prompt(&connection, &prompts, &cwd, thread_id, text, events).await,
          Command::Cancel { thread_id } => {
            if let Some(session) = prompts.session(&thread_id) {
              let _ = connection
                .send_notification(acp::CancelNotification::new(acp::SessionId::new(session)));
            }
          }
        }
      }

      Ok(())
    })
    .await;

  if let Err(error) = result {
    let sessions = shared.senders.lock().unwrap();

    for sender in sessions.values() {
      let _ = sender.unbounded_send(AssistantEvent::Error {
        message: format!("The ACP connection failed: {error}"),
      });
    }
  }
}

async fn prompt(
  connection: &ConnectionTo<Agent>,
  shared: &Arc<Shared>,
  cwd: &Path,
  thread_id: ThreadId,
  text: String,
  events: UnboundedSender<AssistantEvent>,
) {
  let session = match shared.session(&thread_id) {
    Some(session) => session,
    None => {
      match connection
        .send_request(acp::NewSessionRequest::new(cwd.to_path_buf()))
        .block_task()
        .await
      {
        Ok(response) => {
          let session = response.session_id.0.to_string();
          shared.bind_session(thread_id, session.clone());

          session
        }
        Err(error) => {
          let _ = events.unbounded_send(AssistantEvent::Error {
            message: format!("Failed to open an ACP session: {error}"),
          });

          return;
        }
      }
    }
  };

  shared.register(&session, events);

  let response = connection
    .send_request(acp::PromptRequest::new(
      acp::SessionId::new(session.clone()),
      vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
    ))
    .block_task()
    .await;

  let events = match response {
    Ok(response) => shared.end_turn(&session, response.stop_reason),
    Err(error) => shared.fail_turn(&session, format!("The ACP agent failed: {error}")),
  };

  shared.emit(&session, events);
  shared.unregister(&session);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_session_approval_only_covers_the_exact_command() {
    let shared = Shared::new(PathBuf::from("."));

    shared.approve_for_session("session", "cargo test --workspace".into());

    assert!(shared.is_approved("session", "cargo test --workspace"));
    // A prefix match would have let anything after `cargo test` through.
    assert!(!shared.is_approved("session", "cargo test --workspace; rm -rf /"));
    assert!(!shared.is_approved("session", "cargo test"));
    // Another session never inherits an approval.
    assert!(!shared.is_approved("other", "cargo test --workspace"));
  }
}
