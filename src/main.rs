// Copyright (C) 2019 Nicolas Schodet
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of this software
// and associated documentation files (the "Software"), to deal in the Software without
// restriction, including without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all copies or
// substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
// BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

//! Help with pin assignment on STM32.
//!
//! This reads database extracted from CubeMX and produce a table of all signals that can be mapped
//! to the microcontroller pins.  This table can be open with a spreadsheet.
use std::error::Error;
use std::io;
use std::path::PathBuf;
use structopt::StructOpt;

mod db;
mod table;

/// MCU pins mapper.
#[derive(StructOpt, Debug)]
struct Opt {
    /// Database path
    #[structopt(short = "d", long, default_value = "db", parse(from_os_str))]
    database: PathBuf,
    /// Exclude component
    #[structopt(short = "x", long, number_of_values = 1)]
    exclude: Vec<String>,
    #[structopt(subcommand)]
    command: OptCommand,
}

#[derive(StructOpt, Debug)]
enum OptCommand {
    /// Search the database for MCUs matching the given regex.
    #[structopt(name = "parts")]
    Parts { pattern: String },
    /// Output a pin out table for a given part.
    #[structopt(name = "table")]
    Table { part: String },
}

fn main() -> Result<(), Box<Error>> {
    let opt = Opt::from_args();
    match opt.command {
        OptCommand::Parts { pattern } => {
            for part in db::list_parts(&opt.database, &pattern)? {
                let part_info = db::PartInfo::new(&opt.database, &part)?;
                println!("{}", part_info.summary());
            }
        }
        OptCommand::Table { part } => {
            let part_info = db::PartInfo::new(&opt.database, &part)?;
            let filter = table::SignalFilter::new(&opt.exclude)?;
            table::write_pin_out(&part_info, io::stdout(), &filter)?;
        }
    }
    Ok(())
}
