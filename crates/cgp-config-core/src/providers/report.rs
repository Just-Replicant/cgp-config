use cgp_fork::prelude::*;

use crate::{LoadReport, LoadReportComponent, LoadWarning, WarningReporter};

/// Appends warnings to the context's `load_report: LoadReport` field. The preset provider.
pub struct ReportOnLoader;

#[cgp_impl(ReportOnLoader: LoadReportComponent)]
impl WarningReporter
where
    Self: HasField<Symbol!("load_report"), Value = LoadReport>,
{
    fn report_warning(&self, warning: LoadWarning) {
        self.get_field(PhantomData::<Symbol!("load_report")>)
            .push(warning);
    }
}

/// Drops every warning. For contexts without a `load_report` field.
pub struct IgnoreWarnings;

#[cgp_impl(IgnoreWarnings: LoadReportComponent)]
impl WarningReporter {
    fn report_warning(&self, _warning: LoadWarning) {}
}
