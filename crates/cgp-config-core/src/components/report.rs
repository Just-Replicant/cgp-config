use cgp_fork::prelude::*;

use crate::{LoadReport, LoadWarning};

/// Records a non-fatal finding of the current load.
///
/// The preset provider `ReportOnLoader` appends to the context's `load_report` field;
/// `IgnoreWarnings` drops them.
#[cgp_component {
    name: LoadReportComponent,
    provider: WarningReporter,
}]
pub trait CanReportWarning {
    /// Records `warning`.
    fn report_warning(&self, warning: LoadWarning);
}

/// The warnings collected on a loader. Implemented automatically for a context with a
/// `load_report: LoadReport` field.
#[cgp_auto_getter]
pub trait HasLoadReport {
    /// The report.
    fn load_report(&self) -> &LoadReport;
}
