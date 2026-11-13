use firmware_core::{
    buttons::{BACK, Detail, LEFT, Navigation, OK, Page, RIGHT, Tab},
    observer,
    settings::{NextBoot, Screen, Settings},
};

#[test]
fn slider_requires_three_passthrough_samples() {
    for mask in 0..8 {
        let samples = core::array::from_fn(|i| mask & (1 << i) != 0);
        assert_eq!(observer::ready(samples), mask == 7);
    }
}

#[test]
fn carried_active_boot_is_removed_without_losing_meter_preferences() {
    for boot in [
        NextBoot::CableTest,
        NextBoot::ScanPd,
        NextBoot::Normal,
        NextBoot::QuickCharge,
        NextBoot::ScanQuickCharge {
            then_cable_test: false,
        },
        NextBoot::ScanQuickCharge {
            then_cable_test: true,
        },
    ] {
        let mut settings = Settings::default();
        settings.brightness = 2;
        settings.next_boot = boot;
        settings.resume = Some(Screen::Charger);
        let calibration = settings.calibration;
        observer::sanitize(&mut settings);
        assert_eq!(settings.next_boot, NextBoot::Normal);
        assert_eq!(settings.resume, None);
        assert_eq!(settings.brightness, 2);
        assert_eq!(settings.calibration, calibration);
    }
}

#[test]
fn stale_active_pages_and_long_or_short_gestures_cannot_transmit() {
    let tabs = [
        Tab::Fixed,
        Tab::Pps,
        Tab::Avs,
        Tab::Qc2,
        Tab::Qc3,
        Tab::Scp,
        Tab::Fcp,
        Tab::Afc,
        Tab::detail(Detail::Pd),
        Tab::detail(Detail::Charger),
        Tab::detail(Detail::Cable),
        Tab::Overview,
    ];
    for tab in tabs {
        let mut nav = Navigation::new(Page::Tabs(tab), Settings::default());
        for round in 0..20 {
            for key in [OK, RIGHT, OK, LEFT, BACK] {
                for raw in core::iter::repeat_n(key, if round % 2 == 0 { 80 } else { 5 })
                    .chain(core::iter::repeat_n(0, 5))
                {
                    observer::navigation(&mut nav);
                    let (input, command) = nav.update(raw);
                    assert!(command.is_none(), "active command from {tab:?}");
                    assert!(input.qc_request().is_none());
                    assert_eq!(input.settings.next_boot, NextBoot::Normal);
                    assert!(input.page.tab().is_none_or(|tab| tab.available(true)));
                }
            }
        }
    }
}
