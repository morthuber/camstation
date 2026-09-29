use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::application::Options;
use crate::media::{CameraController, DecoderInfo, PlaybackEvent, PlaybackState};

type ControllerSlot = Rc<RefCell<Option<CameraController>>>;

pub(crate) fn build_main_window(application: &gtk4::Application, options: &Options) {
    let controller = ControllerSlot::default();

    let url_entry = gtk4::Entry::builder()
        .hexpand(true)
        .placeholder_text("rtsp://camera.local/stream")
        .text(options.rtsp_url.as_deref().unwrap_or_default())
        .build();
    url_entry.set_input_purpose(gtk4::InputPurpose::Url);

    let connect_button = gtk4::Button::with_label("Connect");
    connect_button.add_css_class("suggested-action");

    let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    controls.set_margin_top(12);
    controls.set_margin_bottom(12);
    controls.set_margin_start(12);
    controls.set_margin_end(12);
    controls.append(&gtk4::Label::new(Some("RTSP URL")));
    controls.append(&url_entry);
    controls.append(&connect_button);

    let picture = gtk4::Picture::new();
    picture.set_content_fit(gtk4::ContentFit::Contain);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_can_shrink(true);

    let status_label = gtk4::Label::new(Some("Not connected"));
    status_label.set_halign(gtk4::Align::Start);
    status_label.set_valign(gtk4::Align::Start);
    status_label.set_margin_top(12);
    status_label.set_margin_start(12);
    status_label.add_css_class("title-4");

    let video_overlay = gtk4::Overlay::new();
    video_overlay.set_hexpand(true);
    video_overlay.set_vexpand(true);
    video_overlay.set_child(Some(&picture));
    video_overlay.add_overlay(&status_label);

    let decoder_label = gtk4::Label::new(Some("Decoder: waiting for stream"));
    decoder_label.set_halign(gtk4::Align::Start);
    decoder_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    decoder_label.set_margin_top(8);
    decoder_label.set_margin_bottom(8);
    decoder_label.set_margin_start(12);
    decoder_label.set_margin_end(12);

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.append(&controls);
    root.append(&video_overlay);
    root.append(&decoder_label);

    if options.kiosk && options.rtsp_url.is_some() {
        controls.set_visible(false);
    }

    connect_button.connect_clicked({
        let controller = controller.clone();
        let url_entry = url_entry.clone();
        let picture = picture.clone();
        let status_label = status_label.clone();
        let decoder_label = decoder_label.clone();

        move |_| {
            start_stream(
                &url_entry,
                &picture,
                &status_label,
                &decoder_label,
                &controller,
            );
        }
    });

    url_entry.connect_activate({
        let connect_button = connect_button.clone();
        move |_| connect_button.emit_clicked()
    });

    let window = gtk4::ApplicationWindow::builder()
        .application(application)
        .title("Camview — M1 RTSP proof of concept")
        .default_width(1_280)
        .default_height(720)
        .child(&root)
        .build();

    window.connect_close_request({
        let controller = controller.clone();
        move |_| {
            controller.borrow_mut().take();
            gtk4::glib::Propagation::Proceed
        }
    });

    if options.kiosk {
        window.fullscreen();
    }

    tracing::info!(
        kiosk = options.kiosk,
        requested_view = options.view.as_deref(),
        custom_config = options.config.is_some(),
        initial_stream_configured = options.rtsp_url.is_some(),
        "presenting application window"
    );

    window.present();

    if options.rtsp_url.is_some() {
        connect_button.emit_clicked();
    }
}

fn start_stream(
    url_entry: &gtk4::Entry,
    picture: &gtk4::Picture,
    status_label: &gtk4::Label,
    decoder_label: &gtk4::Label,
    controller: &ControllerSlot,
) {
    controller.borrow_mut().take();
    picture.set_paintable(gtk4::gdk::Paintable::NONE);
    status_label.set_text("Connecting…");
    decoder_label.set_text("Decoder: waiting for stream");

    let status_for_events = status_label.clone();
    let decoder_for_events = decoder_label.clone();
    let uri = url_entry.text();

    let camera = match CameraController::new(uri.as_str(), move |event| match event {
        PlaybackEvent::StateChanged(state) => {
            status_for_events.set_text(state_label(state));
        }
        PlaybackEvent::DecoderChanged(decoder) => {
            decoder_for_events.set_text(&decoder_label_text(&decoder));
        }
        PlaybackEvent::EndOfStream => {
            status_for_events.set_text("Stream ended");
        }
        PlaybackEvent::Error(error) => {
            status_for_events.set_text(&format!("Stream error: {error}"));
        }
    }) {
        Ok(camera) => camera,
        Err(error) => {
            status_label.set_text(&format!("Cannot create stream: {error:#}"));
            tracing::warn!(error = %error, "could not create camera pipeline");
            return;
        }
    };

    picture.set_paintable(Some(camera.paintable()));
    if let Err(error) = camera.start() {
        status_label.set_text(&format!("Cannot start stream: {error:#}"));
        tracing::warn!(error = %error, "could not start camera pipeline");
        return;
    }

    *controller.borrow_mut() = Some(camera);
}

fn state_label(state: PlaybackState) -> &'static str {
    match state {
        PlaybackState::Stopped => "Stopped",
        PlaybackState::Starting => "Connecting…",
        PlaybackState::Playing => "Live",
        PlaybackState::Paused => "Preparing stream…",
    }
}

fn decoder_label_text(decoder: &DecoderInfo) -> String {
    let acceleration = if decoder.hardware_accelerated {
        "hardware accelerated"
    } else {
        "software decoded"
    };
    format!("Decoder: {} ({acceleration})", decoder.factories.join(", "))
}
