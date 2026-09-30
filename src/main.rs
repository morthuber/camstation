use anyhow::{Context, Result};
use clap::Parser;

use camstation::application::{self, Options};

fn main() -> Result<()> {
    let options = Options::parse();
    application::init_logging(options.log.as_deref(), options.log_file.as_deref())?;

    gstreamer::init().context("failed to initialize GStreamer")?;
    tracing::info!(
        gstreamer_version = %gstreamer::version_string(),
        "initialized media runtime"
    );

    application::run(options);
    Ok(())
}
