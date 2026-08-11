//! Deterministic native-window state and effect contracts.

mod controller;
mod effect;
mod intent;
mod state;

pub use controller::reduce;
pub use effect::{
    ClipboardEffect, HostEffectContext, NotificationEffect, PersistenceEffect, RendererEffect,
    RuntimePortEffect, SpawnEffect, UriEffect, WindowEffect, WindowPortEffect,
};
pub use intent::{
    CommandIntent, ConfigDiff, PaneLifecycleIntent, PlatformIntent, TimerId, TimerIntent,
    WindowIntent,
};
pub use state::{
    ConfigState, LifecycleState, PaneState, PlatformState, PresentationState, TimerState,
    WindowState,
};
