//! SPIKE, throwaway: writes the game's own map footprint data to a file so we can check whether it can place the
//! character on the TIPS tile map without hand calibration.
//!
//! What it saves (to `tips_mapdump.json` next to the DLL, never sent anywhere):
//! - every entry in the game's map table (GWCA `AreaInfo`): the map's icon position and its rectangle on the
//!   in-game world map, indexed by map ID;
//! - the current map's `map_boundaries` (GWCA `MapContext`, five floats GWCA doesn't name) and the first 0x80 bytes
//!   of the map context, so the meaning of those floats can be worked out;
//! - optionally, a recording of the character's position (every candidate in `position.rs`) while walking.
//!
//! Like `position.rs`, every read goes through `gw::read_into`, which fails instead of faulting.
//! The offsets and byte pattern come from GWCA and are unconfirmed against a running client.

use crate::{gw, position};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Instant;

/// GWCA's pattern for the map table: `6B C6 7C 5E 05`, with the table's address stored in the 4 bytes that follow.
const AREA_TABLE_PATTERN: &[u8] = b"\x6B\xC6\x7C\x5E\x05";
const AREA_TABLE_MASK: &str = "xxxxx";
const AREA_TABLE_PTR_OFFSET: usize = 5;
/// GWCA `AreaInfo` is 0x7C bytes.
const AREA_STRIDE: usize = 0x7C;
/// Map IDs in the names table go up to about 890.
const MAX_MAP_ID: usize = 1024;
/// How much of the map context to keep raw, to decode `map_boundaries` and what sits around it.
const CONTEXT_RAW_BYTES: usize = 0x80;
/// About 80 minutes at four samples a second.
const MAX_SAMPLES: usize = 20_000;

fn u32_at(raw: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([raw[off], raw[off + 1], raw[off + 2], raw[off + 3]])
}

/// One map table entry as JSON, or `None` if it is empty (all the fields we care about are zero).
pub fn area_json(id: u32, raw: &[u8]) -> Option<Value> {
    if raw.len() < AREA_STRIDE {
        return None;
    }
    let f = |off: usize| u32_at(raw, off);
    let (x, y) = (f(0x40), f(0x44));
    let rect = [f(0x48), f(0x4C), f(0x50), f(0x54)];
    if x == 0 && y == 0 && rect == [0, 0, 0, 0] {
        return None;
    }
    Some(json!({
        "id": id,
        "campaign": f(0x00),
        "continent": f(0x04),
        "region": f(0x08),
        "type": f(0x0C),
        "flags": f(0x10),
        "x": x,
        "y": y,
        "icon_start": [rect[0], rect[1]],
        "icon_end": [rect[2], rect[3]],
        "icon_start_dupe": [f(0x58), f(0x5C)],
        "icon_end_dupe": [f(0x60), f(0x64)],
    }))
}

/// The current map's `map_boundaries`: five floats at the start of the map context.
pub fn read_bounds() -> Option<[f32; 5]> {
    let ctx = gw::map_context_addr()?;
    let mut b = [0u8; 20];
    if !gw::read_into(ctx, &mut b) {
        return None;
    }
    Some(std::array::from_fn(|i| f32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])))
}

/// The first `CONTEXT_RAW_BYTES` of the map context, each word as hex and as a float.
fn context_raw() -> Option<Value> {
    let ctx = gw::map_context_addr()?;
    let mut b = [0u8; CONTEXT_RAW_BYTES];
    if !gw::read_into(ctx, &mut b) {
        return None;
    }
    let words: Vec<Value> = (0..CONTEXT_RAW_BYTES / 4)
        .map(|i| {
            let w = u32_at(&b, 4 * i);
            json!({ "off": format!("{:#04x}", 4 * i), "hex": format!("{w:#010x}"), "f32": f32::from_bits(w) as f64 })
        })
        .collect();
    Some(Value::Array(words))
}

fn pos_json(p: &Option<position::Pos>) -> Value {
    match p {
        Some(p) => json!([p.x as f64, p.y as f64, p.z as f64]),
        None => Value::Null,
    }
}

struct Sample {
    t_ms: u64,
    map_id: u32,
    reading: position::Reading,
    bounds: Option<[f32; 5]>,
}

#[derive(Default)]
pub struct Recorder {
    pub recording: bool,
    started: Option<Instant>,
    samples: Vec<Sample>,
    table: Option<usize>,
    scanned: bool,
    /// What to show the user: where the file went, or why it didn't.
    pub message: String,
}

impl Recorder {
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn start(&mut self) {
        self.samples.clear();
        self.started = Some(Instant::now());
        self.recording = true;
        self.message.clear();
    }

    pub fn push(&mut self, map_id: u32, reading: &position::Reading, bounds: Option<[f32; 5]>) {
        if !self.recording {
            return;
        }
        if self.samples.len() >= MAX_SAMPLES {
            self.recording = false;
            self.message = "Recording stopped: the sample limit was reached. Save the file.".into();
            return;
        }
        let t_ms = self.started.map_or(0, |t| t.elapsed().as_millis() as u64);
        self.samples.push(Sample { t_ms, map_id, reading: reading.clone(), bounds });
    }

