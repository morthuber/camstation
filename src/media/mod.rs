//! GStreamer pipeline construction and per-camera lifecycle management.

mod connection_test;
mod controller;

pub(crate) use connection_test::ConnectionTest;
pub(crate) use controller::{CameraController, DecoderInfo, PlaybackEvent, PlaybackState};
