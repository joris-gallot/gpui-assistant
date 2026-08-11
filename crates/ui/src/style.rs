use gpui::{App, Hsla, Pixels};

pub(crate) struct AssistantColors {
  pub background: Hsla,
  pub foreground: Hsla,
  pub border: Hsla,
  pub muted: Hsla,
  pub muted_foreground: Hsla,
  pub danger: Hsla,
  pub radius: Pixels,
}

impl AssistantColors {
  #[cfg(feature = "gpui-component")]
  pub(crate) fn new(cx: &App) -> Self {
    use gpui_component::ActiveTheme;

    let theme = cx.theme();

    Self {
      background: theme.background,
      foreground: theme.foreground,
      border: theme.border,
      muted: theme.muted,
      muted_foreground: theme.muted_foreground,
      danger: theme.danger,
      radius: theme.radius,
    }
  }

  #[cfg(not(feature = "gpui-component"))]
  pub(crate) fn new(_cx: &App) -> Self {
    use gpui::{px, rgb};

    Self {
      background: rgb(0xffffff).into(),
      foreground: rgb(0x111827).into(),
      border: rgb(0xe5e7eb).into(),
      muted: rgb(0xf9fafb).into(),
      muted_foreground: rgb(0x6b7280).into(),
      danger: rgb(0xdc2626).into(),
      radius: px(6.),
    }
  }
}
