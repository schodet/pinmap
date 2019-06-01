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

//! This module handles loading parts information from database.
use flate2::read::GzDecoder;
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Read;
use std::path::Path;

static EXT: &str = ".xml.gz";

type Result<T> = std::result::Result<T, Box<Error>>;

/// Information about a part.
#[derive(Debug)]
pub struct PartInfo<'a> {
    /// Part.
    pub part: &'a str,
    /// Product line.
    pub line: String,
    /// Package.
    pub package: String,
    /// GPIO mapping mode.
    pub gpio_mode: GpioMode,
    /// Information for all pins.
    pub pins: Vec<PinInfo>,
}

/// Information about one pin.
#[derive(Debug)]
pub struct PinInfo {
    /// Name.
    pub name: String,
    /// Position in package.  This can be a number or a letter with a number.
    pub position: String,
    /// Signals.
    pub signals: Vec<SignalInfo>,
}

/// Information about one signal.
#[derive(Debug)]
pub struct SignalInfo {
    /// Name.
    pub name: String,
    /// Mapping information.
    pub map: SignalMap,
}

/// Information on how to map a signal to a pin.
#[derive(Clone, Debug)]
pub enum SignalMap {
    /// Alternate function, with its AF number.
    AF(u8),
    /// Additional function, no AF setup to do.
    AddF,
    /// Remap, used on older parts without the AF system.  The signal can be available on several
    /// remaps.
    Remap(Vec<u8>),
}

/// Mode of GPIO mapping.
#[derive(Clone, Copy, Debug)]
pub enum GpioMode {
    /// Alternate function based mapping.
    AF,
    /// Remap based mapping.
    Remap,
}

/// Map pins and signals to mapping information.  This is used temporarily when loading it from a
/// separated file.
type GpiosInfo = HashMap<String, HashMap<String, SignalMap>>;

