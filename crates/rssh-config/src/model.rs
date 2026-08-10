#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EffectiveConfig {
    pub font: FontConfig,
    pub terminal: TerminalConfig,
    pub input: InputConfig,
    pub window: WindowConfig,
    pub render: RenderConfig,
    pub domain: DomainConfig,
    pub lifecycle: LifecycleConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontConfig {
    pub family: String,
    pub size_milli_points: u32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "monospace".to_owned(),
            size_milli_points: 12_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalConfig {
    pub scrollback_lines: usize,
    pub term: String,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            scrollback_lines: 3_500,
            term: "xterm-256color".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputConfig {
    pub copy_on_select: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowConfig {
    pub title: String,
    pub integrated_titlebar: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "R-SSH".to_owned(),
            integrated_titlebar: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderConfig {
    pub max_fps: u16,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self { max_fps: 60 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DomainConfig {
    pub default_domain: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleConfig {
    pub reload_on_change: bool,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            reload_on_change: true,
        }
    }
}
