mod layout;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use anyhow::Result;
use gtk4::prelude::*;
use uuid::Uuid;

use crate::application::Options;
use crate::config::{AppConfig, CameraConfig, ConfigStore, MAX_GRID_EXTENT, ViewConfig, ViewTile};
use crate::media::{CameraController, ConnectionTest, DecoderInfo, PlaybackEvent, PlaybackState};
use layout::LayoutError;

const MAX_CAMERAS: usize = 10;

struct AppState {
    grid: gtk4::Grid,
    window: gtk4::glib::WeakRef<gtk4::ApplicationWindow>,
    controls: gtk4::Box,
    edit_controls: gtk4::Box,
    error_label: gtk4::Label,
    view_dropdown: gtk4::DropDown,
    view_ids: Vec<Uuid>,
    view_change_guard: Rc<Cell<bool>>,
    tiles: Vec<Rc<CameraTile>>,
    next_runtime_id: u64,
    audio_update_guard: Rc<Cell<bool>>,
    kiosk_mode: bool,
    pointer_timeout: Option<gtk4::glib::SourceId>,
    editing: Option<EditSession>,
    expanded_camera: Option<u64>,
    grid_control_guard: Rc<Cell<bool>>,
    columns_spin: gtk4::SpinButton,
    rows_spin: gtk4::SpinButton,
    add_camera_dropdown: gtk4::DropDown,
    add_camera_ids: Vec<Uuid>,
    layout_status: gtk4::Label,
    grid_background: Option<gtk4::Box>,
    preview: gtk4::Frame,
    discard_confirmation_open: bool,
    config: AppConfig,
    store: ConfigStore,
    current_view: Uuid,
    session_urls: Vec<String>,
    config_save_in_progress: bool,
}

struct CameraTile {
    id: u64,
    camera_id: Option<Uuid>,
    root: gtk4::Frame,
    controller: CameraController,
    audio_button: gtk4::ToggleButton,
    placement: RefCell<ViewTile>,
    edit_actions: gtk4::Box,
}

struct EditSession {
    original: ViewConfig,
    working: ViewConfig,
    interaction: Option<LayoutInteraction>,
}

#[derive(Clone, Copy)]
enum InteractionKind {
    Move,
    Resize,
}

struct LayoutInteraction {
    camera_id: Uuid,
    origin: ViewTile,
    kind: InteractionKind,
    last_valid: Option<ViewConfig>,
}

pub(crate) fn build_main_window(application: &gtk4::Application, options: &Options) {
    let store = match ConfigStore::from_override(options.config.as_deref()) {
        Ok(store) => store,
        Err(error) => {
            show_startup_error(
                application,
                "Cannot locate configuration",
                &format!("{error:#}"),
            );
            return;
        }
    };
    let application = application.clone();
    let options = options.clone();
    let hold = application.hold();
    gtk4::glib::spawn_future_local(async move {
        let load_store = store.clone();
        let result = gtk4::gio::spawn_blocking(move || load_store.load()).await;
        drop(hold);
        match result {
            Ok(Ok(config)) => build_loaded_main_window(&application, &options, store, config),
            Ok(Err(error)) => show_startup_error(
                &application,
                "Cannot load Camview configuration",
                &format!("{error:#}\n\nThe existing file was not modified."),
            ),
            Err(_) => show_startup_error(
                &application,
                "Cannot load Camview configuration",
                "The configuration loader terminated unexpectedly.",
            ),
        }
    });
}

