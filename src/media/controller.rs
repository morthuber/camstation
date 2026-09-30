use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use gst::prelude::*;
use gstreamer as gst;

use super::lifecycle::{HEALTHY_RESET_INTERVAL, ReconnectBackoff, stream_is_stalled};
use super::normalize_rtsp_uri;

const MINIMUM_RTSP_LATENCY_MS: u32 = 0;
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlaybackState {
    Stopped,
    Starting,
    Playing,
    Stalled,
    Reconnecting(u64),
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecoderInfo {
    pub(crate) factories: Vec<String>,
    pub(crate) hardware_accelerated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PlaybackEvent {
    StateChanged(PlaybackState),
    PaintableChanged(gtk4::gdk::Paintable),
    AudioDisabled(String),
    Error(String),
}

type EventHandler = Box<dyn FnMut(PlaybackEvent)>;

type SharedEventDispatcher = Rc<RefCell<EventDispatcher>>;

struct EventDispatcher {
    handler: Option<EventHandler>,
    queued: VecDeque<PlaybackEvent>,
    dispatching: bool,
}

#[derive(Clone)]
struct ControllerContext {
    inner: Weak<RefCell<ControllerInner>>,
    events: Weak<RefCell<EventDispatcher>>,
}

struct ControllerInner {
    camera_key: u64,
    uri: String,
    desired_playing: bool,
    muted: bool,
    generation: u64,
    state: PlaybackState,
    backoff: ReconnectBackoff,
    active: Option<ActivePipeline>,
    recovery_idle: Option<gst::glib::SourceId>,
    retry_source: Option<gst::glib::SourceId>,
    watchdog_source: Option<gst::glib::SourceId>,
    last_frame_count: u64,
    last_progress_at: Instant,
    healthy_since: Option<Instant>,
}

struct ActivePipeline {
    generation: u64,
    playbin: gst::Element,
    bus_watch: Option<gst::bus::BusWatchGuard>,
    probe_pad: gst::Pad,
    probe_id: Option<gst::PadProbeId>,
    frame_counter: Arc<AtomicU64>,
    started_at: Instant,
}

impl ActivePipeline {
    fn shutdown(mut self) {
        self.bus_watch.take();
        if let Some(probe_id) = self.probe_id.take() {
            self.probe_pad.remove_probe(probe_id);
        }
        if let Err(error) = self.playbin.set_state(gst::State::Null) {
            tracing::warn!(?error, "failed to stop camera pipeline cleanly");
        }
    }
}

pub(crate) struct CameraController {
    inner: Rc<RefCell<ControllerInner>>,
    events: SharedEventDispatcher,
}

impl CameraController {
    pub(crate) fn new<F>(camera_key: u64, uri: &str, strip_fragment: bool, emit: F) -> Result<Self>
    where
        F: FnMut(PlaybackEvent) + 'static,
    {
        validate_rtsp_uri(uri)?;
        require_element_factory("gtk4paintablesink")?;
        require_element_factory("playbin3")?;

        let normalized_uri = if strip_fragment {
            normalize_rtsp_uri(uri).to_owned()
        } else {
            uri.to_owned()
        };

        Ok(Self {
            inner: Rc::new(RefCell::new(ControllerInner {
                camera_key,
                uri: normalized_uri,
                desired_playing: false,
                muted: true,
                generation: 0,
                state: PlaybackState::Stopped,
                backoff: ReconnectBackoff::default(),
                active: None,
                recovery_idle: None,
                retry_source: None,
                watchdog_source: None,
                last_frame_count: 0,
                last_progress_at: Instant::now(),
                healthy_since: None,
            })),
            events: Rc::new(RefCell::new(EventDispatcher {
                handler: Some(Box::new(emit)),
                queued: VecDeque::new(),
                dispatching: false,
            })),
        })
    }

    pub(crate) fn start(&self) {
        {
            let mut inner = self.inner.borrow_mut();
            if inner.desired_playing {
                return;
            }
            inner.desired_playing = true;
        }

        let context = self.context();
        install_watchdog(&context);
        start_generation(&context);
    }

    pub(crate) fn stop(&self) {
        stop_controller(&self.context());
    }

    pub(crate) fn set_muted(&self, muted: bool) {
        switch_audio_mode(&self.context(), muted);
    }

    fn context(&self) -> ControllerContext {
        ControllerContext {
            inner: Rc::downgrade(&self.inner),
            events: Rc::downgrade(&self.events),
        }
    }
}

impl Drop for CameraController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn require_element_factory(name: &str) -> Result<()> {
    if gst::ElementFactory::find(name).is_none() {
        bail!("GStreamer plugin '{name}' is unavailable");
    }
    Ok(())
}

fn install_watchdog(context: &ControllerContext) {
    let Some(inner) = context.inner.upgrade() else {
        return;
    };
    let mut inner = inner.borrow_mut();
    if inner.watchdog_source.is_some() {
        return;
    }

    let watchdog_context = context.clone();
    inner.watchdog_source = Some(gst::glib::timeout_add_local(WATCHDOG_INTERVAL, move || {
        let Some(inner) = watchdog_context.inner.upgrade() else {
            return gst::glib::ControlFlow::Break;
        };
        if !inner.borrow().desired_playing {
            return gst::glib::ControlFlow::Break;
        }

        check_watchdog(&watchdog_context);
        gst::glib::ControlFlow::Continue
    }));
}

fn check_watchdog(context: &ControllerContext) {
    let now = Instant::now();
    let mut stalled_generation = None;

    {
        let Some(inner) = context.inner.upgrade() else {
            return;
        };
        let mut inner = inner.borrow_mut();
        let Some(active) = inner.active.as_ref() else {
            return;
        };

        let generation = active.generation;
        let started_at = active.started_at;
        let frame_count = active.frame_counter.load(Ordering::Relaxed);

        if frame_count != inner.last_frame_count {
            inner.last_frame_count = frame_count;
            inner.last_progress_at = now;
            inner.healthy_since.get_or_insert(now);

            if inner.backoff.has_retried()
                && inner
                    .healthy_since
                    .is_some_and(|since| now.duration_since(since) >= HEALTHY_RESET_INTERVAL)
            {
                tracing::debug!(camera = inner.camera_key, "reset reconnect backoff");
                inner.backoff.reset();
            }
            return;
        }

        if stream_is_stalled(
            frame_count,
            now.duration_since(started_at),
            now.duration_since(inner.last_progress_at),
        ) {
            stalled_generation = Some(generation);
        }
    }

    if let Some(generation) = stalled_generation {
        tracing::warn!(generation, "camera stream stopped delivering video frames");
        notify_state(context, PlaybackState::Stalled);
        queue_recovery(
            context,
            generation,
            "stream stopped delivering video frames",
        );
    }
}

fn start_generation(context: &ControllerContext) {
    let (camera_key, uri, muted, generation) = {
        let Some(inner) = context.inner.upgrade() else {
            return;
        };
        let mut inner = inner.borrow_mut();
        if !inner.desired_playing || inner.active.is_some() {
            return;
        }

        inner.generation = inner.generation.wrapping_add(1);
        inner.last_frame_count = 0;
        inner.last_progress_at = Instant::now();
        inner.healthy_since = None;
        (
            inner.camera_key,
            inner.uri.clone(),
            inner.muted,
            inner.generation,
        )
    };

    notify_state(context, PlaybackState::Starting);

    let (active, paintable) = match build_pipeline(context, camera_key, &uri, muted, generation) {
        Ok(result) => result,
        Err(error) => {
            let error = redact_sensitive_text(&format!("{error:#}"));
            tracing::warn!(camera = camera_key, generation, error = %error, "could not build camera pipeline");
            mark_failed(context);
            emit_event(context, PlaybackEvent::Error(error));
            return;
        }
    };

    let playbin = active.playbin.clone();
    {
        let Some(inner) = context.inner.upgrade() else {
            active.shutdown();
            return;
        };
        let mut inner = inner.borrow_mut();
        if !inner.desired_playing || inner.generation != generation {
            drop(inner);
            active.shutdown();
            return;
        }
        inner.active = Some(active);
    }

    emit_event(context, PlaybackEvent::PaintableChanged(paintable));

    if let Err(error) = playbin.set_state(gst::State::Playing) {
        let message = format!("failed to start camera pipeline: {error:?}");
        emit_event(context, PlaybackEvent::Error(message.clone()));
        queue_recovery(context, generation, &message);
    }
}

fn build_pipeline(
    context: &ControllerContext,
    camera_key: u64,
    uri: &str,
    muted: bool,
    generation: u64,
) -> Result<(ActivePipeline, gtk4::gdk::Paintable)> {
    let video_sink = gst::ElementFactory::make("gtk4paintablesink")
        .name(format!("camstation_video_{camera_key}_{generation}"))
        .property("sync", false)
        .build()
        .context("GStreamer plugin 'gtk4paintablesink' is unavailable")?;
    let paintable = video_sink.property::<gtk4::gdk::Paintable>("paintable");

    let audio_sink = if muted {
        build_silent_audio_sink(camera_key, generation)?
    } else {
        match gst::ElementFactory::make("autoaudiosink")
            .name(format!("camstation_audio_{camera_key}_{generation}"))
            .build()
        {
            Ok(sink) => sink,
            Err(error) => {
                tracing::warn!(
                    camera = camera_key,
                    ?error,
                    "audio output unavailable; using silent sink"
                );
                build_silent_audio_sink(camera_key, generation)?
            }
        }
    };

    let playbin = gst::ElementFactory::make("playbin3")
        .name(format!("camstation_player_{camera_key}_{generation}"))
        .property("uri", uri)
        .property("video-sink", &video_sink)
        .property("audio-sink", &audio_sink)
        .property("mute", muted)
        .build()
        .context("GStreamer plugin 'playbin3' is unavailable")?;
    configure_rtsp_source(&playbin);

    let probe_pad = video_sink
        .static_pad("sink")
        .context("video sink did not expose its sink pad")?;
    let frame_counter = Arc::new(AtomicU64::new(0));
    let probe_counter = frame_counter.clone();
    let probe_id = probe_pad
        .add_probe(
            gst::PadProbeType::BUFFER | gst::PadProbeType::BUFFER_LIST,
            move |_, _| {
                probe_counter.fetch_add(1, Ordering::Relaxed);
                gst::PadProbeReturn::Ok
            },
        )
        .context("failed to install the video frame watchdog probe")?;

    let bus = playbin
        .bus()
        .context("playback pipeline did not provide a GStreamer bus")?;
    let watched_playbin = playbin.clone();
    let watched_context = context.clone();
    let mut last_decoder = None;
    let bus_watch = bus
        .add_watch_local(move |_, message| {
            if !is_current_generation(&watched_context, generation) {
                return gst::glib::ControlFlow::Break;
            }

            match message.view() {
                gst::MessageView::Error(error) => {
                    let gst_error = error.error();
                    let debug_str = error.debug().unwrap_or_default();
                    let error = redact_sensitive_text(&gst_error.to_string());
                    if !muted && message_is_from_or_below(message, &audio_sink) {
                        tracing::warn!(
                            camera = camera_key,
                            generation,
                            error = %error,
                            debug = %debug_str,
                            "camera audio failed; falling back to muted playback"
                        );
                        queue_audio_fallback(&watched_context, generation, &error);
                    } else {
                        tracing::warn!(
                            camera = camera_key,
                            generation,
                            error = %error,
                            debug = %debug_str,
                            "camera pipeline failed"
                        );
                        emit_event(&watched_context, PlaybackEvent::Error(error.clone()));
                        queue_recovery(&watched_context, generation, &error);
                    }
                }
                gst::MessageView::Eos(_) => {
                    let error = "camera stream reached end of stream".to_owned();
                    tracing::warn!(
                        camera = camera_key,
                        generation,
                        "camera pipeline reached end of stream"
                    );
                    emit_event(&watched_context, PlaybackEvent::Error(error.clone()));
                    queue_recovery(&watched_context, generation, &error);
                }
                gst::MessageView::StateChanged(state)
                    if message_is_from(message, &watched_playbin)
                        && state.current() == gst::State::Playing =>
                {
                    notify_state(&watched_context, PlaybackState::Playing);
                    emit_decoder_if_changed(&watched_playbin, &mut last_decoder);
                }
                gst::MessageView::AsyncDone(_) | gst::MessageView::StreamStart(_) => {
                    emit_decoder_if_changed(&watched_playbin, &mut last_decoder);
                }
                _ => {}
            }

            gst::glib::ControlFlow::Continue
        })
        .context("failed to attach the camera pipeline bus watch")?;

    Ok((
        ActivePipeline {
            generation,
            playbin,
            bus_watch: Some(bus_watch),
            probe_pad,
            probe_id: Some(probe_id),
            frame_counter,
            started_at: Instant::now(),
        },
        paintable,
    ))
}

fn build_silent_audio_sink(camera_key: u64, generation: u64) -> Result<gst::Element> {
    gst::ElementFactory::make("fakesink")
        .name(format!("camstation_silent_audio_{camera_key}_{generation}"))
        .build()
        .context("GStreamer audio fallback 'fakesink' is unavailable")
}

fn switch_audio_mode(context: &ControllerContext, muted: bool) {
    let (active, recovery_idle, should_restart) = {
        let Some(inner) = context.inner.upgrade() else {
            return;
        };
        let mut inner = inner.borrow_mut();
        if inner.muted == muted {
            return;
        }

        inner.muted = muted;
        let should_restart = inner.desired_playing && inner.active.is_some();
        if should_restart {
            inner.generation = inner.generation.wrapping_add(1);
        }
        (
            if should_restart {
                inner.active.take()
            } else {
                None
            },
            if should_restart {
                inner.recovery_idle.take()
            } else {
                None
            },
            should_restart,
        )
    };

    if let Some(source) = recovery_idle {
        source.remove();
    }
    if let Some(active) = active {
        active.shutdown();
    }
    if should_restart {
        start_generation(context);
    }
}

fn queue_audio_fallback(context: &ControllerContext, generation: u64, error: &str) {
    let Some(inner) = context.inner.upgrade() else {
        return;
    };
    let mut inner = inner.borrow_mut();
    if !inner.desired_playing
        || inner.generation != generation
        || inner.recovery_idle.is_some()
        || inner.retry_source.is_some()
    {
        return;
    }

    let fallback_context = context.clone();
    let error = error.to_owned();
    inner.recovery_idle = Some(gst::glib::idle_add_local_once(move || {
        fallback_to_silent_audio(&fallback_context, generation, error);
    }));
}

fn fallback_to_silent_audio(context: &ControllerContext, generation: u64, error: String) {
    let active = {
        let Some(inner) = context.inner.upgrade() else {
            return;
        };
        let mut inner = inner.borrow_mut();
        inner.recovery_idle.take();
        if !inner.desired_playing || inner.generation != generation {
            return;
        }

        inner.muted = true;
        inner.generation = inner.generation.wrapping_add(1);
        inner.active.take()
    };

    if let Some(active) = active {
        active.shutdown();
    }
    start_generation(context);
    emit_event(
        context,
        PlaybackEvent::AudioDisabled(format!("Audio unavailable: {error}")),
    );
}

fn queue_recovery(context: &ControllerContext, failed_generation: u64, reason: &str) {
    let Some(inner) = context.inner.upgrade() else {
        return;
    };
    let mut inner = inner.borrow_mut();
    if !inner.desired_playing
        || inner.generation != failed_generation
        || inner.recovery_idle.is_some()
        || inner.retry_source.is_some()
    {
        return;
    }

    tracing::debug!(
        camera = inner.camera_key,
        failed_generation,
        reason,
        "queued camera recovery"
    );
    let recovery_context = context.clone();
    inner.recovery_idle = Some(gst::glib::idle_add_local_once(move || {
        begin_recovery(&recovery_context, failed_generation);
    }));
}

fn begin_recovery(context: &ControllerContext, failed_generation: u64) {
    let (active, delay, retry_generation, camera_key) = {
        let Some(inner) = context.inner.upgrade() else {
            return;
        };
        let mut inner = inner.borrow_mut();
        inner.recovery_idle.take();
        if !inner.desired_playing || inner.generation != failed_generation {
            return;
        }

        inner.generation = inner.generation.wrapping_add(1);
        let retry_generation = inner.generation;
        let delay = inner.backoff.next_delay();
        inner.healthy_since = None;
        (
            inner.active.take(),
            delay,
            retry_generation,
            inner.camera_key,
        )
    };

    if let Some(active) = active {
        active.shutdown();
    }

    tracing::info!(
        camera = camera_key,
        retry_seconds = delay.as_secs(),
        "camera reconnect scheduled"
    );
    notify_state(context, PlaybackState::Reconnecting(delay.as_secs()));

    let retry_context = context.clone();
    let retry_source = gst::glib::timeout_add_local_once(delay, move || {
        let Some(inner) = retry_context.inner.upgrade() else {
            return;
        };
        {
            let mut inner = inner.borrow_mut();
            inner.retry_source.take();
            if !inner.desired_playing || inner.generation != retry_generation {
                return;
            }
        }
        start_generation(&retry_context);
    });

    let Some(inner) = context.inner.upgrade() else {
        retry_source.remove();
        return;
    };
    let mut inner = inner.borrow_mut();
    if inner.desired_playing && inner.generation == retry_generation {
        inner.retry_source = Some(retry_source);
    } else {
        drop(inner);
        retry_source.remove();
    }
}

fn mark_failed(context: &ControllerContext) {
    let watchdog_source = {
        let Some(inner) = context.inner.upgrade() else {
            return;
        };
        let mut inner = inner.borrow_mut();
        inner.desired_playing = false;
        inner.watchdog_source.take()
    };

    if let Some(source) = watchdog_source {
        source.remove();
    }
    notify_state(context, PlaybackState::Failed);
}

fn stop_controller(context: &ControllerContext) {
    let (active, recovery_idle, retry_source, watchdog_source, should_notify) = {
        let Some(inner) = context.inner.upgrade() else {
            return;
        };
        let mut inner = inner.borrow_mut();
        let should_notify = inner.state != PlaybackState::Stopped;
        inner.desired_playing = false;
        inner.generation = inner.generation.wrapping_add(1);
        inner.state = PlaybackState::Stopped;
        (
            inner.active.take(),
            inner.recovery_idle.take(),
            inner.retry_source.take(),
            inner.watchdog_source.take(),
            should_notify,
        )
    };

    if let Some(source) = recovery_idle {
        source.remove();
    }
    if let Some(source) = retry_source {
        source.remove();
    }
    if let Some(source) = watchdog_source {
        source.remove();
    }
    if let Some(active) = active {
        active.shutdown();
    }
    if should_notify {
        emit_event(context, PlaybackEvent::StateChanged(PlaybackState::Stopped));
    }
}

fn notify_state(context: &ControllerContext, state: PlaybackState) {
    let changed = {
        let Some(inner) = context.inner.upgrade() else {
            return;
        };
        let mut inner = inner.borrow_mut();
        if inner.state == state {
            false
        } else {
            inner.state = state;
            true
        }
    };

    if changed {
        emit_event(context, PlaybackEvent::StateChanged(state));
    }
}

fn emit_event(context: &ControllerContext, event: PlaybackEvent) {
    let Some(events) = context.events.upgrade() else {
        return;
    };

    {
        let mut dispatcher = events.borrow_mut();
        dispatcher.queued.push_back(event);
        if dispatcher.dispatching {
            return;
        }
        dispatcher.dispatching = true;
    }

    loop {
        let Some((mut handler, event)) = (|| {
            let mut dispatcher = events.borrow_mut();
            let Some(event) = dispatcher.queued.pop_front() else {
                dispatcher.dispatching = false;
                return None;
            };
            let handler = dispatcher
                .handler
                .take()
                .expect("event handler missing outside callback dispatch");
            Some((handler, event))
        })() else {
            break;
        };

        handler(event);
        events.borrow_mut().handler = Some(handler);
    }
}

fn is_current_generation(context: &ControllerContext, generation: u64) -> bool {
    context.inner.upgrade().is_some_and(|inner| {
        let inner = inner.borrow();
        inner.desired_playing && inner.generation == generation
    })
}

pub(super) fn configure_rtsp_source(playbin: &gst::Element) {
    playbin.connect_local("source-setup", false, move |values| {
        let Some(source) = values
            .get(1)
            .and_then(|value| value.get::<gst::Element>().ok())
        else {
            tracing::warn!("playbin source-setup did not provide a source element");
            return None;
        };

        let factory_name = source
            .factory()
            .map(|factory| factory.name().to_string())
            .unwrap_or_else(|| source.type_().name().to_string());
        tracing::debug!(source = %factory_name, "configured media source for minimum latency");

        if source.find_property("latency").is_some() {
            source.set_property("latency", MINIMUM_RTSP_LATENCY_MS);
        }
        if source.find_property("drop-on-latency").is_some() {
            source.set_property("drop-on-latency", true);
        }
        if source.find_property("buffer-mode").is_some() {
            source.set_property_from_str("buffer-mode", "none");
        }
        if source.find_property("protocols").is_some() {
            source.set_property_from_str("protocols", "tcp");
        }

        None
    });
}

fn emit_decoder_if_changed(playbin: &gst::Element, last_decoder: &mut Option<DecoderInfo>) {
    let decoder = detect_video_decoders(playbin);
    if decoder.factories.is_empty() || last_decoder.as_ref() == Some(&decoder) {
        return;
    }

    tracing::info!(
        decoders = ?decoder.factories,
        hardware_accelerated = decoder.hardware_accelerated,
        "selected video decoder"
    );
    *last_decoder = Some(decoder.clone());
}

fn detect_video_decoders(playbin: &gst::Element) -> DecoderInfo {
    let mut decoders = playbin
        .clone()
        .downcast::<gst::Bin>()
        .ok()
        .into_iter()
        .flat_map(|bin| {
            bin.iterate_recurse()
                .into_iter()
                .filter_map(|item| item.ok())
        })
        .filter_map(|element| element.factory())
        .filter_map(|factory| {
            let klass = factory.metadata("klass")?;
            (klass.contains("Decoder") && klass.contains("Video")).then(|| {
                let name = factory.name().to_string();
                let hardware_accelerated = is_hardware_decoder(&name, klass);
                (name, hardware_accelerated)
            })
        })
        .collect::<Vec<_>>();

    decoders.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    decoders.dedup_by(|left, right| left.0 == right.0);

    DecoderInfo {
        hardware_accelerated: decoders.iter().any(|(_, hardware)| *hardware),
        factories: decoders.into_iter().map(|(name, _)| name).collect(),
    }
}

fn is_hardware_decoder(factory_name: &str, klass: &str) -> bool {
    if klass.split('/').any(|component| component == "Hardware") {
        return true;
    }

    let name = factory_name.to_ascii_lowercase();
    [
        "amf", "d3d11", "d3d12", "msdk", "nv", "omx", "qsv", "v4l2", "va", "vtdec", "vulkan",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        || name.contains("vaapi")
}

fn message_is_from_or_below(message: &gst::MessageRef, element: &gst::Element) -> bool {
    message.src().is_some_and(|source| {
        source
            .downcast_ref::<gst::Element>()
            .is_some_and(|source| source == element)
            || source.has_as_ancestor(element)
    })
}

fn message_is_from(message: &gst::MessageRef, element: &gst::Element) -> bool {
    message
        .src()
        .and_then(|source| source.downcast_ref::<gst::Element>())
        .is_some_and(|source| source == element)
}

pub(crate) fn validate_rtsp_uri(uri: &str) -> Result<()> {
    if uri.trim() != uri {
        bail!("RTSP URL must not have leading or trailing whitespace");
    }
    let lower = uri.to_ascii_lowercase();

    if uri.is_empty() {
        bail!("enter an RTSP URL");
    }
    if !lower.starts_with("rtsp://") && !lower.starts_with("rtsps://") {
        bail!("URL must use the rtsp:// or rtsps:// scheme");
    }
    if uri.chars().any(char::is_whitespace) {
        bail!("RTSP URL must not contain whitespace");
    }

    let authority_start = uri.find("://").map_or(0, |index| index + 3);
    let authority = uri[authority_start..]
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let valid_host = if host.starts_with('[') {
        host.find(']').is_some_and(|end| end > 1)
    } else {
        !host.split(':').next().unwrap_or_default().is_empty()
    };
    if !valid_host {
        bail!("RTSP URL must include a host");
    }

    Ok(())
}

pub(crate) fn redact_sensitive_text(text: &str) -> String {
    let mut redacted = text.to_owned();

    for scheme in ["rtsp://", "rtsps://"] {
        let mut search_from = 0;
        while let Some(relative_start) = redacted[search_from..].to_ascii_lowercase().find(scheme) {
            let start = search_from + relative_start;
            let authority_start = start + scheme.len();
            let end = redacted[authority_start..]
                .find(|character: char| {
                    character.is_whitespace() || matches!(character, '"' | '\'' | ')' | ']' | '>')
                })
                .map_or(redacted.len(), |offset| authority_start + offset);

            let Some(at_offset) = redacted[authority_start..end].find('@') else {
                search_from = end;
                continue;
            };

            let credentials_end = authority_start + at_offset + 1;
            redacted.replace_range(authority_start..credentials_end, "<credentials>@");
            search_from = authority_start + "<credentials>@".len();
        }
    }

    redacted
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serial_test::serial;

    #[test]
    fn validates_supported_rtsp_urls() {
        assert!(validate_rtsp_uri("rtsp://camera.local/stream").is_ok());
        assert!(validate_rtsp_uri("rtsps://user:password@camera.local/stream").is_ok());
        assert!(validate_rtsp_uri("RTSP://[::1]:8554/live").is_ok());
    }

    #[test]
    fn rejects_invalid_rtsp_urls() {
        assert!(validate_rtsp_uri("").is_err());
        assert!(validate_rtsp_uri("https://camera.local/stream").is_err());
        assert!(validate_rtsp_uri("rtsp:///stream").is_err());
        assert!(validate_rtsp_uri("rtsp://camera.local/bad stream").is_err());
        assert!(validate_rtsp_uri(" rtsp://camera.local/stream").is_err());
        assert!(validate_rtsp_uri("rtsp://:8554/stream").is_err());
        assert!(validate_rtsp_uri("rtsp://?token=secret").is_err());
    }

    #[test]
    fn classifies_hardware_decoders_generically() {
        assert!(is_hardware_decoder(
            "custom_decoder",
            "Codec/Decoder/Video/Hardware"
        ));
        assert!(is_hardware_decoder("vah264dec", "Codec/Decoder/Video"));
        assert!(is_hardware_decoder("nvh265dec", "Codec/Decoder/Video"));
        assert!(is_hardware_decoder("v4l2h264dec", "Codec/Decoder/Video"));
        assert!(!is_hardware_decoder("avdec_h264", "Codec/Decoder/Video"));
    }

    #[test]
    fn redacts_rtsp_credentials() {
        assert_eq!(
            redact_sensitive_text("failed for rtsp://admin:secret@camera.local/live"),
            "failed for rtsp://<credentials>@camera.local/live"
        );
        assert_eq!(
            redact_sensitive_text("rtsps://camera.local/live"),
            "rtsps://camera.local/live"
        );
        assert_eq!(
            redact_sensitive_text("(RTSP://user:secret@camera.local/live)"),
            "(RTSP://<credentials>@camera.local/live)"
        );
        assert_eq!(
            redact_sensitive_text("'rtsps://user:p%40ss@[::1]:8554/live'"),
            "'rtsps://<credentials>@[::1]:8554/live'"
        );
        let multiple = "rtsp://one:a@first/live and rtsps://two:b@second/live";
        let redacted = redact_sensitive_text(multiple);
        assert!(!redacted.contains("one:a"));
        assert!(!redacted.contains("two:b"));
        assert_eq!(redact_sensitive_text(&redacted), redacted);
    }

    proptest! {
        #[test]
        fn generated_userinfo_is_never_retained(
            user in "[a-zA-Z0-9]{1,16}",
            password in "[a-zA-Z0-9_-]{1,24}",
        ) {
            let text = format!("failed: rtsp://{user}:{password}@camera.local/live");
            let redacted = redact_sensitive_text(&text);
            let credentials = format!("{user}:{password}");
            prop_assert!(!redacted.contains(&credentials));
            prop_assert!(redacted.contains("rtsp://<credentials>@camera.local/live"));
        }
    }

    #[test]
    fn event_dispatcher_preserves_reentrant_event_order() {
        let observed = Rc::new(RefCell::new(Vec::new()));
        let events = Rc::new(RefCell::new(EventDispatcher {
            handler: None,
            queued: VecDeque::new(),
            dispatching: false,
        }));
        let weak_events = Rc::downgrade(&events);
        let observed_for_handler = observed.clone();
        events.borrow_mut().handler = Some(Box::new(move |event| {
            observed_for_handler.borrow_mut().push(event.clone());
            if event == PlaybackEvent::StateChanged(PlaybackState::Starting) {
                let context = ControllerContext {
                    inner: Weak::new(),
                    events: weak_events.clone(),
                };
                emit_event(
                    &context,
                    PlaybackEvent::StateChanged(PlaybackState::Playing),
                );
            }
        }));
        let context = ControllerContext {
            inner: Weak::new(),
            events: Rc::downgrade(&events),
        };

        emit_event(
            &context,
            PlaybackEvent::StateChanged(PlaybackState::Starting),
        );

        assert_eq!(
            *observed.borrow(),
            [
                PlaybackEvent::StateChanged(PlaybackState::Starting),
                PlaybackEvent::StateChanged(PlaybackState::Playing),
            ]
        );
        assert!(!events.borrow().dispatching);
    }

    #[test]
    #[serial]
    fn media_component_synthetic_pipeline_delivers_frames_and_eos() {
        gst::init().unwrap();
        let pipeline = gst::parse::launch(
            "videotestsrc num-buffers=5 ! videoconvert ! fakesink name=test_sink sync=false",
        )
        .unwrap()
        .downcast::<gst::Pipeline>()
        .unwrap();
        let sink = pipeline.by_name("test_sink").unwrap();
        let pad = sink.static_pad("sink").unwrap();
        let frames = Arc::new(AtomicU64::new(0));
        let probe_frames = frames.clone();
        let probe = pad
            .add_probe(gst::PadProbeType::BUFFER, move |_, _| {
                probe_frames.fetch_add(1, Ordering::Relaxed);
                gst::PadProbeReturn::Ok
            })
            .unwrap();

        pipeline.set_state(gst::State::Playing).unwrap();
        let message = pipeline.bus().unwrap().timed_pop_filtered(
            gst::ClockTime::from_seconds(5),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        pipeline.set_state(gst::State::Null).unwrap();
        pad.remove_probe(probe);

        assert!(message.is_some_and(|message| matches!(message.view(), gst::MessageView::Eos(_))));
        assert_eq!(frames.load(Ordering::Relaxed), 5);
    }
}
