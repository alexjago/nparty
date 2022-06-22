//! nparty: N-Party-Preferred distribution of Australian Senate ballots and subsequent analysis.  
//! Copyright (C) 2017-2022  Alex Jago <abjago@abjago.net>.
//! Released under the MIT or Apache-2.0 licenses, at your option.
#[macro_use]
extern crate serde_derive;

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

/// Sets up the application
fn main() -> anyhow::Result<()> {
    let mut settings = Settings::default();
    settings.custom_font = Some(std::borrow::Cow::Borrowed(include_bytes!(
        r"SourceCodePro-Medium.ttf"
    )));
    settings.enable_env = None;

    run_derived::<app::Cli, _>(settings, |n| {
        app::actual(n).unwrap();
    });
    Ok(())
}
