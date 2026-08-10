mod diagnostic;
mod diff;
mod model;
mod patch;
mod validate;

pub use diagnostic::ConfigDiagnostic;
pub use diff::{
    ConfigDiff, DomainConfigDiff, FontConfigDiff, InputConfigDiff, LifecycleConfigDiff,
    RenderConfigDiff, TerminalConfigDiff, WindowConfigDiff,
};
pub use model::{
    DomainConfig, EffectiveConfig, FontConfig, InputConfig, LifecycleConfig, RenderConfig,
    TerminalConfig, WindowConfig,
};
pub use patch::{
    ConfigPatch, DomainConfigPatch, FontConfigPatch, InputConfigPatch, LifecycleConfigPatch, Patch,
    RenderConfigPatch, TerminalConfigPatch, WindowConfigPatch, resolve_layers,
};
pub use validate::{ConfigUpdate, ValidatedConfigStore, validate};
