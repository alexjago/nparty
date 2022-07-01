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
#![allow(clippy::multiple_crate_versions)] // reason = "transitive dependencies"
#![allow(clippy::too_many_lines)] // reason = "mostly translated functions from previous version; TODO: split"
#![allow(clippy::cognitive_complexity)] // reason = "mostly translated functions from previous version; TODO: split"
#![allow(clippy::items_after_statements)] // reason = "items defined adjacent to use"
#![allow(clippy::use_self)] // reason = "derive-related bugs"
                            // One known false positive, but an item specific allow doesn't seem to work

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
fn main() -> anyhow::Result<()> {
    app::actual(Cli::parse())
}
