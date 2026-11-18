// `#[cgp_auto_getter]` expands a `&str` getter to `HasField<.., Value = String>` and names
// `String` unqualified, so a `no_std` crate must import it (ADR-001, finding 7).
#[allow(unused_imports)]
use alloc::string::String;

use cgp_fork::prelude::*;

/// The profile chosen at bootstrap (`--profile`, then `{PREFIX}_PROFILE`, then
/// `"default"`). It is never read from a config file.
///
/// Implemented automatically for a context with an `active_profile: String` field.
#[cgp_auto_getter]
pub trait HasActiveProfile {
    /// The active profile name.
    fn active_profile(&self) -> &str;
}