fn build_loaded_main_window(
    application: &gtk4::Application,
    options: &Options,
    store: ConfigStore,
    config: AppConfig,
) {
    let current_view = resolve_startup_view(&config, options.view.as_deref())
        .or_else(|| config.views.first().map(|view| view.id))
        .expect("validated default configuration always contains a view");
    let kiosk = options.kiosk || (config.kiosk_on_start && !options.windowed);

    let view_model = gtk4::StringList::new(&[]);
    let view_dropdown = gtk4::DropDown::builder().model(&view_model).build();
    view_dropdown.set_hexpand(false);
    view_dropdown.set_tooltip_text(Some("Select a saved camera view"));

    let cameras_button = gtk4::Button::with_label("Cameras…");
    let views_button = gtk4::Button::with_label("Views…");
    let edit_button = gtk4::Button::with_label("Edit layout");

    let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    controls.set_margin_top(12);
    controls.set_margin_bottom(6);
    controls.set_margin_start(12);
    controls.set_margin_end(12);
    controls.append(&gtk4::Label::new(Some("View")));
    controls.append(&view_dropdown);
    controls.append(&cameras_button);
    controls.append(&views_button);
    controls.append(&edit_button);

    let cancel_edit = gtk4::Button::with_label("Cancel");
    let save_edit = gtk4::Button::with_label("Save layout");
    save_edit.add_css_class("suggested-action");
    let columns_spin = gtk4::SpinButton::with_range(1.0, MAX_GRID_EXTENT as f64, 1.0);
    let rows_spin = gtk4::SpinButton::with_range(1.0, MAX_GRID_EXTENT as f64, 1.0);
    let add_camera_dropdown = gtk4::DropDown::from_strings(&[]);
    let add_camera_button = gtk4::Button::with_label("Add camera");
    let layout_status = manager_status_label();
    layout_status.set_hexpand(true);
    let edit_controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    edit_controls.set_margin_top(12);
    edit_controls.set_margin_bottom(6);
    edit_controls.set_margin_start(12);
    edit_controls.set_margin_end(12);
    edit_controls.append(&gtk4::Label::new(Some("Columns")));
    edit_controls.append(&columns_spin);
    edit_controls.append(&gtk4::Label::new(Some("Rows")));
    edit_controls.append(&rows_spin);
    edit_controls.append(&add_camera_dropdown);
    edit_controls.append(&add_camera_button);
    edit_controls.append(&layout_status);
    edit_controls.append(&cancel_edit);
    edit_controls.append(&save_edit);
    edit_controls.set_visible(false);

    let error_label = gtk4::Label::new(None);
    error_label.set_halign(gtk4::Align::Start);
    error_label.set_margin_bottom(6);
    error_label.set_margin_start(12);
    error_label.set_margin_end(12);
    error_label.add_css_class("error");
    error_label.set_visible(false);

    let grid = gtk4::Grid::builder()
        .column_spacing(8)
        .row_spacing(8)
        .column_homogeneous(true)
        .row_homogeneous(true)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();

    let scroller = gtk4::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .child(&grid)
        .build();

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.append(&controls);
    root.append(&edit_controls);
    root.append(&error_label);
    root.append(&scroller);

    if kiosk {
        controls.set_visible(false);
        error_label.set_visible(false);
    }

    let window = gtk4::ApplicationWindow::builder()
        .application(application)
        .title("Camview")
        .default_width(1_280)
        .default_height(720)
        .child(&root)
        .build();
    let preview = gtk4::Frame::new(None);
    preview.set_can_target(false);
    preview.add_css_class("layout-preview");
    preview.set_visible(false);
    let css = gtk4::CssProvider::new();
    css.load_from_bytes(&gtk4::glib::Bytes::from_static(
        b".layout-preview { background-color: rgba(53, 132, 228, 0.35); border: 3px solid #3584e4; }\
          .layout-preview.invalid { background-color: rgba(224, 27, 36, 0.35); border-color: #e01b24; }",
    ));
    gtk4::style_context_add_provider_for_display(
        &gtk4::prelude::WidgetExt::display(&window),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let state = Rc::new(RefCell::new(AppState {
        grid: grid.clone(),
        window: window.downgrade(),
        controls: controls.clone(),
        edit_controls: edit_controls.clone(),
        error_label: error_label.clone(),
        view_dropdown: view_dropdown.clone(),
        view_ids: Vec::new(),
        view_change_guard: Rc::new(Cell::new(false)),
        tiles: Vec::new(),
        next_runtime_id: 1,
        audio_update_guard: Rc::new(Cell::new(false)),
        kiosk_mode: kiosk,
        pointer_timeout: None,
        editing: None,
        expanded_camera: None,
        grid_control_guard: Rc::new(Cell::new(false)),
        columns_spin: columns_spin.clone(),
        rows_spin: rows_spin.clone(),
        add_camera_dropdown: add_camera_dropdown.clone(),
        add_camera_ids: Vec::new(),
        layout_status: layout_status.clone(),
        grid_background: None,
        preview,
        discard_confirmation_open: false,
        config,
        store,
        current_view,
        session_urls: options.rtsp_url.clone(),
        config_save_in_progress: false,
    }));

    view_dropdown.connect_selected_notify({
        let state = state.clone();
        move |dropdown| {
            let (guard, view_id) = {
                let state = state.borrow();
                let selected = dropdown.selected() as usize;
                (
                    state.view_change_guard.clone(),
                    state.view_ids.get(selected).copied(),
                )
            };
            if guard.get() {
                return;
            }
            if let Some(view_id) = view_id {
                state.borrow_mut().current_view = view_id;
                apply_current_view(&state);
            }
        }
    });

    cameras_button.connect_clicked({
        let state = state.clone();
        let window = window.clone();
        move |_| show_camera_manager(window.upcast_ref(), &state)
    });
    views_button.connect_clicked({
        let state = state.clone();
        let window = window.clone();
        move |_| show_view_manager(window.upcast_ref(), &state)
    });
    edit_button.connect_clicked({
        let state = state.clone();
        move |_| enter_layout_edit(&state)
    });
    cancel_edit.connect_clicked({
        let state = state.clone();
        move |_| cancel_layout_edit(&state)
    });
    save_edit.connect_clicked({
        let state = state.clone();
        move |button| save_layout_edit(&state, button)
    });
    columns_spin.connect_value_changed({
        let state = state.clone();
        move |spin| {
            if state.borrow().grid_control_guard.get() {
                return;
            }
            let columns = spin.value_as_int() as u32;
            let rows = state
                .borrow()
                .editing
                .as_ref()
                .map_or(1, |editing| editing.working.rows);
            update_editing_view(&state, |view| layout::set_dimensions(view, columns, rows));
        }
    });
    rows_spin.connect_value_changed({
        let state = state.clone();
        move |spin| {
            if state.borrow().grid_control_guard.get() {
                return;
            }
            let rows = spin.value_as_int() as u32;
            let columns = state
                .borrow()
                .editing
                .as_ref()
                .map_or(1, |editing| editing.working.columns);
            update_editing_view(&state, |view| layout::set_dimensions(view, columns, rows));
        }
    });
    add_camera_button.connect_clicked({
        let state = state.clone();
        move |_| {
            let camera_id = {
                let state = state.borrow();
                state
                    .add_camera_ids
                    .get(state.add_camera_dropdown.selected() as usize)
                    .copied()
            };
            if let Some(camera_id) = camera_id {
                update_editing_view(&state, |view| layout::add_camera(view, camera_id));
            }
        }
    });

    let key_controller = gtk4::EventControllerKey::new();
    key_controller.connect_key_pressed({
        let state = state.clone();
        move |_, key, _, _| {
            if key == gtk4::gdk::Key::F11 {
                let enabled = !state.borrow().kiosk_mode;
                set_kiosk_mode(&state, enabled);
                return gtk4::glib::Propagation::Stop;
            }
            if key == gtk4::gdk::Key::Escape && state.borrow().expanded_camera.is_some() {
                collapse_expanded_camera(&state);
                return gtk4::glib::Propagation::Stop;
            }
            gtk4::glib::Propagation::Proceed
        }
    });
    window.add_controller(key_controller);

    let motion = gtk4::EventControllerMotion::new();
    motion.connect_motion({
        let state = state.clone();
        move |_, _, _| note_kiosk_pointer_activity(&state)
    });
    motion.connect_enter({
        let state = state.clone();
        move |_, _, _| note_kiosk_pointer_activity(&state)
    });
    window.add_controller(motion);

    window.connect_close_request({
        let state = state.clone();
        move |_| {
            if state.borrow().config_save_in_progress {
                state
                    .borrow()
                    .layout_status
                    .set_text("Wait for the layout save to finish before closing.");
                return gtk4::glib::Propagation::Stop;
            }
            if state.borrow().editing.is_some() {
                show_discard_layout_confirmation(&state);
                return gtk4::glib::Propagation::Stop;
            }
            if let Some(timeout) = state.borrow_mut().pointer_timeout.take() {
                timeout.remove();
            }
            stop_all_tiles(&state);
            gtk4::glib::Propagation::Proceed
        }
    });

    if kiosk {
        window.fullscreen();
    }

    tracing::info!(
        kiosk,
        requested_view = options.view.as_deref(),
        custom_config = options.config.is_some(),
        config_path = %state.borrow().store.path().display(),
        "presenting application window"
    );

    refresh_view_dropdown(&state);
    window.present();
    apply_current_view(&state);
    if kiosk {
        note_kiosk_pointer_activity(&state);
    }
}

fn resolve_startup_view(config: &AppConfig, requested: Option<&str>) -> Option<Uuid> {
    if let Some(requested) = requested {
        if let Ok(id) = Uuid::parse_str(requested)
            && config.views.iter().any(|view| view.id == id)
        {
            return Some(id);
        }
        if let Some(view) = config
            .views
            .iter()
            .find(|view| view.name.eq_ignore_ascii_case(requested))
        {
            return Some(view.id);
        }
        tracing::warn!(
            requested_view = requested,
            "requested view does not exist; using configured startup view"
        );
    }
    config.startup_view
}

fn show_startup_error(application: &gtk4::Application, title: &str, message: &str) {
    let label = gtk4::Label::builder()
        .label(message)
        .wrap(true)
        .selectable(true)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    let window = gtk4::ApplicationWindow::builder()
        .application(application)
        .title(title)
        .default_width(640)
        .child(&label)
        .build();
    window.present();
}

fn refresh_view_dropdown(state: &Rc<RefCell<AppState>>) {
    let (dropdown, guard, names, ids, current_view) = {
        let state = state.borrow();
        (
            state.view_dropdown.clone(),
            state.view_change_guard.clone(),
            state
                .config
                .views
                .iter()
                .map(|view| view.name.clone())
                .collect::<Vec<_>>(),
            state
                .config
                .views
                .iter()
                .map(|view| view.id)
                .collect::<Vec<_>>(),
            state.current_view,
        )
    };
    let selected = ids
        .iter()
        .position(|id| *id == current_view)
        .unwrap_or_default() as u32;
    let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
    let model = gtk4::StringList::new(&name_refs);

    guard.set(true);
    {
        let mut state = state.borrow_mut();
        state.view_ids = ids;
    }
    dropdown.set_model(Some(&model));
    dropdown.set_selected(selected);
    guard.set(false);
}

fn apply_current_view(state: &Rc<RefCell<AppState>>) {
    state.borrow_mut().expanded_camera = None;
    stop_all_tiles(state);
    hide_error(state);
    let (view, configured_cameras, session_urls) = {
        let state = state.borrow();
        let Some(view) = state
            .config
            .views
            .iter()
            .find(|view| view.id == state.current_view)
        else {
            return;
        };
        let cameras = view
            .tiles
            .iter()
            .filter_map(|tile| {
                state
                    .config
                    .cameras
                    .iter()
                    .find(|camera| camera.id == tile.camera_id)
                    .cloned()
                    .map(|camera| (camera, tile.clone()))
            })
            .collect::<Vec<_>>();
        (view.clone(), cameras, state.session_urls.clone())
    };

    let mut occupied = occupied_cells(&view.tiles);
    let mut rows = view.rows;
    let mut session_tiles = Vec::new();
    for uri in session_urls
        .iter()
        .take(MAX_CAMERAS.saturating_sub(configured_cameras.len()))
    {
        let (column, row) = next_free_cell(view.columns, rows, &occupied).unwrap_or_else(|| {
            let row = rows;
            rows += 1;
            (0, row)
        });
        occupied.insert((column, row));
        session_tiles.push((uri.clone(), column, row));
    }

    attach_grid_background(state, view.columns, rows);
    for (camera, tile) in configured_cameras.into_iter().take(MAX_CAMERAS) {
        let uri = camera
            .substream_url
            .as_deref()
            .unwrap_or(&camera.rtsp_url)
            .to_owned();
        add_runtime_camera(state, Some(camera.id), &camera.name, &uri, &tile);
    }
    for (index, (uri, column, row)) in session_tiles.into_iter().enumerate() {
        let tile = ViewTile {
            camera_id: Uuid::nil(),
            column,
            row,
            column_span: 1,
            row_span: 1,
        };
        add_runtime_camera(
            state,
            None,
            &format!("Command-line camera {}", index + 1),
            &uri,
            &tile,
        );
    }
    if session_urls.len() + configured_cameras_len(state) > MAX_CAMERAS {
        show_error(
            state,
            &format!("Only {MAX_CAMERAS} cameras can be displayed in one view."),
        );
    }
}

fn add_runtime_camera(
    state: &Rc<RefCell<AppState>>,
    persistent_camera_id: Option<Uuid>,
    name: &str,
    uri: &str,
    placement: &ViewTile,
) -> bool {
    if state.borrow().tiles.len() >= MAX_CAMERAS {
        return false;
    }
    let (camera_id, grid) = {
        let mut state = state.borrow_mut();
        let camera_id = state.next_runtime_id;
        state.next_runtime_id += 1;
        (camera_id, state.grid.clone())
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

    let name_label = gtk4::Label::new(Some(name));
    name_label.set_hexpand(true);
    name_label.set_halign(gtk4::Align::Start);
    name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    name_label.add_css_class("heading");

    let audio_button = gtk4::ToggleButton::with_label("Audio");
    audio_button.set_tooltip_text(Some("Make this the only audible camera"));
    let remove_button = gtk4::Button::with_label("Remove");
    let resize_handle = gtk4::Button::with_label("↘");
    resize_handle.set_tooltip_text(Some("Drag to resize this tile"));
    let edit_actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    edit_actions.append(&remove_button);
    edit_actions.append(&resize_handle);
    edit_actions.set_visible(false);

    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    actions.set_margin_top(6);
    actions.set_margin_bottom(6);
    actions.set_margin_start(8);
    actions.set_margin_end(8);
    actions.append(&name_label);
    actions.append(&audio_button);
    actions.append(&edit_actions);

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
            show_error(state, &format!("Cannot start '{name}': {error:#}"));
            return false;
        }
    };

    let tile = Rc::new(CameraTile {
        id: camera_id,
        camera_id: persistent_camera_id,
        root,
        controller,
        audio_button,
        placement: RefCell::new(placement.clone()),
        edit_actions,
    });
    tile.audio_button.connect_toggled({
        let weak_state = Rc::downgrade(state);
        let weak_tile = Rc::downgrade(&tile);
        move |button| {
            let (Some(state), Some(tile)) = (weak_state.upgrade(), weak_tile.upgrade()) else {
                return;
            };
            let guard = state.borrow().audio_update_guard.clone();
            if !guard.get() {
                set_audible_camera(&state, tile.id, button.is_active());
            }
        }
    });
    setup_tile_interactions(
        state,
        &tile,
        &name_label,
        &resize_handle,
        &remove_button,
        &video_overlay,
    );

    grid.attach(
        &tile.root,
        placement.column as i32,
        placement.row as i32,
        placement.column_span as i32,
        placement.row_span as i32,
    );
    state.borrow_mut().tiles.push(tile.clone());
    tile.controller.start();
    true
}

