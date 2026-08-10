mod model;
mod patch;

pub use model::{
    DomainConfig, EffectiveConfig, FontConfig, InputConfig, LifecycleConfig, RenderConfig,
    TerminalConfig, WindowConfig,
};
pub use patch::{
    ConfigPatch, DomainConfigPatch, FontConfigPatch, InputConfigPatch, LifecycleConfigPatch, Patch,
    RenderConfigPatch, TerminalConfigPatch, WindowConfigPatch, resolve_layers,
};
