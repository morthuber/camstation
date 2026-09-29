mod application;
mod config;
mod media;
mod ui;

use anyhow::{Context, Result};
use clap::Parser;

use crate::application::Options;

fn main() -> Result<()> {
    let options = Options::parse();
    application::init_logging(options.log.as_deref())?;

    gstreamer::init().context("failed to initialize GStreamer")?;
    tracing::info!(
        gstreamer_version = %gstreamer::version_string(),
        "initialized media runtime"
    );

    application::run(options);
    Ok(())
}
