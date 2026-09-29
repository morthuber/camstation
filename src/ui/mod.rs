use std::cell::{Cell, RefCell};
use std::rc::Rc;

use anyhow::Result;
use gtk4::prelude::*;
use uuid::Uuid;

use crate::application::Options;
use crate::config::{AppConfig, CameraConfig, ConfigStore, MAX_GRID_EXTENT, ViewConfig, ViewTile};
use crate::media::{CameraController, ConnectionTest, DecoderInfo, PlaybackEvent, PlaybackState};

const MAX_CAMERAS: usize = 10;

struct AppState {
    grid: gtk4::Grid,
    error_label: gtk4::Label,
    view_dropdown: gtk4::DropDown,
    view_ids: Vec<Uuid>,
    view_change_guard: Rc<Cell<bool>>,
    tiles: Vec<Rc<CameraTile>>,
    next_runtime_id: u64,
    audio_update_guard: Rc<Cell<bool>>,
    editable: bool,
    config: AppConfig,
    store: ConfigStore,
    current_view: Uuid,
    session_urls: Vec<String>,
    config_save_in_progress: bool,
}

struct CameraTile {
    id: u64,
    root: gtk4::Frame,
    controller: CameraController,
    audio_button: gtk4::ToggleButton,
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
    let kiosk = options.kiosk || config.kiosk_on_start;

    let view_model = gtk4::StringList::new(&[]);
    let view_dropdown = gtk4::DropDown::builder().model(&view_model).build();
    view_dropdown.set_hexpand(false);
    view_dropdown.set_tooltip_text(Some("Select a saved camera view"));

    let cameras_button = gtk4::Button::with_label("Cameras…");
    let views_button = gtk4::Button::with_label("Views…");

    let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    controls.set_margin_top(12);
    controls.set_margin_bottom(6);
    controls.set_margin_start(12);
    controls.set_margin_end(12);
    controls.append(&gtk4::Label::new(Some("View")));
    controls.append(&view_dropdown);
    controls.append(&cameras_button);
    controls.append(&views_button);

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
    root.append(&error_label);
    root.append(&scroller);

    let editable = !kiosk;
    if !editable {
        controls.set_visible(false);
        error_label.set_visible(false);
    }

    let state = Rc::new(RefCell::new(AppState {
        grid: grid.clone(),
        error_label: error_label.clone(),
        view_dropdown: view_dropdown.clone(),
        view_ids: Vec::new(),
        view_change_guard: Rc::new(Cell::new(false)),
        tiles: Vec::new(),
        next_runtime_id: 1,
        audio_update_guard: Rc::new(Cell::new(false)),
        editable,
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

    let window = gtk4::ApplicationWindow::builder()
        .application(application)
        .title("Camview")
        .default_width(1_280)
        .default_height(720)
        .child(&root)
        .build();

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

    window.connect_close_request({
        let state = state.clone();
        move |_| {
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
        add_runtime_camera(state, &camera.name, &uri, &tile);
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

    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    actions.set_margin_top(6);
    actions.set_margin_bottom(6);
    actions.set_margin_start(8);
    actions.set_margin_end(8);
    actions.append(&name_label);
    actions.append(&audio_button);

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
            if !guard.get() {
                set_audible_camera(&state, tile.id, button.is_active());
            }
        }
    });

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
    state.error_label.set_visible(state.editable);
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
