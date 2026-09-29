use anyhow::{Context, Result, anyhow, bail};
use gst::prelude::*;
use gstreamer as gst;

const MINIMUM_RTSP_LATENCY_MS: u32 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlaybackState {
    Stopped,
    Starting,
    Playing,
    Paused,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecoderInfo {
    pub(crate) factories: Vec<String>,
    pub(crate) hardware_accelerated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PlaybackEvent {
    StateChanged(PlaybackState),
    DecoderChanged(DecoderInfo),
    EndOfStream,
    Error(String),
}

pub(crate) struct CameraController {
    playbin: gst::Element,
    _bus_watch: gst::bus::BusWatchGuard,
    paintable: gtk4::gdk::Paintable,
}

impl CameraController {
    pub(crate) fn new<F>(uri: &str, mut emit: F) -> Result<Self>
    where
        F: FnMut(PlaybackEvent) + 'static,
    {
        validate_rtsp_uri(uri)?;

        let video_sink = gst::ElementFactory::make("gtk4paintablesink")
            .name("camview_video_sink")
            .property("sync", false)
            .build()
            .context("GStreamer plugin 'gtk4paintablesink' is unavailable")?;
        let paintable = video_sink.property::<gtk4::gdk::Paintable>("paintable");

        let audio_sink = gst::ElementFactory::make("fakesink")
            .name("camview_muted_audio_sink")
            .property("sync", true)
            .build()
            .context("GStreamer plugin 'fakesink' is unavailable")?;

        let playbin = gst::ElementFactory::make("playbin3")
            .name("camview_player")
            .property("uri", uri)
            .property("video-sink", &video_sink)
            .property("audio-sink", &audio_sink)
            .property("mute", true)
            .build()
            .context("GStreamer plugin 'playbin3' is unavailable")?;

        configure_rtsp_source(&playbin);

        let bus = playbin
            .bus()
            .context("playback pipeline did not provide a GStreamer bus")?;
        let watched_playbin = playbin.clone();
        let mut last_decoder = None;
        let bus_watch = bus
            .add_watch_local(move |_, message| {
                match message.view() {
                    gst::MessageView::Error(error) => {
                        let message = redact_sensitive_text(&error.error().to_string());
                        tracing::warn!(error = %message, "camera pipeline failed");
                        emit(PlaybackEvent::Error(message));
                    }
                    gst::MessageView::Eos(_) => {
                        tracing::warn!("camera pipeline reached end of stream");
                        emit(PlaybackEvent::EndOfStream);
                    }
                    gst::MessageView::StateChanged(state)
                        if message_is_from(message, &watched_playbin) =>
                    {
                        let state = match state.current() {
                            gst::State::Null => PlaybackState::Stopped,
                            gst::State::Ready | gst::State::Paused => PlaybackState::Paused,
                            gst::State::Playing => PlaybackState::Playing,
                            _ => PlaybackState::Starting,
                        };
                        tracing::debug!(?state, "camera pipeline state changed");
                        emit(PlaybackEvent::StateChanged(state));

                        if state == PlaybackState::Playing {
                            emit_decoder_if_changed(&watched_playbin, &mut last_decoder, &mut emit);
                        }
                    }
                    gst::MessageView::AsyncDone(_) | gst::MessageView::StreamStart(_) => {
                        emit_decoder_if_changed(&watched_playbin, &mut last_decoder, &mut emit);
                    }
                    _ => {}
                }

                gst::glib::ControlFlow::Continue
            })
            .context("failed to attach the camera pipeline bus watch")?;

        Ok(Self {
            playbin,
            _bus_watch: bus_watch,
            paintable,
        })
    }

    pub(crate) fn paintable(&self) -> &gtk4::gdk::Paintable {
        &self.paintable
    }

    pub(crate) fn start(&self) -> Result<()> {
        self.playbin
            .set_state(gst::State::Playing)
            .map_err(|error| anyhow!("failed to start camera pipeline: {error:?}"))?;
        Ok(())
    }

    pub(crate) fn stop(&self) {
        if let Err(error) = self.playbin.set_state(gst::State::Null) {
            tracing::warn!(?error, "failed to stop camera pipeline cleanly");
        }
    }
}

impl Drop for CameraController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn configure_rtsp_source(playbin: &gst::Element) {
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

fn emit_decoder_if_changed<F>(
    playbin: &gst::Element,
    last_decoder: &mut Option<DecoderInfo>,
    emit: &mut F,
) where
    F: FnMut(PlaybackEvent),
{
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
    emit(PlaybackEvent::DecoderChanged(decoder));
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

fn message_is_from(message: &gst::MessageRef, element: &gst::Element) -> bool {
    message
        .src()
        .and_then(|source| source.downcast_ref::<gst::Element>())
        .is_some_and(|source| source == element)
}

pub(crate) fn validate_rtsp_uri(uri: &str) -> Result<()> {
    let uri = uri.trim();
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
    let authority = &uri[authority_start..];
    if authority.is_empty() || authority.starts_with('/') {
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

    #[test]
    fn validates_supported_rtsp_urls() {
        assert!(validate_rtsp_uri("rtsp://camera.local/stream").is_ok());
        assert!(validate_rtsp_uri("rtsps://user:password@camera.local/stream").is_ok());
    }

    #[test]
    fn rejects_invalid_rtsp_urls() {
        assert!(validate_rtsp_uri("").is_err());
        assert!(validate_rtsp_uri("https://camera.local/stream").is_err());
        assert!(validate_rtsp_uri("rtsp:///stream").is_err());
        assert!(validate_rtsp_uri("rtsp://camera.local/bad stream").is_err());
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
    }
}