    /// Address of the map table, found by scanning the game image the first time (tens of ms).
    fn table(&mut self) -> Option<usize> {
        if !self.scanned {
            self.scanned = true;
            self.table = find_area_table();
            tracing::info!("map table {}", self.table.map_or("not found".to_string(), |a| format!("at {a:#x}")));
        }
        self.table
    }

    /// Writes everything gathered so far to `path`. `current` is the loaded map's ID and whether it is explorable.
    pub fn save(&mut self, path: Option<PathBuf>, current: Option<(u32, bool)>) {
        let Some(path) = path else {
            self.message = "Couldn't work out where the DLL is, so nowhere to save.".into();
            return;
        };
        let table = self.table();
        let mut areas = Vec::new();
        if let Some(table) = table {
            let mut raw = [0u8; AREA_STRIDE];
            for id in 0..MAX_MAP_ID {
                if gw::read_into(table + id * AREA_STRIDE, &mut raw) {
                    if let Some(a) = area_json(id as u32, &raw) {
                        areas.push(a);
                    }
                }
            }
        }
        let samples: Vec<Value> = self
            .samples
            .iter()
            .map(|s| {
                json!({
                    "t_ms": s.t_ms,
                    "map_id": s.map_id,
                    "agent_id": s.reading.agent_id,
                    "agent": pos_json(&s.reading.agent),
                    "movement_a": pos_json(&s.reading.movement_a),
                    "movement_b": pos_json(&s.reading.movement_b),
                    "bounds": s.bounds.map(|b| b.map(|v| v as f64)),
                })
            })
            .collect();
        let doc = json!({
            "note": "SPIKE: map table, map context and a walk, for working out world-map placement. Not sent anywhere.",
            "current_map_id": current.map(|c| c.0),
            "current_is_explorable": current.map(|c| c.1),
            "map_table_found": table.is_some(),
            "map_table_addr": table.map(|a| format!("{a:#x}")),
            "map_context_bounds": read_bounds().map(|b| b.map(|v| v as f64)),
            "map_context_raw": context_raw(),
            "area_count": areas.len(),
            "areas": areas,
            "sample_count": samples.len(),
            "samples": samples,
        });
        self.message = match serde_json::to_string(&doc) {
            Ok(text) => match std::fs::write(&path, &text) {
                Ok(()) => format!("Saved {} maps and {} samples to {}", areas.len(), samples.len(), path.display()),
                Err(e) => format!("Couldn't write {}: {e}", path.display()),
            },
            Err(e) => format!("Couldn't build the file: {e}"),
        };
        tracing::info!("{}", self.message);
    }
}

fn find_area_table() -> Option<usize> {
    let (base, size) = position::main_module()?;
    let at = position::scan(base, size, AREA_TABLE_PATTERN, AREA_TABLE_MASK)?;
    let table = gw::read_u32(at + AREA_TABLE_PTR_OFFSET)? as usize;
    // Sanity check: the first entry has to be readable.
    let mut first = [0u8; AREA_STRIDE];
    (table != 0 && gw::read_into(table, &mut first)).then_some(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(set: &[(usize, u32)]) -> Vec<u8> {
        let mut raw = vec![0u8; AREA_STRIDE];
        for (off, v) in set {
            raw[*off..*off + 4].copy_from_slice(&v.to_le_bytes());
        }
        raw
    }

    #[test]
    fn empty_entries_are_skipped() {
        assert!(area_json(5, &entry(&[(0x04, 1)])).is_none());
    }

    #[test]
    fn reads_the_icon_position_and_rectangle() {
        let raw = entry(&[(0x04, 1), (0x40, 1000), (0x44, 2000), (0x48, 900), (0x4C, 1900), (0x50, 1100), (0x54, 2100)]);
        let a = area_json(449, &raw).unwrap();
        assert_eq!(a["id"], 449);
        assert_eq!(a["continent"], 1);
        assert_eq!(a["x"], 1000);
        assert_eq!(a["y"], 2000);
        assert_eq!(a["icon_start"], json!([900, 1900]));
        assert_eq!(a["icon_end"], json!([1100, 2100]));
    }

    #[test]
    fn recording_stops_at_the_limit() {
        let mut r = Recorder::default();
        r.start();
        let reading = position::Reading::default();
        for _ in 0..MAX_SAMPLES + 5 {
            r.push(1, &reading, None);
        }
        assert_eq!(r.len(), MAX_SAMPLES);
        assert!(!r.recording);
    }

    #[test]
    fn nothing_is_recorded_until_started() {
        let mut r = Recorder::default();
        r.push(1, &position::Reading::default(), None);
        assert_eq!(r.len(), 0);
    }
}
