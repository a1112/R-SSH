//! Deterministic native-window state and effect contracts.

mod controller;
mod effect;
mod intent;
mod layout;
mod presentation;
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
pub use layout::{
    PaneLayout, PaneLayoutPane, PaneLayoutSpec, PanePlacement, PaneRenderRect, PaneSeparator,
    PaneSplitDirection, PaneSplitSpec, build_pane_layout,
};
pub use presentation::{
    CellMetrics, CursorPresentation, FrameRevision, OverlayPresentation, PaneFrameCandidate,
    PresentationError, PresentationFrame, PresentationInput, PresentedPane, RenderMode,
    ScaleFactor, ScrollbarPresentation, SelectionPresentation, SurfacePresentation,
    TabPresentation, build_presentation,
};
pub use state::{
    ConfigState, LifecycleState, PaneState, PlatformState, PresentationState, TimerState,
    WindowState,
};
