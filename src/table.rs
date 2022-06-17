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

//! Handle table output.
use crate::db;
use itertools::Itertools;
use regex::{Regex, RegexSet};
use std::collections::hash_set::HashSet;
use std::collections::HashMap;
use std::error::Error;
use std::io::Write;
use std::result::Result as StdResult;

type Result<T> = StdResult<T, Box<dyn Error>>;

/// Filter signals to reduce pin out table size.
pub struct SignalFilter {
    /// Signals to exclude from table.
    excludes: RegexSet,
    /// Substitutions to shorten signal names.
    subs: Vec<Regex>,
    /// Factorizations to reduce the number of similar signals, with the associated separator.
    facts_sep: Vec<(Regex, &'static str)>,
}

/// Produce a pin out table.
pub fn write_pin_out(
    part_info: &db::PartInfo,
    writer: impl Write,
    filter: &SignalFilter,
) -> Result<()> {
    match part_info.gpio_mode {
        db::GpioMode::AF => write_pin_out_af(part_info, writer, filter),
        db::GpioMode::Remap => write_pin_out_remap(part_info, writer, filter),
    }
}

/// Produce a pin out table for AF based parts.
fn write_pin_out_af(
    part_info: &db::PartInfo,
    writer: impl Write,
    filter: &SignalFilter,
) -> Result<()> {
    let mut writer = csv::Writer::from_writer(writer);
    for pin in &part_info.pins {
        let mut signals: [Vec<_>; 17] = Default::default();
        for signal in &pin.signals {
            let index = match signal.map {
                db::SignalMap::AF(af) => af as usize,
                db::SignalMap::AddF => signals.len() - 1,
                _ => panic!("Bad signal map"),
            };
            signals[index].push(signal.name.as_str());
        }
        let signals = filter.signal_filter(&pin.name, &pin.position, &signals);
        let mut row = Vec::new();
        row.push(pin.name.clone());
        row.push(pin.position.clone());
        for i in &signals {
            row.push(i.join(" "));
        }
        writer.write_record(row)?;
    }
    Ok(())
}

/// Produce a pin out table for Remap based parts.
fn write_pin_out_remap(
    part_info: &db::PartInfo,
    writer: impl Write,
    filter: &SignalFilter,
) -> Result<()> {
    let mut lines = Vec::new();
    let mut allcats = HashSet::new();
    for pin in &part_info.pins {
        let signals = pin
            .signals
            .iter()
            .map(|signal| {
                if let db::SignalMap::Remap(remaps) = &signal.map {
                    let remaps = remaps.iter().sorted().map(|x| x.to_string()).join(",");
                    format!("{}({})", signal.name, remaps)
                } else {
                    signal.name.clone()
                }
            })
            .collect::<Vec<_>>();
        let signals = filter
            .signal_filter(&pin.name, &pin.position, &[signals])
            .into_iter()
            .next()
            .unwrap();
        let mut signals_hash = HashMap::new();
        for signal in signals {
            let cat = signal.split('_').next().unwrap().to_owned();
            allcats.insert(cat.clone());
            signals_hash.entry(cat).or_insert(Vec::new()).push(signal);
        }
        lines.push((&pin.name, &pin.position, signals_hash));
    }
    let mut writer = csv::Writer::from_writer(writer);
    let mut allcats = allcats.into_iter().collect::<Vec<_>>();
    allcats.sort();
    for (name, position, signals_hash) in lines {
        let mut row = Vec::new();
        row.push(name.clone());
        row.push(position.clone());
        for cat in &allcats {
            let col = signals_hash.get(cat);
            if let Some(col) = col {
                row.push(col.iter().join(" "));
            } else {
                row.push(String::from(""));
            }
        }
        writer.write_record(row)?;
    }
    Ok(())
}

impl SignalFilter {
    /// Prepare a new filter.
    pub fn new(exclude: &Vec<String>) -> StdResult<SignalFilter, regex::Error> {
        let excludes = RegexSet::new(exclude.iter().map(|x| format!(r"^(?:{})[0-9_]", x)))?;
        let subs = [
            "((?:HR|LP)?T)IM",
            "((?:LP)?U)S?ART",
            "(D)FSDM",
            "(F)S?MC",
            "(Q)UADSPI(?:_BK)?",
            "(S)PI",
            "(SW)PMI",
            "I2(S)",
            "(SD)MMC",
            "(SP)DIFRX",
            "FD(C)AN",
            "USB_OTG_([FH]S)",
            r"(T\d_B)KIN",
        ]
        .iter()
        .map(|x| Regex::new(&format!(r"^{}([0-9_])", x)))
        .collect::<StdResult<_, _>>()?;
        let facts_sep = [
            (r"T\d_B\d?_COMP(\d+)", ""),
            (r"ADC(\d)_IN[NP]?\d+", ""),
            (r"ADC\d+_IN([NP]?\d+)", ""),
            (r"[SUT]\d_(.+)", "/"),
        ]
        .iter()
        .map(|(fact, sep)| Ok((Regex::new(fact)?, *sep)))
        .collect::<StdResult<_, _>>()?;
        Ok(SignalFilter {
            excludes,
            subs,
            facts_sep,
        })
    }
    /// Filter a list of signal.
    fn signal_filter<'a, I, J, S>(
        self: &Self,
        _name: &str,
        _position: &str,
        cols: I,
    ) -> Vec<Vec<String>>
    where
        S: ToString,
        J: IntoIterator<Item = S>,
        I: IntoIterator<Item = J>,
    {
        let mut res = Vec::new();
        for signals in cols {
            let signals = signals
                .into_iter()
                .map(|s| {
                    self.subs
                        .iter()
                        .fold(s.to_string(), |s, re| re.replace(&s, "$1$2").to_string())
                })
                .collect();
            let signals = self.facts_sep.iter().fold(signals, |signals, (fact, sep)| {
                factorize(&signals, &fact, sep)
            });
            let signals = signals
                .into_iter()
                .filter(|s| !self.excludes.is_match(s))
                .collect();
            res.push(signals);
        }
        res
    }
}

/// For a given iterable, match each items with the given regex, if there are several matches they
/// are factorized on the first subgroup.
fn factorize<I, S>(it: I, re: &Regex, sep: &str) -> Vec<String>
where
    S: ToString,
    I: IntoIterator<Item = S>,
{
    let mut others = Vec::new();
    let mut facts: HashMap<_, Vec<_>> = HashMap::new();
    for i in it {
        let i = i.to_string();
        if let Some(c) = re.captures(&i) {
            let g = c.get(1).expect("first group should match");
            let termout = (&i[..g.start()], &i[g.end()..]);
            let termout = (termout.0.to_owned(), termout.1.to_owned());
            let term = g.as_str().to_owned();
            facts.entry(termout).or_insert(Vec::new()).push(term);
        } else {
            others.push(i);
        }
    }
    let mut r = Vec::new();
    for (termout, terms) in facts {
        let terms = terms.join(sep);
        r.push(format!("{}{}{}", termout.0, terms, termout.1));
    }
    r.extend(others);
    r
}
