use std::sync::Arc;

use crate::EffectiveConfig;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Patch<T> {
    #[default]
    Inherit,
    Clear,
    Set(T),
}

impl<T: Clone> Patch<T> {
    fn apply_to(&self, value: &mut T, schema_default: &T) {
        match self {
            Self::Inherit => {}
            Self::Clear => value.clone_from(schema_default),
            Self::Set(replacement) => value.clone_from(replacement),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigPatch {
    pub font: FontConfigPatch,
    pub terminal: TerminalConfigPatch,
    pub input: InputConfigPatch,
    pub window: WindowConfigPatch,
    pub render: RenderConfigPatch,
    pub domain: DomainConfigPatch,
    pub lifecycle: LifecycleConfigPatch,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontConfigPatch {
    pub family: Patch<String>,
    pub size_milli_points: Patch<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalConfigPatch {
    pub scrollback_lines: Patch<usize>,
    pub term: Patch<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputConfigPatch {
    pub copy_on_select: Patch<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowConfigPatch {
    pub title: Patch<String>,
    pub integrated_titlebar: Patch<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderConfigPatch {
    pub max_fps: Patch<u16>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DomainConfigPatch {
    pub default_domain: Patch<Option<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LifecycleConfigPatch {
    pub reload_on_change: Patch<bool>,
}

pub fn resolve_layers<'a>(
    layers: impl IntoIterator<Item = &'a ConfigPatch>,
) -> Arc<EffectiveConfig> {
    let defaults = EffectiveConfig::default();
    let mut resolved = defaults.clone();
    for layer in layers {
        layer.apply_to(&mut resolved, &defaults);
    }
    Arc::new(resolved)
}

impl ConfigPatch {
    fn apply_to(&self, config: &mut EffectiveConfig, defaults: &EffectiveConfig) {
        self.font
            .family
            .apply_to(&mut config.font.family, &defaults.font.family);
        self.font.size_milli_points.apply_to(
            &mut config.font.size_milli_points,
            &defaults.font.size_milli_points,
        );
        self.terminal.scrollback_lines.apply_to(
            &mut config.terminal.scrollback_lines,
            &defaults.terminal.scrollback_lines,
        );
        self.terminal
            .term
            .apply_to(&mut config.terminal.term, &defaults.terminal.term);
        self.input.copy_on_select.apply_to(
            &mut config.input.copy_on_select,
            &defaults.input.copy_on_select,
        );
        self.window
            .title
            .apply_to(&mut config.window.title, &defaults.window.title);
        self.window.integrated_titlebar.apply_to(
            &mut config.window.integrated_titlebar,
            &defaults.window.integrated_titlebar,
        );
        self.render
            .max_fps
            .apply_to(&mut config.render.max_fps, &defaults.render.max_fps);
        self.domain.default_domain.apply_to(
            &mut config.domain.default_domain,
            &defaults.domain.default_domain,
        );
        self.lifecycle.reload_on_change.apply_to(
            &mut config.lifecycle.reload_on_change,
            &defaults.lifecycle.reload_on_change,
        );
    }
}
