use std::path::PathBuf;

use super::*;

fn terminal(byte_limit: usize) -> Terminal {
  Terminal {
    byte_limit,
    ..Default::default()
  }
}

#[test]
fn output_under_the_limit_is_kept_whole() {
  let mut terminal = terminal(64);

  terminal.push("one\n");
  terminal.push("two\n");

  assert_eq!(terminal.output, "one\ntwo\n");
  assert!(!terminal.truncated);
}

#[test]
fn output_over_the_limit_drops_whole_lines_from_the_front() {
  let mut terminal = terminal(8);

  terminal.push("one\ntwo\nthree\n");

  assert_eq!(terminal.output, "three\n");
  assert!(terminal.truncated);
}

#[test]
fn a_single_long_line_is_cut_on_a_char_boundary() {
  let mut terminal = terminal(4);

  terminal.push("éééé");

  assert!(terminal.truncated);
  assert!(terminal.output.len() <= 4);
  // Dropping mid-codepoint would have panicked or produced replacement characters.
  assert!(terminal.output.chars().all(|character| character == 'é'));
}

#[cfg(unix)]
#[test]
fn a_spawned_command_reports_its_output_and_exit_code() {
  use std::time::Instant;

  let terminals = Terminals::default();
  let mut request = acp::CreateTerminalRequest::new(acp::SessionId::new("session"), "sh");
  request.args = vec!["-c".into(), "printf hello; exit 3".into()];

  let id = terminals
    .create(&request, &PathBuf::from("."), Box::new(|_, _| {}))
    .expect("sh is available");
  let started = Instant::now();

  let exit = loop {
    let response = terminals.output(&id).expect("a live terminal");

    if let Some(exit) = response.exit_status {
      assert_eq!(response.output, "hello");
      break exit;
    }

    assert!(
      started.elapsed() < Duration::from_secs(10),
      "sh never exited"
    );
    thread::sleep(EXIT_POLL_INTERVAL);
  };

  assert_eq!(exit.exit_code, Some(3));
  assert_eq!(terminals.text(&id).as_deref(), Some("hello"));

  terminals.release(&id);
  assert!(terminals.output(&id).is_none());
}
