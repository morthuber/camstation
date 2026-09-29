use gtk4::prelude::*;

use crate::application::Options;

pub(crate) fn build_main_window(application: &gtk4::Application, options: &Options) {
    let message = if options.kiosk {
        "Camview development shell is ready.\nKiosk playback will be implemented in M1."
    } else {
        "Camview development shell is ready.\nRTSP playback will be implemented in M1."
    };

    let label = gtk4::Label::builder()
        .label(message)
        .justify(gtk4::Justification::Center)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .wrap(true)
        .build();

    let window = gtk4::ApplicationWindow::builder()
        .application(application)
        .title("Camview")
        .default_width(960)
        .default_height(540)
        .child(&label)
        .build();

    if options.kiosk {
        window.fullscreen();
    }

    tracing::info!(
        kiosk = options.kiosk,
        requested_view = options.view.as_deref(),
        custom_config = options.config.is_some(),
        "presenting application window"
    );

    window.present();
}
