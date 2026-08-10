use gpui_assistant_core::{AssistantEventStream, AssistantRuntime, ThreadId, UserInput};

#[derive(Clone, Debug)]
pub struct OpenAiCompatibleRuntime {
  pub base_url: String,
  pub api_key: Option<String>,
  pub model: String,
}

impl OpenAiCompatibleRuntime {
  pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
    Self {
      base_url: base_url.into(),
      api_key: None,
      model: model.into(),
    }
  }

  pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
    self.api_key = Some(api_key.into());
    self
  }
}

impl AssistantRuntime for OpenAiCompatibleRuntime {
  fn send(&self, _input: UserInput) -> AssistantEventStream<'_> {
    Box::pin(futures_util::stream::empty())
  }

  fn cancel(&self, _thread_id: &ThreadId) {}
}