fn stop_all_tiles(state: &Rc<RefCell<AppState>>) {
    let (grid, tiles) = {
        let mut state = state.borrow_mut();
        (state.grid.clone(), std::mem::take(&mut state.tiles))
    };
    for tile in tiles {
        tile.controller.stop();
    }
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
    state.borrow_mut().grid_background = None;
}

fn setup_tile_interactions(
    state: &Rc<RefCell<AppState>>,
    tile: &Rc<CameraTile>,
    move_handle: &gtk4::Label,
    resize_handle: &gtk4::Button,
    remove_button: &gtk4::Button,
    video: &gtk4::Overlay,
) {
    let move_gesture = gtk4::GestureDrag::new();
    move_gesture.connect_drag_begin({
        let state = state.clone();
        let tile = tile.clone();
        move |_, _, _| begin_layout_interaction(&state, &tile, InteractionKind::Move)
    });
    move_gesture.connect_drag_update({
        let state = state.clone();
        move |_, x, y| update_layout_interaction(&state, x, y)
    });
    move_gesture.connect_drag_end({
        let state = state.clone();
        move |_, _, _| finish_layout_interaction(&state)
    });
    move_handle.add_controller(move_gesture);

    let resize_gesture = gtk4::GestureDrag::new();
    resize_gesture.connect_drag_begin({
        let state = state.clone();
        let tile = tile.clone();
        move |_, _, _| begin_layout_interaction(&state, &tile, InteractionKind::Resize)
    });
    resize_gesture.connect_drag_update({
        let state = state.clone();
        move |_, x, y| update_layout_interaction(&state, x, y)
    });
    resize_gesture.connect_drag_end({
        let state = state.clone();
        move |_, _, _| finish_layout_interaction(&state)
    });
    resize_handle.add_controller(resize_gesture);

    remove_button.connect_clicked({
        let state = state.clone();
        let tile = tile.clone();
        move |_| {
            let Some(camera_id) = tile.camera_id else {
                return;
            };
            update_editing_view(&state, |view| layout::remove_camera(view, camera_id));
        }
    });

    let expand = gtk4::GestureClick::new();
    expand.connect_released({
        let state = state.clone();
        let tile = tile.clone();
        move |_, presses, _, _| {
            if presses == 2 {
                toggle_expanded_camera(&state, tile.id);
            }
        }
    });
    video.add_controller(expand);
}

fn begin_layout_interaction(
    state: &Rc<RefCell<AppState>>,
    tile: &Rc<CameraTile>,
    kind: InteractionKind,
) {
    let Some(camera_id) = tile.camera_id else {
        return;
    };
    let mut state = state.borrow_mut();
    let Some(editing) = &mut state.editing else {
        return;
    };
    let Some(origin) = layout::tile(&editing.working, camera_id).cloned() else {
        return;
    };
    editing.interaction = Some(LayoutInteraction {
        camera_id,
        origin,
        kind,
        last_valid: None,
    });
}

fn update_layout_interaction(state: &Rc<RefCell<AppState>>, x: f64, y: f64) {
    let (columns, rows, width, height, interaction, working) = {
        let state = state.borrow();
        let Some(editing) = &state.editing else {
            return;
        };
        let Some(interaction) = &editing.interaction else {
            return;
        };
        (
            editing.working.columns,
            editing.working.rows,
            state.grid.width().max(1) as f64,
            state.grid.height().max(1) as f64,
            (
                interaction.camera_id,
                interaction.origin.clone(),
                interaction.kind,
            ),
            editing.working.clone(),
        )
    };
    let cell_width =
        ((width - f64::from(columns.saturating_sub(1)) * 8.0) / f64::from(columns)).max(1.0);
    let cell_height =
        ((height - f64::from(rows.saturating_sub(1)) * 8.0) / f64::from(rows)).max(1.0);
    let column_delta = (x / cell_width).round() as i64;
    let row_delta = (y / cell_height).round() as i64;
    let (camera_id, origin, kind) = interaction;
    let mut preview = origin.clone();
    let proposal = match kind {
        InteractionKind::Move => {
            preview.column =
                ((i64::from(origin.column) + column_delta).max(0) as u32).min(MAX_GRID_EXTENT);
            preview.row = ((i64::from(origin.row) + row_delta).max(0) as u32).min(MAX_GRID_EXTENT);
            layout::move_tile(&working, camera_id, preview.column, preview.row)
        }
        InteractionKind::Resize => {
            preview.column_span =
                ((i64::from(origin.column_span) + column_delta).max(1) as u32).min(MAX_GRID_EXTENT);
            preview.row_span =
                ((i64::from(origin.row_span) + row_delta).max(1) as u32).min(MAX_GRID_EXTENT);
            layout::resize_tile(&working, camera_id, preview.column_span, preview.row_span)
        }
    };
    let valid = proposal.is_ok();
    {
        let mut state = state.borrow_mut();
        if let Some(interaction) = state
            .editing
            .as_mut()
            .and_then(|editing| editing.interaction.as_mut())
        {
            interaction.last_valid = proposal.ok();
        }
        state.layout_status.set_text(if valid {
            ""
        } else {
            "That placement overlaps another tile or exceeds the grid."
        });
    }
    show_layout_preview(state, &preview, valid);
}

