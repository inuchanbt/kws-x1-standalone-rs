//! Policy shared by the observation-only firmware and host regressions.
use crate::{
    buttons::Navigation,
    settings::{NextBoot, Settings},
};

pub fn sanitize(settings: &mut Settings) {
    settings.next_boot = NextBoot::Normal;
    settings.resume = None;
    settings.qc_findings = None;
    settings.scp_findings = None;
    settings.fcp_findings = None;
    settings.afc_findings = None;
}

pub fn navigation(nav: &mut Navigation) {
    nav.set_selectable(false);
    nav.set_passthrough(true);
    nav.set_cable_test(false, false);
    nav.set_silent(false, false);
}

/// Start only after all three physical slider samples indicate passthrough.
pub fn ready(samples: [bool; 3]) -> bool {
    samples.into_iter().all(|high| high)
}
