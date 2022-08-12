#![allow(dead_code)]
// there's a bunch of imports from the modules we need that won't be used.

use std::path::PathBuf;

use color_eyre::eyre::{eyre, Result};

#[macro_use]
extern crate serde_derive;

mod term;
mod utils;

use crate::utils::open_csvz_from_path;

fn main() -> Result<()> {
    if let Some(arg1) = std::env::args().nth(1) {
        let pathy = PathBuf::from(arg1);

        let mut progress = 0_usize;
        let mut total_len = 0_usize;
        let mut longbois = 0_usize;

        let mut record = csv::ByteRecord::new();

        let mut prefs_rdr = csv::ReaderBuilder::new()
            .flexible(true)
            .escape(Some(b'\\'))
            .from_reader(open_csvz_from_path(&pathy)?);

        while prefs_rdr.read_byte_record(&mut record)? {
            progress += 1;
            total_len += record.len();
            if record.len() > total_len / progress {
                longbois += 1;
            }
        }
        println!("{progress} records, {total_len} fields, {longbois} longer than avg\n");
        Ok(())
    } else {
        Err(eyre!("No path supplied."))
    }
}
