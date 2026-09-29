use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use gst::prelude::*;
use gstreamer as gst;

use super::controller::{configure_rtsp_source, redact_sensitive_text, validate_rtsp_uri};

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(8);

type TestCallback = Box<dyn FnOnce(std::result::Result<(), String>)>;

struct TestInner {
    pipeline: Option<gst::Element>,
    bus_watch: Option<gst::bus::BusWatchGuard>,
    timeout_source: Option<gst::glib::SourceId>,
    completion_idle: Option<gst::glib::SourceId>,
    pending_result: Option<std::result::Result<(), String>>,
    callback: Option<TestCallback>,
}

pub(crate) struct ConnectionTest {
    inner: Rc<RefCell<TestInner>>,
}

impl ConnectionTest {
    pub(crate) fn start<F>(uri: &str, callback: F) -> Result<Self>
    where
        F: FnOnce(std::result::Result<(), String>) + 'static,
    {
        validate_rtsp_uri(uri)?;

        let video_sink = gst::ElementFactory::make("fakesink")
            .name("camview_connection_test_video")
            .property("sync", false)
            .build()
            .context("GStreamer plugin 'fakesink' is unavailable")?;
        let audio_sink = gst::ElementFactory::make("fakesink")
            .name("camview_connection_test_audio")
            .property("sync", false)
            .build()
            .context("GStreamer plugin 'fakesink' is unavailable")?;
        let pipeline = gst::ElementFactory::make("playbin3")
            .name("camview_connection_test")
            .property("uri", uri)
            .property("video-sink", &video_sink)
            .property("audio-sink", &audio_sink)
            .property("mute", true)
            .build()
            .context("GStreamer plugin 'playbin3' is unavailable")?;
        configure_rtsp_source(&pipeline);

        let inner = Rc::new(RefCell::new(TestInner {
            pipeline: Some(pipeline.clone()),
            bus_watch: None,
            timeout_source: None,
            completion_idle: None,
            pending_result: None,
            callback: Some(Box::new(callback)),
        }));
        let weak_inner = Rc::downgrade(&inner);

        let bus = pipeline
            .bus()
            .context("connection-test pipeline did not provide a GStreamer bus")?;
        let bus_watch = bus
            .add_watch_local({
                let weak_inner = weak_inner.clone();
                let watched_pipeline = pipeline.clone();
                move |_, message| {
                    match message.view() {
                        gst::MessageView::Error(error) => {
                            let error = redact_sensitive_text(&error.error().to_string());
                            queue_completion(&weak_inner, Err(error));
                        }
                        gst::MessageView::StateChanged(state)
                            if message_is_from(message, &watched_pipeline)
                                && state.current() == gst::State::Playing =>
                        {
                            queue_completion(&weak_inner, Ok(()));
                        }
                        _ => {}
                    }
                    gst::glib::ControlFlow::Continue
                }
            })
            .context("failed to attach connection-test bus watch")?;
        inner.borrow_mut().bus_watch = Some(bus_watch);

        let timeout_source = gst::glib::timeout_add_local_once(CONNECTION_TIMEOUT, {
            let weak_inner = weak_inner.clone();
            move || {
                if let Some(inner) = weak_inner.upgrade() {
                    inner.borrow_mut().timeout_source.take();
                }
                queue_completion(
                    &weak_inner,
                    Err(format!(
                        "connection test timed out after {} seconds",
                        CONNECTION_TIMEOUT.as_secs()
                    )),
                );
            }
        });
        inner.borrow_mut().timeout_source = Some(timeout_source);

        if let Err(error) = pipeline.set_state(gst::State::Playing) {
            stop_inner(&inner);
            return Err(anyhow!("failed to start connection test: {error:?}"));
        }

        Ok(Self { inner })
    }
}

impl Drop for ConnectionTest {
    fn drop(&mut self) {
        stop_inner(&self.inner);
    }
}

fn queue_completion(
    weak_inner: &Weak<RefCell<TestInner>>,
    result: std::result::Result<(), String>,
) {
    let Some(inner) = weak_inner.upgrade() else {
        return;
    };
    let mut inner = inner.borrow_mut();
    if inner.pending_result.is_some() || inner.completion_idle.is_some() {
        return;
    }
    inner.pending_result = Some(result);

    let weak_inner = weak_inner.clone();
    inner.completion_idle = Some(gst::glib::idle_add_local_once(move || {
        finish_test(&weak_inner);
    }));
}

fn finish_test(weak_inner: &Weak<RefCell<TestInner>>) {
    let Some(inner) = weak_inner.upgrade() else {
        return;
    };

    let (pipeline, bus_watch, timeout_source, callback, result) = {
        let mut inner = inner.borrow_mut();
        inner.completion_idle.take();
        (
            inner.pipeline.take(),
            inner.bus_watch.take(),
            inner.timeout_source.take(),
            inner.callback.take(),
            inner.pending_result.take(),
        )
    };

    drop(bus_watch);
    if let Some(source) = timeout_source {
        source.remove();
    }
    if let Some(pipeline) = pipeline {
        let _ = pipeline.set_state(gst::State::Null);
    }
    if let (Some(callback), Some(result)) = (callback, result) {
        callback(result);
    }
}

fn stop_inner(inner: &Rc<RefCell<TestInner>>) {
    let (pipeline, bus_watch, timeout_source, completion_idle) = {
        let mut inner = inner.borrow_mut();
        inner.callback.take();
        inner.pending_result.take();
        (
            inner.pipeline.take(),
            inner.bus_watch.take(),
            inner.timeout_source.take(),
            inner.completion_idle.take(),
        )
    };

    drop(bus_watch);
    if let Some(source) = timeout_source {
        source.remove();
    }
    if let Some(source) = completion_idle {
        source.remove();
    }
    if let Some(pipeline) = pipeline {
        let _ = pipeline.set_state(gst::State::Null);
    }
}

fn message_is_from(message: &gst::MessageRef, element: &gst::Element) -> bool {
    message
        .src()
        .and_then(|source| source.downcast_ref::<gst::Element>())
        .is_some_and(|source| source == element)
}