fn finish_layout_interaction(state: &Rc<RefCell<AppState>>) {
    let candidate = {
        let mut state = state.borrow_mut();
        state.preview.set_visible(false);
        state
            .editing
            .as_mut()
            .and_then(|editing| editing.interaction.take())
            .and_then(|interaction| interaction.last_valid)
    };
    if let Some(candidate) = candidate
        && let Some(editing) = &mut state.borrow_mut().editing
    {
        editing.working = candidate;
    }
    render_editing_grid(state);
}

fn show_layout_preview(state: &Rc<RefCell<AppState>>, tile: &ViewTile, valid: bool) {
    let state = state.borrow();
    if state.preview.parent().is_some() {
        state.grid.remove(&state.preview);
    }
    state.preview.remove_css_class("invalid");
    if !valid {
        state.preview.add_css_class("invalid");
    }
    state.grid.attach(
        &state.preview,
        tile.column as i32,
        tile.row as i32,
        tile.column_span as i32,
        tile.row_span as i32,
    );
    state.preview.set_visible(true);
}

fn update_editing_view(
    state: &Rc<RefCell<AppState>>,
    update: impl FnOnce(&ViewConfig) -> Result<ViewConfig, LayoutError>,
) {
    let working = {
        let state = state.borrow();
        let Some(editing) = &state.editing else {
            return;
        };
        editing.working.clone()
    };
    match update(&working) {
        Ok(candidate) => {
            if let Some(editing) = &mut state.borrow_mut().editing {
                editing.working = candidate;
            }
            state.borrow().layout_status.set_text("");
            render_editing_grid(state);
        }
        Err(error) => {
            state.borrow().layout_status.set_text(&error.to_string());
            refresh_layout_controls(state);
        }
    }
}

fn enter_layout_edit(state: &Rc<RefCell<AppState>>) {
    let view = {
        let state = state.borrow();
        if state.kiosk_mode || state.editing.is_some() {
            return;
        }
        let Some(view) = state
            .config
            .views
            .iter()
            .find(|view| view.id == state.current_view)
        else {
            return;
        };
        view.clone()
    };
    collapse_expanded_camera(state);
    {
        let mut state = state.borrow_mut();
        state.editing = Some(EditSession {
            original: view.clone(),
            working: view,
            interaction: None,
        });
        state.controls.set_visible(false);
        state.edit_controls.set_visible(true);
        state.layout_status.set_text("");
    }
    remove_transient_tiles(state);
    refresh_layout_controls(state);
    render_editing_grid(state);
}

fn cancel_layout_edit(state: &Rc<RefCell<AppState>>) {
    let original = state
        .borrow_mut()
        .editing
        .take()
        .map(|editing| editing.original);
    if original.is_none() {
        return;
    }
    {
        let state = state.borrow();
        state.edit_controls.set_visible(false);
        state.controls.set_visible(!state.kiosk_mode);
    }
    apply_current_view(state);
}

fn save_layout_edit(state: &Rc<RefCell<AppState>>, save_button: &gtk4::Button) {
    let working = match state.borrow().editing.as_ref() {
        Some(editing) => editing.working.clone(),
        None => return,
    };
    let mut candidate = state.borrow().config.clone();
    let Some(index) = candidate
        .views
        .iter()
        .position(|view| view.id == working.id)
    else {
        return;
    };
    candidate.views[index] = working;
    save_button.set_sensitive(false);
    state.borrow().edit_controls.set_sensitive(false);
    let save_button = save_button.clone();
    let state_for_callback = state.clone();
    commit_config(state, candidate, move |result| {
        save_button.set_sensitive(true);
        state_for_callback
            .borrow()
            .edit_controls
            .set_sensitive(true);
        match result {
            Ok(()) => {
                state_for_callback.borrow_mut().editing = None;
                {
                    let state = state_for_callback.borrow();
                    state.edit_controls.set_visible(false);
                    state.controls.set_visible(!state.kiosk_mode);
                }
                apply_current_view(&state_for_callback);
            }
            Err(error) => state_for_callback
                .borrow()
                .layout_status
                .set_text(&format!("Could not save layout: {error:#}")),
        }
    });
}

fn remove_transient_tiles(state: &Rc<RefCell<AppState>>) {
    let removed = {
        let mut state = state.borrow_mut();
        let mut removed = Vec::new();
        let mut retained = Vec::new();
        for tile in std::mem::take(&mut state.tiles) {
            if tile.camera_id.is_none() {
                removed.push(tile);
            } else {
                retained.push(tile);
            }
        }
        state.tiles = retained;
        removed
    };
    for tile in removed {
        if tile.root.parent().is_some() {
            state.borrow().grid.remove(&tile.root);
        }
        tile.controller.stop();
    }
}

fn render_editing_grid(state: &Rc<RefCell<AppState>>) {
    let working = match state.borrow().editing.as_ref() {
        Some(editing) => editing.working.clone(),
        None => return,
    };
    let existing = state.borrow().tiles.clone();
    let wanted = working
        .tiles
        .iter()
        .map(|tile| tile.camera_id)
        .collect::<std::collections::HashSet<_>>();
    let removed = {
        let mut state = state.borrow_mut();
        let mut retained = Vec::new();
        let mut removed = Vec::new();
        for tile in std::mem::take(&mut state.tiles) {
            if tile.camera_id.is_some_and(|id| wanted.contains(&id)) {
                retained.push(tile);
            } else {
                removed.push(tile);
            }
        }
        state.tiles = retained;
        removed
    };
    for tile in removed {
        tile.controller.stop();
    }
    clear_grid(state);
    attach_grid_background(state, working.columns, working.rows);

    for placement in &working.tiles {
        if let Some(tile) = existing
            .iter()
            .find(|tile| tile.camera_id == Some(placement.camera_id))
            .cloned()
        {
            *tile.placement.borrow_mut() = placement.clone();
            tile.edit_actions.set_visible(true);
            state.borrow().grid.attach(
                &tile.root,
                placement.column as i32,
                placement.row as i32,
                placement.column_span as i32,
                placement.row_span as i32,
            );
            continue;
        }
        let camera = state
            .borrow()
            .config
            .cameras
            .iter()
            .find(|camera| camera.id == placement.camera_id)
            .cloned();
        if let Some(camera) = camera {
            let uri = camera
                .substream_url
                .as_deref()
                .unwrap_or(&camera.rtsp_url)
                .to_owned();
            add_runtime_camera(state, Some(camera.id), &camera.name, &uri, placement);
            if let Some(tile) = state.borrow().tiles.last() {
                tile.edit_actions.set_visible(true);
            }
        }
    }
    refresh_layout_controls(state);
}

fn clear_grid(state: &Rc<RefCell<AppState>>) {
    let grid = state.borrow().grid.clone();
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
    state.borrow_mut().grid_background = None;
}

