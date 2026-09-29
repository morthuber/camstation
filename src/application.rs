use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use gtk4::prelude::*;
use tracing_subscriber::EnvFilter;

use crate::ui;

pub(crate) const APPLICATION_ID: &str = "org.camstation.camstation";

#[derive(Clone, Debug, Parser)]
#[command(author, version, about)]
pub(crate) struct Options {
    /// Start fullscreen in kiosk mode.
    #[arg(long, conflicts_with = "windowed")]
    pub(crate) kiosk: bool,

    /// Ignore kiosk-on-start and open with normal window controls.
    #[arg(long, conflicts_with = "kiosk")]
    pub(crate) windowed: bool,

    /// Override the configured startup view by ID or name.
    #[arg(long, value_name = "ID_OR_NAME")]
    pub(crate) view: Option<String>,

    /// Use an alternate configuration file.
    #[arg(long, value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,

    /// Add an RTSP stream at startup; may be specified up to ten times.
    #[arg(long, value_name = "URL")]
    pub(crate) rtsp_url: Vec<String>,

    /// Override the tracing filter (for example, camstation=debug).
    #[arg(long, value_name = "FILTER")]
    pub(crate) log: Option<String>,
}

pub(crate) fn init_logging(cli_filter: Option<&str>) -> Result<()> {
    let filter = match cli_filter {
        Some(filter) => EnvFilter::try_new(filter).context("invalid --log filter")?,
        None => EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("camstation=info,warn")),
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize logging: {error}"))
}

pub(crate) fn run(options: Options) {
    let application = gtk4::Application::builder()
        .application_id(APPLICATION_ID)
        .build();

    application.connect_activate(move |application| {
        ui::build_main_window(application, &options);
    });

    // Clap owns Camstation's command line; do not let GApplication parse it again.
    let _exit_code = application.run_with_args(&["camstation"]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kiosk_and_windowed_are_mutually_exclusive() {
        assert_eq!(APPLICATION_ID, "org.camstation.camstation");
        assert!(Options::try_parse_from(["camstation", "--kiosk", "--windowed"]).is_err());
        assert!(
            Options::try_parse_from(["camstation", "--windowed"])
                .unwrap()
                .windowed
        );
    }
}
