use cgp_fork::prelude::*;

use crate::{LoadReport, LoadReportComponent, LoadWarning, WarningReporter};

/// Appends warnings to the context's `load_report: LoadReport` field, and clears that
/// field when a load starts. The preset provider.
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

    fn clear_warnings(&self) {
        self.get_field(PhantomData::<Symbol!("load_report")>)
            .clear();
    }

    fn report_profile_table(&self, profile: &str) {
        self.get_field(PhantomData::<Symbol!("load_report")>)
            .record_profile_table(profile);
    }

    fn profile_table_reported(&self, profile: &str) -> bool {
        self.get_field(PhantomData::<Symbol!("load_report")>)
            .has_profile_table(profile)
    }

    fn note_loaded(&self, stamp: crate::LoadStamp) {
        self.get_field(PhantomData::<Symbol!("load_report")>)
            .set_stamp(stamp);
    }
}

/// Drops every warning, so there is nothing to clear. For contexts without a
/// `load_report` field. It keeps no profile tables either, so a profile-file loader on
/// such a context cannot see `$profiles` tables (its `MissingProfile` warning is dropped
/// anyway).
pub struct IgnoreWarnings;

#[cgp_impl(IgnoreWarnings: LoadReportComponent)]
impl WarningReporter {
    fn report_warning(&self, _warning: LoadWarning) {}

    fn clear_warnings(&self) {}

    fn report_profile_table(&self, _profile: &str) {}

    fn profile_table_reported(&self, _profile: &str) -> bool {
        false
    }

    fn note_loaded(&self, _stamp: crate::LoadStamp) {}
}