fn refresh_layout_controls(state: &Rc<RefCell<AppState>>) {
    let (working, cameras, guard, columns_spin, rows_spin, dropdown) = {
        let state = state.borrow();
        let Some(editing) = &state.editing else {
            return;
        };
        (
            editing.working.clone(),
            state.config.cameras.clone(),
            state.grid_control_guard.clone(),
            state.columns_spin.clone(),
            state.rows_spin.clone(),
            state.add_camera_dropdown.clone(),
        )
    };
    guard.set(true);
    columns_spin.set_value(f64::from(working.columns));
    rows_spin.set_value(f64::from(working.rows));
    guard.set(false);

    let available = cameras
        .into_iter()
        .filter(|camera| !working.tiles.iter().any(|tile| tile.camera_id == camera.id))
        .collect::<Vec<_>>();
    let names = available
        .iter()
        .map(|camera| camera.name.as_str())
        .collect::<Vec<_>>();
    dropdown.set_model(Some(&gtk4::StringList::new(&names)));
    dropdown.set_selected(if available.is_empty() {
        gtk4::INVALID_LIST_POSITION
    } else {
        0
    });
    state.borrow_mut().add_camera_ids = available.iter().map(|camera| camera.id).collect();
}

fn toggle_expanded_camera(state: &Rc<RefCell<AppState>>, camera_id: u64) {
    if state.borrow().editing.is_some() {
        return;
    }
    if state.borrow().expanded_camera == Some(camera_id) {
        collapse_expanded_camera(state);
        return;
    }
    state.borrow_mut().expanded_camera = Some(camera_id);
    render_expanded_camera(state);
}

fn render_expanded_camera(state: &Rc<RefCell<AppState>>) {
    let (camera_id, tile, columns, rows) = {
        let state = state.borrow();
        let Some(camera_id) = state.expanded_camera else {
            return;
        };
        let Some(tile) = state
            .tiles
            .iter()
            .find(|tile| tile.id == camera_id)
            .cloned()
        else {
            return;
        };
        let view = state
            .config
            .views
            .iter()
            .find(|view| view.id == state.current_view);
        (
            camera_id,
            tile,
            view.map_or(1, |view| view.columns),
            view.map_or(1, |view| view.rows),
        )
    };
    clear_grid(state);
    attach_grid_background(state, columns, rows);
    state
        .borrow()
        .grid
        .attach(&tile.root, 0, 0, columns as i32, rows as i32);
    tracing::debug!(camera = camera_id, "expanded camera tile");
}

fn collapse_expanded_camera(state: &Rc<RefCell<AppState>>) {
    if state.borrow_mut().expanded_camera.take().is_none() {
        return;
    }
    render_runtime_grid(state);
}

fn render_runtime_grid(state: &Rc<RefCell<AppState>>) {
    let (tiles, columns, rows) = {
        let state = state.borrow();
        let view = state
            .config
            .views
            .iter()
            .find(|view| view.id == state.current_view);
        let columns = view.map_or(1, |view| view.columns);
        let rows = state
            .tiles
            .iter()
            .map(|tile| {
                let placement = tile.placement.borrow();
                placement.row + placement.row_span
            })
            .max()
            .unwrap_or_else(|| view.map_or(1, |view| view.rows))
            .max(view.map_or(1, |view| view.rows));
        (state.tiles.clone(), columns, rows)
    };
    clear_grid(state);
    attach_grid_background(state, columns, rows);
    for tile in tiles {
        let placement = tile.placement.borrow();
        state.borrow().grid.attach(
            &tile.root,
            placement.column as i32,
            placement.row as i32,
            placement.column_span as i32,
            placement.row_span as i32,
        );
    }
}

fn set_kiosk_mode(state: &Rc<RefCell<AppState>>, enabled: bool) {
    if state.borrow().kiosk_mode == enabled {
        return;
    }
    if state.borrow().config_save_in_progress {
        state
            .borrow()
            .layout_status
            .set_text("Wait for the layout save to finish before changing modes.");
        return;
    }
    if enabled && state.borrow().editing.is_some() {
        cancel_layout_edit(state);
    }
    collapse_expanded_camera(state);
    {
        let mut state = state.borrow_mut();
        state.kiosk_mode = enabled;
        state.controls.set_visible(!enabled);
        state.edit_controls.set_visible(false);
        state
            .error_label
            .set_visible(!enabled && !state.error_label.text().is_empty());
        if let Some(timeout) = state.pointer_timeout.take() {
            timeout.remove();
        }
        if let Some(window) = state.window.upgrade() {
            if enabled {
                window.fullscreen();
            } else {
                window.unfullscreen();
                window.set_cursor_from_name(None);
            }
        }
    }
    if enabled {
        note_kiosk_pointer_activity(state);
    }
}

fn note_kiosk_pointer_activity(state: &Rc<RefCell<AppState>>) {
    let mut state_ref = state.borrow_mut();
    if !state_ref.kiosk_mode {
        return;
    }
    if let Some(window) = state_ref.window.upgrade() {
        window.set_cursor_from_name(None);
    }
    if let Some(timeout) = state_ref.pointer_timeout.take() {
        timeout.remove();
    }
    let weak_state = Rc::downgrade(state);
    state_ref.pointer_timeout = Some(gtk4::glib::timeout_add_local_once(
        Duration::from_secs(3),
        move || {
            let Some(state) = weak_state.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            state.pointer_timeout.take();
            if state.kiosk_mode
                && let Some(window) = state.window.upgrade()
            {
                window.set_cursor_from_name(Some("none"));
            }
        },
    ));
}

