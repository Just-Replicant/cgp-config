use cgp_fork::prelude::*;

use crate::{LoadReport, LoadWarning};

/// Records the non-fatal findings of the current load.
///
/// The preset provider `ReportOnLoader` appends to the context's `load_report` field and
/// clears it when a load starts; `IgnoreWarnings` drops warnings and has nothing to clear.
///
/// `LoadLayered` calls [`clear_warnings`](CanReportWarning::clear_warnings) before its first stage, so
/// the report always describes the latest load, including a failed one, and a second load
/// does not stack the first load's warnings.
#[cgp_component {
    name: LoadReportComponent,
    provider: WarningReporter,
}]
pub trait CanReportWarning {
    /// Records `warning`.
    fn report_warning(&self, warning: LoadWarning);

    /// Discards the warnings (and recorded profile tables) of earlier loads. Called once
    /// at the start of every load.
    fn clear_warnings(&self);

    /// Records that a document of the current load defined `$profiles.{profile}`.
    /// `PrepareFileLayer` calls it for the active profile; the profile-file loader reads
    /// it back with [`profile_table_reported`](Self::profile_table_reported).
    fn report_profile_table(&self, profile: &str);

    /// Whether [`report_profile_table`](Self::report_profile_table) was called for
    /// `profile` during the current load. A reporter that keeps nothing answers `false`.
    fn profile_table_reported(&self, profile: &str) -> bool;
}

/// The warnings collected on a loader. Implemented automatically for a context with a
/// `load_report: LoadReport` field.
#[cgp_auto_getter]
pub trait HasLoadReport {
    /// The report.
    fn load_report(&self) -> &LoadReport;
}
