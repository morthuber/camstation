use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;

use crate::application::Options;
use crate::media::{CameraController, DecoderInfo, PlaybackEvent, PlaybackState};

const MAX_CAMERAS: usize = 10;

struct AppState {
    flow_box: gtk4::FlowBox,
    error_label: gtk4::Label,
    tiles: Vec<Rc<CameraTile>>,
    next_camera_id: u64,
    audio_update_guard: Rc<Cell<bool>>,
    editable: bool,
}

struct CameraTile {
    id: u64,
    root: gtk4::Frame,
    controller: CameraController,
    audio_button: gtk4::ToggleButton,
}

pub(crate) fn build_main_window(application: &gtk4::Application, options: &Options) {
    let url_entry = gtk4::Entry::builder()
        .hexpand(true)
        .placeholder_text("rtsp://camera.local/stream")
        .build();
    url_entry.set_input_purpose(gtk4::InputPurpose::Url);

    let add_button = gtk4::Button::with_label("Add camera");
    add_button.add_css_class("suggested-action");

    let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    controls.set_margin_top(12);
    controls.set_margin_bottom(6);
    controls.set_margin_start(12);
    controls.set_margin_end(12);
    controls.append(&gtk4::Label::new(Some("RTSP URL")));
    controls.append(&url_entry);
    controls.append(&add_button);

    let error_label = gtk4::Label::new(None);
    error_label.set_halign(gtk4::Align::Start);
    error_label.set_margin_bottom(6);
    error_label.set_margin_start(12);
    error_label.set_margin_end(12);
    error_label.add_css_class("error");
    error_label.set_visible(false);

    let flow_box = gtk4::FlowBox::builder()
        .column_spacing(8)
        .row_spacing(8)
        .homogeneous(true)
        .max_children_per_line(4)
        .min_children_per_line(1)
        .selection_mode(gtk4::SelectionMode::None)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();

    let scroller = gtk4::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .child(&flow_box)
        .build();

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.append(&controls);
    root.append(&error_label);
    root.append(&scroller);

    let editable = !options.kiosk;
    if !editable {
        controls.set_visible(false);
        error_label.set_visible(false);
    }

    let state = Rc::new(RefCell::new(AppState {
        flow_box: flow_box.clone(),
        error_label: error_label.clone(),
        tiles: Vec::new(),
        next_camera_id: 1,
        audio_update_guard: Rc::new(Cell::new(false)),
        editable,
    }));

    add_button.connect_clicked({
        let state = state.clone();
        let url_entry = url_entry.clone();
        move |_| {
            let uri = url_entry.text();
            if add_camera(&state, uri.as_str()) {
                url_entry.set_text("");
            }
        }
    });

    url_entry.connect_activate({
        let add_button = add_button.clone();
        move |_| add_button.emit_clicked()
    });

    let window = gtk4::ApplicationWindow::builder()
        .application(application)
        .title("Camview — M2 multi-camera viewer")
        .default_width(1_280)
        .default_height(720)
        .child(&root)
        .build();

    window.connect_close_request({
        let state = state.clone();
        move |_| {
            let tiles = std::mem::take(&mut state.borrow_mut().tiles);
            for tile in tiles {
                tile.controller.stop();
            }
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
        initial_camera_count = options.rtsp_url.len(),
        "presenting application window"
    );

    window.present();

    let mut ignored_urls = 0;
    for uri in &options.rtsp_url {
        if state.borrow().tiles.len() >= MAX_CAMERAS {
            ignored_urls += 1;
        } else {
            add_camera(&state, uri);
        }
    }
    if ignored_urls > 0 {
        show_error(
            &state,
            &format!(
                "Ignored {ignored_urls} startup URL(s); M2 supports at most {MAX_CAMERAS} valid cameras."
            ),
        );
    }
}

fn add_camera(state: &Rc<RefCell<AppState>>, uri: &str) -> bool {
    if state.borrow().tiles.len() >= MAX_CAMERAS {
        show_error(
            state,
            &format!("M2 supports at most {MAX_CAMERAS} simultaneous cameras."),
        );
        return false;
    }

    let (camera_id, flow_box, editable) = {
        let mut app_state = state.borrow_mut();
        let camera_id = app_state.next_camera_id;
        app_state.next_camera_id += 1;
        (camera_id, app_state.flow_box.clone(), app_state.editable)
    };

    let picture = gtk4::Picture::new();
    picture.set_content_fit(gtk4::ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_size_request(320, 180);

    let status_label = gtk4::Label::new(Some("Connecting…"));
    status_label.set_halign(gtk4::Align::Start);
    status_label.set_valign(gtk4::Align::Start);
    status_label.set_margin_top(8);
    status_label.set_margin_start(8);
    status_label.add_css_class("title-4");

    let video_overlay = gtk4::Overlay::new();
    video_overlay.set_hexpand(true);
    video_overlay.set_vexpand(true);
    video_overlay.set_child(Some(&picture));
    video_overlay.add_overlay(&status_label);

    let name_label = gtk4::Label::new(Some(&format!("Camera {camera_id}")));
    name_label.set_hexpand(true);
    name_label.set_halign(gtk4::Align::Start);
    name_label.add_css_class("heading");

    let audio_button = gtk4::ToggleButton::with_label("Audio");
    audio_button.set_tooltip_text(Some("Make this the only audible camera"));

    let remove_button = gtk4::Button::with_label("Remove");
    remove_button.set_visible(editable);

    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    actions.set_margin_top(6);
    actions.set_margin_bottom(6);
    actions.set_margin_start(8);
    actions.set_margin_end(8);
    actions.append(&name_label);
    actions.append(&audio_button);
    actions.append(&remove_button);

    let decoder_label = gtk4::Label::new(Some("Decoder: waiting for stream"));
    decoder_label.set_halign(gtk4::Align::Start);
    decoder_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    decoder_label.set_margin_bottom(6);
    decoder_label.set_margin_start(8);
    decoder_label.set_margin_end(8);

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content.append(&video_overlay);
    content.append(&actions);
    content.append(&decoder_label);

    let root = gtk4::Frame::builder().child(&content).build();
    root.set_hexpand(true);
    root.set_vexpand(true);

    let status_for_events = status_label.clone();
    let decoder_for_events = decoder_label.clone();
    let picture_for_events = picture.clone();
    let audio_for_events = audio_button.clone();
    let controller = match CameraController::new(camera_id, uri, move |event| match event {
        PlaybackEvent::StateChanged(state) => {
            status_for_events.set_text(&state_label(state));
        }
        PlaybackEvent::PaintableChanged(paintable) => {
            picture_for_events.set_paintable(Some(&paintable));
            decoder_for_events.set_text("Decoder: waiting for stream");
        }
        PlaybackEvent::DecoderChanged(decoder) => {
            decoder_for_events.set_text(&decoder_label_text(&decoder));
        }
        PlaybackEvent::AudioDisabled(error) => {
            audio_for_events.set_active(false);
            status_for_events.set_text(&error);
        }
        PlaybackEvent::Error(error) => {
            status_for_events.set_text(&format!("Stream error: {error}"));
        }
    }) {
        Ok(controller) => controller,
        Err(error) => {
            show_error(state, &format!("Cannot add camera: {error:#}"));
            tracing::warn!(camera = camera_id, error = %error, "could not create camera controller");
            return false;
        }
    };

    let tile = Rc::new(CameraTile {
        id: camera_id,
        root,
        controller,
        audio_button,
    });

    tile.audio_button.connect_toggled({
        let weak_state = Rc::downgrade(state);
        let weak_tile = Rc::downgrade(&tile);
        move |button| {
            let (Some(state), Some(tile)) = (weak_state.upgrade(), weak_tile.upgrade()) else {
                return;
            };
            let guard = state.borrow().audio_update_guard.clone();
            if guard.get() {
                return;
            }
            set_audible_camera(&state, tile.id, button.is_active());
        }
    });

    remove_button.connect_clicked({
        let weak_state = Rc::downgrade(state);
        let weak_tile = Rc::downgrade(&tile);
        move |_| {
            let (Some(state), Some(tile)) = (weak_state.upgrade(), weak_tile.upgrade()) else {
                return;
            };
            remove_camera(&state, tile.id);
        }
    });

    flow_box.append(&tile.root);
    {
        let mut state = state.borrow_mut();
        state.tiles.push(tile.clone());
        state.error_label.set_visible(false);
    }
    update_grid_columns(state);
    tile.controller.start();
    true
}

fn remove_camera(state: &Rc<RefCell<AppState>>, camera_id: u64) {
    let removed = {
        let mut state = state.borrow_mut();
        let Some(index) = state.tiles.iter().position(|tile| tile.id == camera_id) else {
            return;
        };
        let tile = state.tiles.remove(index);
        state.flow_box.remove(&tile.root);
        tile
    };

    removed.controller.stop();
    drop(removed);
    update_grid_columns(state);
}

fn set_audible_camera(state: &Rc<RefCell<AppState>>, camera_id: u64, audible: bool) {
    let (tiles, guard) = {
        let state = state.borrow();
        (state.tiles.clone(), state.audio_update_guard.clone())
    };

    guard.set(true);
    for tile in &tiles {
        if tile.id != camera_id {
            tile.controller.set_muted(true);
            tile.audio_button.set_active(false);
        }
    }
    if let Some(tile) = tiles.iter().find(|tile| tile.id == camera_id) {
        tile.controller.set_muted(!audible);
        tile.audio_button.set_active(audible);
    }
    guard.set(false);
}

fn update_grid_columns(state: &Rc<RefCell<AppState>>) {
    let state = state.borrow();
    let columns = match state.tiles.len() {
        0 | 1 => 1,
        2..=4 => 2,
        5..=9 => 3,
        _ => 4,
    };
    state.flow_box.set_max_children_per_line(columns);
}

fn show_error(state: &Rc<RefCell<AppState>>, message: &str) {
    let state = state.borrow();
    state.error_label.set_text(message);
    state.error_label.set_visible(state.editable);
}

fn state_label(state: PlaybackState) -> String {
    match state {
        PlaybackState::Stopped => "Stopped".to_owned(),
        PlaybackState::Starting => "Connecting…".to_owned(),
        PlaybackState::Playing => "Live".to_owned(),
        PlaybackState::Stalled => "Stream stalled".to_owned(),
        PlaybackState::Reconnecting(seconds) => format!("Reconnecting in {seconds}s…"),
        PlaybackState::Failed => "Stream failed".to_owned(),
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