impl<'a> PartInfo<'a> {
    /// Extract information from XML file in database.
    pub fn new(database: &Path, part: &'a str) -> Result<PartInfo<'a>> {
        // Read XML.
        let xml_name = database.join(["mcu/", part, EXT].concat());
        let xml = read_gziped(&xml_name)?;
        let doc = Document::parse(&xml)?;
        let doc_root = doc.root_element();
        // Basic attributes.
        let line = attribute_or_error(&doc_root, "Line")?;
        let package = attribute_or_error(&doc_root, "Package")?;
        // GPIO.
        let gpio_ip = doc_root
            .children()
            .find(|n| n.has_tag_name("IP") && n.attribute("Name") == Some("GPIO"))
            .ok_or("missing GPIO")?;
        let gpio_version = attribute_or_error(&gpio_ip, "Version")?;
        let (gpio_mode, gpios_info) = load_gpios(database, &gpio_version)?;
        // Pins.
        fn parse_signal(
            signals_map: Option<&HashMap<String, SignalMap>>,
            s: Node,
        ) -> Result<SignalInfo> {
            let name = attribute_or_error(&s, "Name")?;
            let map = match signals_map {
                None => SignalMap::AddF,
                Some(signals_map) => signals_map.get(&name).unwrap_or(&SignalMap::AddF).clone(),
            };
            Ok(SignalInfo { name, map })
        };
        fn parse_pin(gpios_info: &GpiosInfo, n: Node) -> Result<PinInfo> {
            let name = attribute_or_error(&n, "Name")?;
            let position = attribute_or_error(&n, "Position")?;
            let signals = n
                .children()
                .filter(|s| s.has_tag_name("Signal") && s.attribute("Name") != Some("GPIO"))
                .map(|s| {
                    let signals_map = gpios_info.get(&name);
                    parse_signal(signals_map, s)
                })
                .collect::<Result<_>>()?;
            Ok(PinInfo {
                name,
                position,
                signals,
            })
        };
        let pins = doc_root
            .children()
            .filter(|n| n.has_tag_name("Pin"))
            .map(|n| parse_pin(&gpios_info, n))
            .collect::<Result<_>>()?;
        // Done.
        Ok(PartInfo {
            part,
            line,
            package,
            gpio_mode,
            pins,
        })
    }
    /// Produce a one-line part summary.
    pub fn summary(self: &Self) -> String {
        format!("{}: {} {}", self.part, self.line, self.package)
    }
}

/// List all parts in database matching a given regex.
pub fn list_parts(database: &Path, pattern: &str) -> Result<Vec<String>> {
    let re = regex::Regex::new(pattern)?;
    let mut list = Vec::new();
    for entry in database.join("mcu").read_dir()? {
        if let Some(name) = entry?.file_name().to_str() {
            if name.ends_with(EXT) {
                let part = &name[..(name.len() - EXT.len())];
                if re.is_match(part) {
                    list.push(part.to_owned());
                }
            }
        }
    }
    Ok(list)
}

/// Load information on GPIOs from XML file in database.  Return a hash indexed by pin and signal,
/// giving signal mapping information.
fn load_gpios(database: &Path, gpio_version: &str) -> Result<(GpioMode, GpiosInfo)> {
    // Read XML.
    let xml_name = database.join(["mcu/IP/GPIO-", gpio_version, "_Modes", EXT].concat());
    let xml = read_gziped(&xml_name)?;
    let doc = Document::parse(&xml)?;
    let doc_root = doc.root_element();
    // Decode document.
    fn parse_af(signal: Node) -> Result<SignalMap> {
        let af = signal
            .descendants()
            .find(|n| n.has_tag_name("PossibleValue"))
            .ok_or("no AF found")?
            .text()
            .ok_or("no AF text")?;
        let k = "GPIO_AF";
        if !af.starts_with(k) {
            return Err("not an AF".into());
        }
        let i = af[k.len()..].find('_').ok_or("not an AF")?;
        let af = &af[k.len()..k.len() + i];
        let af = af.parse::<u8>()?;
        Ok(SignalMap::AF(af))
    }
    fn parse_remaps(signal: Node) -> Result<SignalMap> {
        let remap_blocks = signal.children().filter(|n| n.has_tag_name("RemapBlock"));
        fn parse_remap(n: Node) -> Result<u8> {
            let name = attribute_or_error(&n, "Name")?;
            let k = "REMAP";
            let i = name.rfind(k).ok_or("missing REMAP")?;
            let remap = name[i + k.len()..].parse::<u8>()?;
            Ok(remap)
        }
        let remaps = remap_blocks.map(parse_remap).collect::<Result<_>>()?;
        Ok(SignalMap::Remap(remaps))
    }
    let mut gpios = HashMap::new();
    let pins = doc_root.children().filter(|n| n.has_tag_name("GPIO_Pin"));
    let mut mode = None;
    for pin in pins {
        let pin_name = attribute_or_error(&pin, "Name")?;
        let signals = pin.children().filter(|n| n.has_tag_name("PinSignal"));
        let mut signals_map = HashMap::new();
        for signal in signals {
            // First try to parse a remap until this fails once.
            if mode.is_none() {
                let one_remap_block = signal.children().find(|n| n.has_tag_name("RemapBlock"));
                mode = if one_remap_block.is_some() {
                    Some(GpioMode::Remap)
                } else {
                    Some(GpioMode::AF)
                }
            }
            let map = match mode.unwrap() {
                GpioMode::AF => parse_af(signal),
                GpioMode::Remap => parse_remaps(signal),
            }?;
            let signal_name = attribute_or_error(&signal, "Name")?;
            signals_map.insert(signal_name, map);
        }
        gpios.insert(pin_name, signals_map);
    }
    Ok((mode.unwrap_or(GpioMode::AF), gpios))
}

/// Read gziped file to string.
fn read_gziped(path: &Path) -> Result<String> {
    let mut gunzip = GzDecoder::new(File::open(path)?);
    let mut xml = String::new();
    gunzip.read_to_string(&mut xml)?;
    Ok(xml)
}

/// Factorize attribute getter, return an error if not found.
fn attribute_or_error(node: &Node, name: &str) -> Result<String> {
    match node.attribute(name) {
        Some(v) => Ok(v.to_owned()),
        None => {
            let tag = node.tag_name().name();
            Err(format!("{} missing a {} attribute", tag, name).into())
        }
    }
}