fn show_discard_layout_confirmation(state: &Rc<RefCell<AppState>>) {
    {
        let mut state = state.borrow_mut();
        if state.discard_confirmation_open {
            return;
        }
        state.discard_confirmation_open = true;
    }
    let dialog = gtk4::AlertDialog::builder()
        .modal(true)
        .message("Discard unsaved layout changes?")
        .detail("The current layout edit has not been saved.")
        .buttons(["Keep editing", "Discard"])
        .cancel_button(0)
        .default_button(0)
        .build();
    let Some(window) = state.borrow().window.upgrade() else {
        return;
    };
    let state = state.clone();
    gtk4::glib::spawn_future_local(async move {
        let response = dialog.choose_future(Some(&window)).await;
        state.borrow_mut().discard_confirmation_open = false;
        if response == Ok(1) {
            state.borrow_mut().editing = None;
            stop_all_tiles(&state);
            window.close();
        }
    });
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

fn configured_cameras_len(state: &Rc<RefCell<AppState>>) -> usize {
    let state = state.borrow();
    state
        .config
        .views
        .iter()
        .find(|view| view.id == state.current_view)
        .map_or(0, |view| view.tiles.len())
}

fn occupied_cells(tiles: &[ViewTile]) -> std::collections::HashSet<(u32, u32)> {
    let mut occupied = std::collections::HashSet::new();
    for tile in tiles {
        for row in tile.row..tile.row + tile.row_span {
            for column in tile.column..tile.column + tile.column_span {
                occupied.insert((column, row));
            }
        }
    }
    occupied
}

fn next_free_cell(
    columns: u32,
    rows: u32,
    occupied: &std::collections::HashSet<(u32, u32)>,
) -> Option<(u32, u32)> {
    (0..rows)
        .flat_map(|row| (0..columns).map(move |column| (column, row)))
        .find(|cell| !occupied.contains(cell))
}

fn attach_grid_background(state: &Rc<RefCell<AppState>>, columns: u32, rows: u32) {
    let background = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    background.set_can_target(false);
    background.set_hexpand(true);
    background.set_vexpand(true);
    background.set_size_request(1, 1);
    state
        .borrow()
        .grid
        .attach(&background, 0, 0, columns as i32, rows as i32);
    state.borrow_mut().grid_background = Some(background);
}

fn commit_config<F>(state: &Rc<RefCell<AppState>>, candidate: AppConfig, callback: F)
where
    F: FnOnce(Result<()>) + 'static,
{
    if let Err(error) = candidate.validate() {
        callback(Err(error));
        return;
    }

    let store = {
        let mut state = state.borrow_mut();
        if state.config_save_in_progress {
            drop(state);
            callback(Err(anyhow::anyhow!(
                "another configuration update is still in progress"
            )));
            return;
        }
        state.config_save_in_progress = true;
        state.store.clone()
    };
    let state = state.clone();
    gtk4::glib::spawn_future_local(async move {
        let result = gtk4::gio::spawn_blocking(move || {
            store.save(&candidate)?;
            Ok::<_, anyhow::Error>(candidate)
        })
        .await;

        match result {
            Ok(Ok(candidate)) => {
                {
                    let mut state = state.borrow_mut();
                    state.config = candidate;
                    state.config_save_in_progress = false;
                }
                callback(Ok(()));
            }
            Ok(Err(error)) => {
                state.borrow_mut().config_save_in_progress = false;
                callback(Err(error));
            }
            Err(_) => {
                state.borrow_mut().config_save_in_progress = false;
                callback(Err(anyhow::anyhow!(
                    "configuration writer terminated unexpectedly"
                )));
            }
        }
    });
}

fn show_camera_manager(parent: &gtk4::Window, state: &Rc<RefCell<AppState>>) {
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    let status = manager_status_label();
    let add = gtk4::Button::with_label("Add camera…");
    add.add_css_class("suggested-action");
    let close = gtk4::Button::with_label("Close");
    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    actions.append(&add);
    actions.append(&close);
    let content = manager_content(&list, &status, &actions);
    let window = gtk4::Window::builder()
        .title("Camera manager")
        .transient_for(parent)
        .modal(true)
        .default_width(720)
        .default_height(480)
        .child(&content)
        .build();

    add.connect_clicked({
        let state = state.clone();
        let list = list.clone();
        let window = window.clone();
        let status = status.clone();
        move |_| show_camera_editor(&window, &state, &list, &status, None)
    });
    close.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    refresh_camera_list(&window, state, &list, &status);
    window.present();
}

fn refresh_camera_list(
    parent: &gtk4::Window,
    state: &Rc<RefCell<AppState>>,
    list: &gtk4::ListBox,
    status: &gtk4::Label,
) {
    clear_list(list);
    let cameras = state.borrow().config.cameras.clone();
    status.set_text(if cameras.is_empty() {
        "No cameras configured."
    } else {
        ""
    });

    for camera in cameras {
        let name = gtk4::Label::new(Some(&camera.name));
        name.set_hexpand(true);
        name.set_halign(gtk4::Align::Start);
        let edit = gtk4::Button::with_label("Edit…");
        let delete = gtk4::Button::with_label("Delete");
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row.set_margin_top(6);
        row.set_margin_bottom(6);
        row.set_margin_start(8);
        row.set_margin_end(8);
        row.append(&name);
        row.append(&edit);
        row.append(&delete);
        list.append(&row);

        edit.connect_clicked({
            let state = state.clone();
            let list = list.clone();
            let parent = parent.clone();
            let camera = camera.clone();
            let status = status.clone();
            move |_| show_camera_editor(&parent, &state, &list, &status, Some(camera.clone()))
        });
        delete.connect_clicked({
            let state = state.clone();
            let list = list.clone();
            let parent = parent.clone();
            let status = status.clone();
            move |button| {
                let mut candidate = state.borrow().config.clone();
                candidate.cameras.retain(|item| item.id != camera.id);
                for view in &mut candidate.views {
                    view.tiles.retain(|tile| tile.camera_id != camera.id);
                }
                button.set_sensitive(false);
                let button = button.clone();
                let state = state.clone();
                let parent = parent.clone();
                let list = list.clone();
                let status = status.clone();
                let commit_state = state.clone();
                commit_config(&commit_state, candidate, move |result| match result {
                    Ok(()) => {
                        apply_current_view(&state);
                        refresh_camera_list(&parent, &state, &list, &status);
                    }
                    Err(error) => {
                        button.set_sensitive(true);
                        status.set_text(&format!("Could not delete camera: {error:#}"));
                    }
                });
            }
        });
    }
}

fn show_camera_editor(
    parent: &gtk4::Window,
    state: &Rc<RefCell<AppState>>,
    manager_list: &gtk4::ListBox,
    manager_status: &gtk4::Label,
    existing: Option<CameraConfig>,
) {
    let name = gtk4::Entry::builder()
        .placeholder_text("Front door")
        .text(existing.as_ref().map_or("", |camera| &camera.name))
        .build();
    let url = gtk4::Entry::builder()
        .placeholder_text("rtsp://camera.local/stream")
        .text(existing.as_ref().map_or("", |camera| &camera.rtsp_url))
        .build();
    let substream = gtk4::Entry::builder()
        .placeholder_text("Optional lower-resolution RTSP URL")
        .text(
            existing
                .as_ref()
                .and_then(|camera| camera.substream_url.as_deref())
                .unwrap_or(""),
        )
        .build();
    let status = manager_status_label();
    let test = gtk4::Button::with_label("Test connection");
    let save = gtk4::Button::with_label("Save");
    save.add_css_class("suggested-action");
    let cancel = gtk4::Button::with_label("Cancel");
    let form = gtk4::Grid::builder()
        .column_spacing(8)
        .row_spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    form.attach(&gtk4::Label::new(Some("Name")), 0, 0, 1, 1);
    form.attach(&name, 1, 0, 1, 1);
    form.attach(&gtk4::Label::new(Some("RTSP URL")), 0, 1, 1, 1);
    form.attach(&url, 1, 1, 1, 1);
    form.attach(&gtk4::Label::new(Some("Substream")), 0, 2, 1, 1);
    form.attach(&substream, 1, 2, 1, 1);
    form.attach(&test, 1, 3, 1, 1);
    form.attach(&status, 0, 4, 2, 1);
    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    actions.set_halign(gtk4::Align::End);
    actions.append(&cancel);
    actions.append(&save);
    form.attach(&actions, 0, 5, 2, 1);

    let window = gtk4::Window::builder()
        .title(if existing.is_some() {
            "Edit camera"
        } else {
            "Add camera"
        })
        .transient_for(parent)
        .modal(true)
        .default_width(640)
        .child(&form)
        .build();
    let active_test = Rc::new(RefCell::new(None::<ConnectionTest>));

    test.connect_clicked({
        let url = url.clone();
        let status = status.clone();
        let test = test.clone();
        let active_test = active_test.clone();
        move |_| {
            active_test.borrow_mut().take();
            status.set_text("Testing connection…");
            test.set_sensitive(false);
            let callback_slot = active_test.clone();
            let callback_status = status.clone();
            let callback_button = test.clone();
            match ConnectionTest::start(url.text().as_str(), move |result| {
                callback_slot.borrow_mut().take();
                callback_button.set_sensitive(true);
                match result {
                    Ok(()) => callback_status.set_text("Connection successful."),
                    Err(error) => callback_status.set_text(&format!("Connection failed: {error}")),
                }
            }) {
                Ok(handle) => *active_test.borrow_mut() = Some(handle),
                Err(error) => {
                    test.set_sensitive(true);
                    status.set_text(&format!("Cannot test connection: {error:#}"));
                }
            }
        }
    });

    save.connect_clicked({
        let state = state.clone();
        let name = name.clone();
        let url = url.clone();
        let substream = substream.clone();
        let status = status.clone();
        let window = window.clone();
        let parent = parent.clone();
        let manager_list = manager_list.clone();
        let manager_status = manager_status.clone();
        let camera_id = existing
            .as_ref()
            .map_or_else(Uuid::new_v4, |camera| camera.id);
        move |button| {
            let camera = CameraConfig {
                id: camera_id,
                name: name.text().trim().to_owned(),
                rtsp_url: url.text().trim().to_owned(),
                substream_url: (!substream.text().trim().is_empty())
                    .then(|| substream.text().trim().to_owned()),
            };
            let mut candidate = state.borrow().config.clone();
            if let Some(index) = candidate
                .cameras
                .iter()
                .position(|item| item.id == camera_id)
            {
                candidate.cameras[index] = camera;
            } else {
                candidate.cameras.push(camera);
            }
            button.set_sensitive(false);
            let button = button.clone();
            let state = state.clone();
            let parent = parent.clone();
            let manager_list = manager_list.clone();
            let manager_status = manager_status.clone();
            let status = status.clone();
            let window = window.clone();
            let commit_state = state.clone();
            commit_config(&commit_state, candidate, move |result| match result {
                Ok(()) => {
                    apply_current_view(&state);
                    refresh_camera_list(&parent, &state, &manager_list, &manager_status);
                    window.close();
                }
                Err(error) => {
                    button.set_sensitive(true);
                    status.set_text(&format!("Could not save camera: {error:#}"));
                }
            });
        }
    });
    cancel.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    window.connect_close_request({
        let active_test = active_test.clone();
        move |_| {
            active_test.borrow_mut().take();
            gtk4::glib::Propagation::Proceed
        }
    });
    window.present();
}

fn show_view_manager(parent: &gtk4::Window, state: &Rc<RefCell<AppState>>) {
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    let status = manager_status_label();
    let kiosk = gtk4::CheckButton::with_label("Enter kiosk mode on application start");
    kiosk.set_active(state.borrow().config.kiosk_on_start);
    let add = gtk4::Button::with_label("Add view…");
    add.add_css_class("suggested-action");
    let close = gtk4::Button::with_label("Close");
    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    actions.append(&add);
    actions.append(&close);
    let content = manager_content(&list, &status, &actions);
    content.prepend(&kiosk);
    let window = gtk4::Window::builder()
        .title("View manager")
        .transient_for(parent)
        .modal(true)
        .default_width(720)
        .default_height(520)
        .child(&content)
        .build();

    let kiosk_update_guard = Rc::new(Cell::new(false));
    kiosk.connect_toggled({
        let state = state.clone();
        let status = status.clone();
        let guard = kiosk_update_guard.clone();
        move |button| {
            if guard.get() {
                return;
            }
            let previous = state.borrow().config.kiosk_on_start;
            let mut candidate = state.borrow().config.clone();
            candidate.kiosk_on_start = button.is_active();
            button.set_sensitive(false);
            let button = button.clone();
            let status = status.clone();
            let guard = guard.clone();
            commit_config(&state, candidate, move |result| {
                button.set_sensitive(true);
                if let Err(error) = result {
                    guard.set(true);
                    button.set_active(previous);
                    guard.set(false);
                    status.set_text(&format!("Could not save kiosk preference: {error:#}"));
                } else {
                    status.set_text("");
                }
            });
        }
    });
    add.connect_clicked({
        let state = state.clone();
        let list = list.clone();
        let window = window.clone();
        let status = status.clone();
        move |_| show_view_editor(&window, &state, &list, &status, None)
    });
    close.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    refresh_view_list(&window, state, &list, &status);
    window.present();
}

fn refresh_view_list(
    parent: &gtk4::Window,
    state: &Rc<RefCell<AppState>>,
    list: &gtk4::ListBox,
    status: &gtk4::Label,
) {
    clear_list(list);
    let (views, startup_view) = {
        let state = state.borrow();
        (state.config.views.clone(), state.config.startup_view)
    };
    status.set_text("");
    for view in views {
        let title = if Some(view.id) == startup_view {
            format!("{} (startup)", view.name)
        } else {
            view.name.clone()
        };
        let name = gtk4::Label::new(Some(&title));
        name.set_hexpand(true);
        name.set_halign(gtk4::Align::Start);
        let edit = gtk4::Button::with_label("Edit…");
        let duplicate = gtk4::Button::with_label("Duplicate");
        let delete = gtk4::Button::with_label("Delete");
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row.set_margin_top(6);
        row.set_margin_bottom(6);
        row.set_margin_start(8);
        row.set_margin_end(8);
        row.append(&name);
        row.append(&edit);
        row.append(&duplicate);
        row.append(&delete);
        list.append(&row);

        edit.connect_clicked({
            let state = state.clone();
            let list = list.clone();
            let parent = parent.clone();
            let view = view.clone();
            let status = status.clone();
            move |_| show_view_editor(&parent, &state, &list, &status, Some(view.clone()))
        });
        duplicate.connect_clicked({
            let state = state.clone();
            let list = list.clone();
            let parent = parent.clone();
            let status = status.clone();
            let view = view.clone();
            move |button| {
                let mut candidate = state.borrow().config.clone();
                let mut copy = view.clone();
                copy.id = Uuid::new_v4();
                copy.name = available_copy_name(&candidate, &view.name);
                candidate.views.push(copy);
                button.set_sensitive(false);
                let button = button.clone();
                let state = state.clone();
                let list = list.clone();
                let parent = parent.clone();
                let status = status.clone();
                let commit_state = state.clone();
                commit_config(&commit_state, candidate, move |result| match result {
                    Ok(()) => {
                        refresh_view_dropdown(&state);
                        refresh_view_list(&parent, &state, &list, &status);
                    }
                    Err(error) => {
                        button.set_sensitive(true);
                        status.set_text(&format!("Could not duplicate view: {error:#}"));
                    }
                });
            }
        });
        delete.connect_clicked({
            let state = state.clone();
            let list = list.clone();
            let parent = parent.clone();
            let status = status.clone();
            move |button| {
                let mut candidate = state.borrow().config.clone();
                if candidate.views.len() == 1 {
                    status.set_text("At least one view is required.");
                    return;
                }
                candidate.views.retain(|item| item.id != view.id);
                if candidate.startup_view == Some(view.id) {
                    candidate.startup_view = candidate.views.first().map(|item| item.id);
                }
                let next_view = if state.borrow().current_view == view.id {
                    candidate.views.first().map(|item| item.id)
                } else {
                    Some(state.borrow().current_view)
                };
                button.set_sensitive(false);
                let button = button.clone();
                let state = state.clone();
                let list = list.clone();
                let parent = parent.clone();
                let status = status.clone();
                let commit_state = state.clone();
                commit_config(&commit_state, candidate, move |result| match result {
                    Ok(()) => {
                        if let Some(next_view) = next_view {
                            state.borrow_mut().current_view = next_view;
                        }
                        refresh_view_dropdown(&state);
                        apply_current_view(&state);
                        refresh_view_list(&parent, &state, &list, &status);
                    }
                    Err(error) => {
                        button.set_sensitive(true);
                        status.set_text(&format!("Could not delete view: {error:#}"));
                    }
                });
            }
        });
    }
}

fn show_view_editor(
    parent: &gtk4::Window,
    state: &Rc<RefCell<AppState>>,
    manager_list: &gtk4::ListBox,
    manager_status: &gtk4::Label,
    existing: Option<ViewConfig>,
) {
    let name = gtk4::Entry::builder()
        .placeholder_text("Overview")
        .text(existing.as_ref().map_or("", |view| &view.name))
        .build();
    let startup = gtk4::CheckButton::with_label("Use this view at startup");
    startup.set_active(
        existing
            .as_ref()
            .is_some_and(|view| state.borrow().config.startup_view == Some(view.id)),
    );
    let camera_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    let selected_ids = existing
        .as_ref()
        .map(|view| {
            view.tiles
                .iter()
                .map(|tile| tile.camera_id)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut camera_checks = Vec::new();
    for camera in state.borrow().config.cameras.clone() {
        let check = gtk4::CheckButton::with_label(&camera.name);
        check.set_active(selected_ids.contains(&camera.id));
        camera_box.append(&check);
        camera_checks.push((camera.id, check));
    }
    let status = manager_status_label();
    let save = gtk4::Button::with_label("Save");
    save.add_css_class("suggested-action");
    let cancel = gtk4::Button::with_label("Cancel");
    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    actions.set_halign(gtk4::Align::End);
    actions.append(&cancel);
    actions.append(&save);
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&gtk4::Label::new(Some("View name")));
    content.append(&name);
    content.append(&gtk4::Label::new(Some("Cameras in this view")));
    content.append(&camera_box);
    content.append(&startup);
    content.append(&status);
    content.append(&actions);

    let window = gtk4::Window::builder()
        .title(if existing.is_some() {
            "Edit view"
        } else {
            "Add view"
        })
        .transient_for(parent)
        .modal(true)
        .default_width(520)
        .child(&content)
        .build();
    let existing_template = existing.clone();
    save.connect_clicked({
        let state = state.clone();
        let name = name.clone();
        let startup = startup.clone();
        let status = status.clone();
        let window = window.clone();
        let parent = parent.clone();
        let manager_list = manager_list.clone();
        let manager_status = manager_status.clone();
        let view_id = existing.as_ref().map_or_else(Uuid::new_v4, |view| view.id);
        move |button| {
            let camera_ids = camera_checks
                .iter()
                .filter(|(_, check)| check.is_active())
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            if camera_ids.len() > MAX_CAMERAS {
                status.set_text(&format!(
                    "A view can contain at most {MAX_CAMERAS} cameras."
                ));
                return;
            }
            let view = reconcile_view(
                existing_template.as_ref(),
                view_id,
                name.text().trim().to_owned(),
                &camera_ids,
            );
            let mut candidate = state.borrow().config.clone();
            if let Some(index) = candidate.views.iter().position(|item| item.id == view_id) {
                candidate.views[index] = view;
            } else {
                candidate.views.push(view);
            }
            if startup.is_active() {
                candidate.startup_view = Some(view_id);
            } else if candidate.startup_view == Some(view_id) {
                candidate.startup_view = None;
            }
            button.set_sensitive(false);
            let button = button.clone();
            let state = state.clone();
            let parent = parent.clone();
            let manager_list = manager_list.clone();
            let manager_status = manager_status.clone();
            let status = status.clone();
            let window = window.clone();
            let commit_state = state.clone();
            commit_config(&commit_state, candidate, move |result| match result {
                Ok(()) => {
                    state.borrow_mut().current_view = view_id;
                    refresh_view_dropdown(&state);
                    apply_current_view(&state);
                    refresh_view_list(&parent, &state, &manager_list, &manager_status);
                    window.close();
                }
                Err(error) => {
                    button.set_sensitive(true);
                    status.set_text(&format!("Could not save view: {error:#}"));
                }
            });
        }
    });
    cancel.connect_clicked({
        let window = window.clone();
        move |_| window.close()
    });
    window.present();
}

fn automatic_layout(camera_ids: &[Uuid]) -> (u32, u32, Vec<ViewTile>) {
    let columns = match camera_ids.len() {
        0 | 1 => 1,
        2..=4 => 2,
        5..=9 => 3,
        _ => 4,
    } as u32;
    let rows = (camera_ids.len() as u32).div_ceil(columns).max(1);
    let tiles = camera_ids
        .iter()
        .enumerate()
        .map(|(index, camera_id)| ViewTile {
            camera_id: *camera_id,
            column: index as u32 % columns,
            row: index as u32 / columns,
            column_span: 1,
            row_span: 1,
        })
        .collect();
    (columns, rows, tiles)
}

fn reconcile_view(
    existing: Option<&ViewConfig>,
    id: Uuid,
    name: String,
    camera_ids: &[Uuid],
) -> ViewConfig {
    let Some(existing) = existing else {
        let (columns, rows, tiles) = automatic_layout(camera_ids);
        return ViewConfig {
            id,
            name,
            columns,
            rows,
            tiles,
        };
    };

    let selected = camera_ids
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let mut tiles = existing
        .tiles
        .iter()
        .filter(|tile| selected.contains(&tile.camera_id))
        .cloned()
        .collect::<Vec<_>>();
    let mut occupied = occupied_cells(&tiles);
    let mut rows = existing.rows;
    for camera_id in camera_ids {
        if tiles.iter().any(|tile| tile.camera_id == *camera_id) {
            continue;
        }
        let (column, row) = if let Some(cell) = next_free_cell(existing.columns, rows, &occupied) {
            cell
        } else if rows < MAX_GRID_EXTENT {
            let row = rows;
            rows += 1;
            (0, row)
        } else {
            let (columns, rows, tiles) = automatic_layout(camera_ids);
            return ViewConfig {
                id,
                name,
                columns,
                rows,
                tiles,
            };
        };
        occupied.insert((column, row));
        tiles.push(ViewTile {
            camera_id: *camera_id,
            column,
            row,
            column_span: 1,
            row_span: 1,
        });
    }

    ViewConfig {
        id,
        name,
        columns: existing.columns,
        rows,
        tiles,
    }
}

fn available_copy_name(config: &AppConfig, original: &str) -> String {
    let base = format!("{original} copy");
    if !config
        .views
        .iter()
        .any(|view| view.name.eq_ignore_ascii_case(&base))
    {
        return base;
    }
    (2..)
        .map(|number| format!("{original} copy {number}"))
        .find(|name| {
            !config
                .views
                .iter()
                .any(|view| view.name.eq_ignore_ascii_case(name))
        })
        .expect("an unused generated view name must exist")
}

fn manager_content(list: &gtk4::ListBox, status: &gtk4::Label, actions: &gtk4::Box) -> gtk4::Box {
    let scroller = gtk4::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(list)
        .build();
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&scroller);
    content.append(status);
    content.append(actions);
    content
}

fn manager_status_label() -> gtk4::Label {
    let label = gtk4::Label::new(None);
    label.set_halign(gtk4::Align::Start);
    label.set_wrap(true);
    label.add_css_class("error");
    label
}

fn clear_list(list: &gtk4::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn show_error(state: &Rc<RefCell<AppState>>, message: &str) {
    let state = state.borrow();
    state.error_label.set_text(message);
    state
        .error_label
        .set_visible(!state.kiosk_mode && state.editing.is_none());
}

fn hide_error(state: &Rc<RefCell<AppState>>) {
    state.borrow().error_label.set_visible(false);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_layout_assigns_unique_grid_cells() {
        let ids = (0..10).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
        let (columns, rows, tiles) = automatic_layout(&ids);
        assert_eq!((columns, rows), (4, 3));
        assert_eq!(tiles.len(), 10);
        assert_eq!(tiles[0].column, 0);
        assert_eq!(tiles[3].column, 3);
        assert_eq!(tiles[4].row, 1);
    }

    #[test]
    fn startup_view_resolves_ids_names_and_fallbacks() {
        let mut config = AppConfig::default();
        let first_id = config.views[0].id;
        let second_id = Uuid::new_v4();
        config.views.push(ViewConfig {
            id: second_id,
            name: "Side door".to_owned(),
            columns: 1,
            rows: 1,
            tiles: Vec::new(),
        });

        assert_eq!(
            resolve_startup_view(&config, Some(&second_id.to_string())),
            Some(second_id)
        );
        assert_eq!(
            resolve_startup_view(&config, Some("SIDE DOOR")),
            Some(second_id)
        );
        assert_eq!(
            resolve_startup_view(&config, Some("missing")),
            Some(first_id)
        );
    }

    #[test]
    fn editing_view_preserves_existing_geometry() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let existing = ViewConfig {
            id: Uuid::new_v4(),
            name: "Custom".to_owned(),
            columns: 3,
            rows: 2,
            tiles: vec![ViewTile {
                camera_id: first,
                column: 0,
                row: 0,
                column_span: 2,
                row_span: 2,
            }],
        };

        let edited = reconcile_view(
            Some(&existing),
            existing.id,
            existing.name.clone(),
            &[first, second],
        );

        assert_eq!(edited.tiles[0], existing.tiles[0]);
        assert_eq!((edited.tiles[1].column, edited.tiles[1].row), (2, 0));
        assert_eq!((edited.columns, edited.rows), (3, 2));
    }

    #[test]
    fn copied_view_names_are_unique() {
        let mut config = AppConfig::default();
        let mut copy = config.views[0].clone();
        copy.id = Uuid::new_v4();
        copy.name = "Overview copy".to_owned();
        config.views.push(copy);

        assert_eq!(available_copy_name(&config, "Overview"), "Overview copy 2");
    }
}
