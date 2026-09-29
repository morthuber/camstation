//! GStreamer pipeline construction and per-camera lifecycle management.

mod connection_test;
mod controller;
mod lifecycle;

pub(crate) use connection_test::ConnectionTest;
pub(crate) use controller::{CameraController, PlaybackEvent, PlaybackState};

/// Remove the URI fragment before handing an RTSP URL to GStreamer.
///
/// Fragments are client-side metadata and must not be included in RTSP request
/// targets. VLC strips them, but GStreamer's RTSP source sends them verbatim.
fn normalize_rtsp_uri(uri: &str) -> &str {
    uri.split_once('#')
        .map_or(uri, |(request_uri, _)| request_uri)
}

#[cfg(test)]
mod tests {
    use super::normalize_rtsp_uri;

    #[test]
    fn strips_rtsp_uri_fragments_without_changing_the_query() {
        assert_eq!(
            normalize_rtsp_uri(
                "rtsp://camera.local/cam/realmonitor?channel=1&subtype=0#media=video"
            ),
            "rtsp://camera.local/cam/realmonitor?channel=1&subtype=0"
        );
    }

    #[test]
    fn leaves_fragment_free_rtsp_uris_unchanged() {
        let uri = "rtsps://camera.local/live?token=secret";
        assert_eq!(normalize_rtsp_uri(uri), uri);
    }
}
