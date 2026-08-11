use std::{
  collections::HashMap,
  io::Read,
  path::Path,
  process::{Child, Command, Stdio},
  sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
  },
  thread,
  time::Duration,
};

use agent_client_protocol::{Responder, schema::v1 as acp};

const DEFAULT_OUTPUT_BYTE_LIMIT: usize = 64 * 1024;
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Terminals the agent asked us to run on its behalf, keyed by the id we minted.
#[derive(Default)]
pub(crate) struct Terminals {
  next_id: AtomicU64,
  terminals: Mutex<HashMap<String, Arc<Mutex<Terminal>>>>,
}

#[derive(Default)]
struct Terminal {
  output: String,
  truncated: bool,
  byte_limit: usize,
  exit: Option<acp::TerminalExitStatus>,
  waiters: Vec<Responder<acp::WaitForTerminalExitResponse>>,
  /// Shared with the exit watcher, which only holds the lock across a `try_wait`, so
  /// killing never waits on it.
  child: Option<Arc<Mutex<Child>>>,
}

impl Terminals {
  pub(crate) fn create(
    &self,
    request: &acp::CreateTerminalRequest,
    fallback_cwd: &Path,
  ) -> std::io::Result<String> {
    let byte_limit = request
      .output_byte_limit
      .and_then(|limit| usize::try_from(limit).ok())
      .unwrap_or(DEFAULT_OUTPUT_BYTE_LIMIT);

    let mut child = Command::new(&request.command)
      .args(&request.args)
      .envs(
        request
          .env
          .iter()
          .map(|variable| (variable.name.clone(), variable.value.clone())),
      )
      .current_dir(
        request
          .cwd
          .clone()
          .unwrap_or_else(|| fallback_cwd.to_path_buf()),
      )
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()?;

    let terminal = Arc::new(Mutex::new(Terminal {
      byte_limit,
      ..Default::default()
    }));
    let id = format!("terminal-{}", self.next_id.fetch_add(1, Ordering::Relaxed));

    if let Some(stderr) = child.stderr.take() {
      spawn_reader(stderr, terminal.clone());
    }
    if let Some(stdout) = child.stdout.take() {
      spawn_reader(stdout, terminal.clone());
    }

    let child = Arc::new(Mutex::new(child));
    terminal.lock().unwrap().child = Some(child.clone());
    spawn_exit_watcher(child, terminal.clone());
    self.terminals.lock().unwrap().insert(id.clone(), terminal);

    Ok(id)
  }

  pub(crate) fn kill(&self, id: &str) {
    let Some(terminal) = self.get(id) else {
      return;
    };
    let child = terminal.lock().unwrap().child.clone();

    if let Some(child) = child {
      let _ = child.lock().unwrap().kill();
    }
  }

  pub(crate) fn output(&self, id: &str) -> Option<acp::TerminalOutputResponse> {
    let terminal = self.get(id)?;
    let terminal = terminal.lock().unwrap();
    let mut response =
      acp::TerminalOutputResponse::new(terminal.output.clone(), terminal.truncated);
    response.exit_status = terminal.exit.clone();

    Some(response)
  }

  /// Parks the responder when the process is still running: answering it is the exit
  /// watcher's job, and blocking here would stall the whole dispatch loop.
  pub(crate) fn wait(&self, id: &str, responder: Responder<acp::WaitForTerminalExitResponse>) {
    let Some(terminal) = self.get(id) else {
      let _ = responder.respond_with_internal_error(format!("unknown terminal {id}"));

      return;
    };
    let mut terminal = terminal.lock().unwrap();

    match terminal.exit.clone() {
      Some(exit) => {
        let _ = responder.respond(acp::WaitForTerminalExitResponse::new(exit));
      }
      None => terminal.waiters.push(responder),
    }
  }

  pub(crate) fn release(&self, id: &str) {
    self.terminals.lock().unwrap().remove(id);
  }

  pub(crate) fn text(&self, id: &str) -> Option<String> {
    let terminal = self.get(id)?;
    let terminal = terminal.lock().unwrap();

    Some(terminal.output.clone())
  }

  fn get(&self, id: &str) -> Option<Arc<Mutex<Terminal>>> {
    self.terminals.lock().unwrap().get(id).cloned()
  }
}

impl Terminal {
  fn push(&mut self, chunk: &str) {
    self.output.push_str(chunk);

    if self.output.len() <= self.byte_limit {
      return;
    }

    self.truncated = true;
    // A terminal is read from its tail, so drop from the front, whole lines first.
    while self.output.len() > self.byte_limit {
      match self.output.find('\n') {
        Some(newline) if newline + 1 < self.output.len() => {
          self.output.drain(..=newline);
        }
        _ => {
          let excess = self.output.len() - self.byte_limit;
          let boundary = self
            .output
            .char_indices()
            .map(|(index, _)| index)
            .find(|index| *index >= excess)
            .unwrap_or(self.output.len());

          self.output.drain(..boundary);
          break;
        }
      }
    }
  }

  fn finish(&mut self, exit: acp::TerminalExitStatus) {
    self.exit = Some(exit.clone());

    for responder in self.waiters.drain(..) {
      let _ = responder.respond(acp::WaitForTerminalExitResponse::new(exit.clone()));
    }
  }
}

fn spawn_reader(mut pipe: impl Read + Send + 'static, terminal: Arc<Mutex<Terminal>>) {
  thread::spawn(move || {
    let mut buffer = [0u8; 8192];

    loop {
      match pipe.read(&mut buffer) {
        Ok(0) | Err(_) => break,
        Ok(read) => {
          let chunk = String::from_utf8_lossy(&buffer[..read]).into_owned();
          terminal.lock().unwrap().push(&chunk);
        }
      }
    }
  });
}

fn spawn_exit_watcher(child: Arc<Mutex<Child>>, terminal: Arc<Mutex<Terminal>>) {
  thread::spawn(move || {
    let status = loop {
      match child.lock().unwrap().try_wait() {
        Ok(Some(status)) => break Some(status),
        Ok(None) => thread::sleep(EXIT_POLL_INTERVAL),
        Err(_) => break None,
      }
    };

    terminal.lock().unwrap().finish(exit_status(status));
  });
}

fn exit_status(status: Option<std::process::ExitStatus>) -> acp::TerminalExitStatus {
  let mut exit = acp::TerminalExitStatus::new();
  let Some(status) = status else {
    return exit;
  };

  exit.exit_code = status.code().and_then(|code| u32::try_from(code).ok());

  #[cfg(unix)]
  {
    use std::os::unix::process::ExitStatusExt;

    exit.signal = status.signal().map(|signal| signal.to_string());
  }

  exit
}

#[cfg(test)]
mod tests;
