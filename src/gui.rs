//! nparty: N-Party-Preferred distribution of Australian Senate ballots and subsequent analysis.  
//! Copyright (C) 2017-2022  Alex Jago <abjago@abjago.net>.
//! Released under the MIT or Apache-2.0 licenses, at your option.
#![deny(clippy::all)]
#[macro_use]
extern crate serde_derive;

use clap::{AppSettings, Parser};
use klask::{run_derived, Settings};

mod aggregator;
mod app;
mod booths;
mod config;
mod data;
mod multiplier;
mod term;
mod upgrades;
mod utils;

use crate::app::CliCommands;

#[derive(Parser, Debug)]
#[clap(version, about)]
#[clap(global_setting(AppSettings::PropagateVersion))]
#[clap(global_setting(AppSettings::UseLongFormatForHelpSubcommand))]
pub struct Gui {
    // This is different to app::Cli because we have a fixed verbosity here
    #[clap(subcommand)]
    pub command: CliCommands,
}

/// Run the GUI.
///
/// Log level is set to INFO.
fn main() -> color_eyre::eyre::Result<()> {
    // install color_eyre with a null theme
    color_eyre::config::HookBuilder::new()
        .display_env_section(false)
        .install()?;
    let mut settings = Settings::default();
    settings.custom_font = Some(std::borrow::Cow::Borrowed(include_bytes!(
        r"SourceCodePro-Medium.ttf"
    )));
    settings.enable_env = None;

    let mut rez = color_eyre::eyre::Result::Ok(());

    run_derived::<Gui, _>(settings, |n| {
        tracing_subscriber::fmt()
            .with_max_level(tracing_subscriber::filter::LevelFilter::INFO)
            .with_target(false)
            .without_time()
            .init();

        rez = app::actual(n.command);
    });
    rez
}
