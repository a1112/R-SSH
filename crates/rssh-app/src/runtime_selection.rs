use std::fmt;

pub(crate) const RUNTIME_SELECTOR_ENV: &str = "RSSH_INTERNAL_RUNTIME";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeSelection {
    Legacy,
    V2,
}

impl RuntimeSelection {
    pub(crate) fn from_process() -> Result<Self, RuntimeSelectionError> {
        Self::select(
            std::env::var_os(RUNTIME_SELECTOR_ENV).as_deref(),
            SelectionPolicy::configured(),
        )
    }

    fn select(
        value: Option<&std::ffi::OsStr>,
        policy: SelectionPolicy,
    ) -> Result<Self, RuntimeSelectionError> {
        match value.and_then(std::ffi::OsStr::to_str) {
            None if value.is_none() => Ok(policy.default_selection()),
            Some("legacy") => Ok(policy.requested_selection(Self::Legacy)),
            Some("v2") => Ok(policy.requested_selection(Self::V2)),
            _ => Err(RuntimeSelectionError),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionPolicy {
    Developer,
    ReleaseLegacy,
    ReleaseV2Evaluation,
}

impl SelectionPolicy {
    const fn configured() -> Self {
        if cfg!(debug_assertions) {
            Self::Developer
        } else if cfg!(feature = "runtime-v2-evaluation") {
            Self::ReleaseV2Evaluation
        } else {
            Self::ReleaseLegacy
        }
    }

    const fn default_selection(self) -> RuntimeSelection {
        match self {
            Self::Developer | Self::ReleaseLegacy => RuntimeSelection::Legacy,
            Self::ReleaseV2Evaluation => RuntimeSelection::V2,
        }
    }

    const fn requested_selection(self, requested: RuntimeSelection) -> RuntimeSelection {
        match self {
            Self::Developer => requested,
            Self::ReleaseLegacy => RuntimeSelection::Legacy,
            Self::ReleaseV2Evaluation => RuntimeSelection::V2,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::{RuntimeSelection, SelectionPolicy};

    #[test]
    fn selection_policy_covers_every_release_input() {
        for (policy, expected) in [
            (SelectionPolicy::ReleaseLegacy, RuntimeSelection::Legacy),
            (SelectionPolicy::ReleaseV2Evaluation, RuntimeSelection::V2),
        ] {
            assert_eq!(RuntimeSelection::select(None, policy), Ok(expected));
            assert_eq!(
                RuntimeSelection::select(Some(OsStr::new("legacy")), policy),
                Ok(expected)
            );
            assert_eq!(
                RuntimeSelection::select(Some(OsStr::new("v2")), policy),
                Ok(expected)
            );
            assert!(RuntimeSelection::select(Some(OsStr::new("auto")), policy).is_err());
        }
    }

    #[test]
    fn compiled_configuration_selects_the_expected_policy() {
        let expected = if cfg!(debug_assertions) {
            SelectionPolicy::Developer
        } else if cfg!(feature = "runtime-v2-evaluation") {
            SelectionPolicy::ReleaseV2Evaluation
        } else {
            SelectionPolicy::ReleaseLegacy
        };

        assert_eq!(SelectionPolicy::configured(), expected);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeSelectionError;

impl fmt::Display for RuntimeSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{RUNTIME_SELECTOR_ENV} must be either `legacy` or `v2`"
        )
    }
}

impl std::error::Error for RuntimeSelectionError {}
