//! GStreamer pipeline construction and per-camera lifecycle management.

mod controller;

pub(crate) use controller::{CameraController, DecoderInfo, PlaybackEvent, PlaybackState};
