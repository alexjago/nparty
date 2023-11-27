//! nparty: N-Party-Preferred distribution of Australian Senate ballots and subsequent analysis.  
//! Copyright (C) 2017-2022  Alex Jago <abjago@abjago.net>.
//! Released under the MIT or Apache-2.0 licenses, at your option.

// We have *all* the lints!
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::cast_possible_truncation)]
// reason = "truncations are all collection-lengths (usize) to u32, where u32 is already large for the relevant problem domain"
#![allow(clippy::multiple_crate_versions)]
// reason = "transitive dependencies"
#![allow(clippy::too_many_lines)]
// reason = "mostly translated functions from previous version; TODO: refactor"
#![allow(clippy::items_after_statements)]
// reason = "items defined adjacent to use"
// One known false positive, but an item specific allow doesn't seem to work
#![allow(clippy::future_not_send)] // reason = TotallyNotAZipFile and we aren't multithreaded

// OK on with the show
#[macro_use]
extern crate serde_derive;

use clap::Parser;

mod aggregator;
mod app;
mod booths;
mod config;
mod data;
mod multiplier;
mod term;
mod upgrades;
mod utils;
use app::Cli;

/// Run the CLI.
fn main() -> color_eyre::eyre::Result<()> {
    // Parse command-line arguments...
    let cli = Cli::parse();

    // ... So that we can set a verbosity level
    tracing_subscriber::fmt()
        .with_max_level(match cli.verbose.log_level_filter() {
            log::LevelFilter::Off => tracing_subscriber::filter::LevelFilter::OFF,
            log::LevelFilter::Error => tracing_subscriber::filter::LevelFilter::ERROR,
            log::LevelFilter::Warn => tracing_subscriber::filter::LevelFilter::WARN,
            log::LevelFilter::Info => tracing_subscriber::filter::LevelFilter::INFO,
            log::LevelFilter::Debug => tracing_subscriber::filter::LevelFilter::DEBUG,
            log::LevelFilter::Trace => tracing_subscriber::filter::LevelFilter::TRACE,
        })
        .with_target(false)
        .without_time()
        .init();

    // Initialise sweet coloured error messages
    color_eyre::config::HookBuilder::new()
        .display_env_section(false)
        .install()?;
    std::env::set_var("RUST_SPANTRACE", "0");

    // finally, run the app!
    app::actual(cli.command)
}
