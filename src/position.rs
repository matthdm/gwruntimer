//! Reads the controlled character's position (x, y inside the current map) for the TIPS tracker.
//!
//! This is an experiment: the offsets come from GWCA's headers and its agent-array byte pattern, and
//! haven't been confirmed against a running client yet. So every read goes through `gw::read_into`
//! (ReadProcessMemory on our own process), which fails safely instead of faulting, and the panel shows
//! each candidate value so you can see which one follows your character.
//!
//! It never reads the character's name, account, party or anything else; only coordinates.

use crate::gw;

/// GWCA's pattern for the agent array: `FF 50 10 47 83 C6 04 3B FB 75 E1`. The pointer to the array sits
/// 0x0D bytes after the start of the match.
const AGENT_ARRAY_PATTERN: &[u8] = b"\xFF\x50\x10\x47\x83\xC6\x04\x3B\xFB\x75\xE1";
const AGENT_ARRAY_MASK: &str = "xxxxxxxxxxx";
const AGENT_ARRAY_PTR_OFFSET: usize = 0x0D;

// GWCA `Array<T>`: { buffer, allocated, size, param } (16 bytes).
const ARRAY_BUFFER: usize = 0;
const ARRAY_SIZE: usize = 8;

// GWCA `Agent`: z at +0x30, pos {x, y, plane} at +0x74.
const AGENT_Z: usize = 0x30;
const AGENT_POS: usize = 0x74;
const AGENT_PLANE: usize = 0x7C;

// GWCA `AgentContext`: `Array<AgentMovement*> agent_movement` at +0xE8.
const AGENT_CTX_MOVEMENT: usize = 0xE8;
// GWCA `AgentMovement`: two Vec3f whose purpose GWCA leaves unnamed, at +0x64 and +0x74.
const MOVEMENT_VEC_A: usize = 0x64;
const MOVEMENT_VEC_B: usize = 0x74;

/// Anything beyond this is not a coordinate inside a map.
pub const MAX_COORD: f32 = 100_000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pos {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Pos {
    /// A usable coordinate: finite and in range.
    pub fn is_sane(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.x.abs() <= MAX_COORD && self.y.abs() <= MAX_COORD
    }
}

/// Which memory location the tracker trusts for the position it sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Source {
    /// `Agent.pos` found through GWCA's agent array (the primary candidate).
    #[default]
    Agent,
    /// `AgentMovement` Vec3f at +0x64.
    MovementA,
    /// `AgentMovement` Vec3f at +0x74.
    MovementB,
}

impl Source {
    pub const ALL: [Source; 3] = [Source::Agent, Source::MovementA, Source::MovementB];

    pub fn label(self) -> &'static str {
        match self {
            Source::Agent => "Agent position (agent array)",
            Source::MovementA => "Movement vector A (+0x64)",
            Source::MovementB => "Movement vector B (+0x74)",
        }
    }

    #[cfg(test)]
    pub fn to_index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> Source {
        Self::ALL.get(i).copied().unwrap_or_default()
    }
}

/// Every candidate for the current frame, for the diagnostic panel.
#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub agent_id: u32,
    pub array_found: bool,
    pub agent: Option<Pos>,
    pub plane: Option<u32>,
    pub movement_a: Option<Pos>,
    pub movement_b: Option<Pos>,
}

impl Reading {
    pub fn pick(&self, source: Source) -> Option<Pos> {
        let p = match source {
            Source::Agent => self.agent,
            Source::MovementA => self.movement_a,
            Source::MovementB => self.movement_b,
        }?;
        p.is_sane().then_some(p)
    }
}

#[derive(Default)]
pub struct Probe {
    scanned: bool,
    /// Address of GWCA's `AgentArray` object, once the pattern has been found.
    array_addr: Option<usize>,
}

impl Probe {
    /// Reads the position candidates for `agent_id`. Scans for the agent array the first time (tens of ms).
    pub fn read(&mut self, agent_id: u32, agent_ctx: Option<usize>) -> Reading {
        if !self.scanned {
            self.scanned = true;
            self.array_addr = find_agent_array();
            tracing::info!("agent array {}", self.array_addr.map_or("not found".to_string(), |a| format!("at {a:#x}")));
        }
        let mut r = Reading { agent_id, array_found: self.array_addr.is_some(), ..Reading::default() };
        if let Some(array) = self.array_addr {
            if let Some(agent) = array_entry(array, agent_id) {
                // `Agent` stores x, y then plane at +0x74; z is a separate float earlier in the struct.
                r.agent = read_vec(agent + AGENT_POS, agent + AGENT_Z);
                r.plane = gw::read_u32(agent + AGENT_PLANE);
            }
        }
        if let Some(ctx) = agent_ctx {
            if let Some(mv) = array_entry(ctx + AGENT_CTX_MOVEMENT, agent_id) {
                r.movement_a = read_vec3(mv + MOVEMENT_VEC_A);
                r.movement_b = read_vec3(mv + MOVEMENT_VEC_B);
            }
        }
        r
    }
}

/// `array[index]` for a GWCA `Array<T*>` object at `array_addr`, with every pointer checked.
fn array_entry(array_addr: usize, index: u32) -> Option<usize> {
    let buffer = gw::read_u32(array_addr + ARRAY_BUFFER)? as usize;
    let size = gw::read_u32(array_addr + ARRAY_SIZE)?;
    if buffer == 0 || index >= size || size > 1_000_000 {
        return None;
    }
    let entry = gw::read_u32(buffer + 4 * index as usize)? as usize;
    (entry != 0).then_some(entry)
}

/// x and y at `xy_addr`, z at `z_addr`.
fn read_vec(xy_addr: usize, z_addr: usize) -> Option<Pos> {
    Some(Pos { x: gw::read_f32(xy_addr)?, y: gw::read_f32(xy_addr + 4)?, z: gw::read_f32(z_addr)? })
}

fn read_vec3(addr: usize) -> Option<Pos> {
    Some(Pos { x: gw::read_f32(addr)?, y: gw::read_f32(addr + 4)?, z: gw::read_f32(addr + 8)? })
}

// ---- finding the agent array ----

/// Scans the game's own image for GWCA's agent-array pattern and returns the array's address.
fn find_agent_array() -> Option<usize> {
    let (base, size) = main_module()?;
    let at = scan(base, size, AGENT_ARRAY_PATTERN, AGENT_ARRAY_MASK)?;
    let array = gw::read_u32(at + AGENT_ARRAY_PTR_OFFSET)? as usize;
    // Sanity check: it should look like an array (a buffer pointer and a plausible size).
    let buffer = gw::read_u32(array + ARRAY_BUFFER)?;
    let len = gw::read_u32(array + ARRAY_SIZE)?;
    (buffer != 0 && len > 0 && len < 1_000_000).then_some(array)
}

/// Base address and size of the running executable's image.
#[cfg(all(windows, target_arch = "x86"))]
pub(crate) fn main_module() -> Option<(usize, usize)> {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    let base = unsafe { GetModuleHandleW(None).ok()? }.0 as usize;
    let pe = base + gw::read_u32(base + 0x3C)? as usize; // IMAGE_DOS_HEADER.e_lfanew
    if gw::read_u32(pe)? != 0x0000_4550 {
        return None; // "PE\0\0"
    }
    let size_of_image = gw::read_u32(pe + 24 + 56)? as usize; // OptionalHeader.SizeOfImage (PE32)
    (size_of_image > 0 && size_of_image < 0x4000_0000).then_some((base, size_of_image))
}

#[cfg(not(all(windows, target_arch = "x86")))]
pub(crate) fn main_module() -> Option<(usize, usize)> {
    None
}

/// Finds `pattern` (with `mask`: 'x' = must match, anything else = wildcard) in `size` bytes at `base`,
/// reading in chunks and skipping anything unreadable. Returns the address of the match.
pub(crate) fn scan(base: usize, size: usize, pattern: &[u8], mask: &str) -> Option<usize> {
    const CHUNK: usize = 64 * 1024;
    let mut buf = vec![0u8; CHUNK + pattern.len()];
    let mut offset = 0usize;
    while offset < size {
        let want = (size - offset).min(CHUNK + pattern.len());
        // Reads that run into an unmapped page fail as a whole; fall back to a page at a time.
        if gw::read_into(base + offset, &mut buf[..want]) {
            if let Some(i) = find_in(&buf[..want], pattern, mask) {
                return Some(base + offset + i);
            }
        }
        offset += CHUNK;
    }
    None
}

/// Pure pattern search, so it can be tested without a game.
pub fn find_in(haystack: &[u8], pattern: &[u8], mask: &str) -> Option<usize> {
    let mask = mask.as_bytes();
    if pattern.is_empty() || mask.len() != pattern.len() || haystack.len() < pattern.len() {
        return None;
    }
    (0..=haystack.len() - pattern.len()).find(|&i| pattern.iter().zip(mask).enumerate().all(|(j, (p, m))| *m != b'x' || haystack[i + j] == *p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_an_exact_pattern() {
        let hay = [0u8, 1, 2, 0xFF, 0x50, 0x10, 9, 9];
        assert_eq!(find_in(&hay, b"\xFF\x50\x10", "xxx"), Some(3));
        assert_eq!(find_in(&hay, b"\xFF\x51\x10", "xxx"), None);
    }

    #[test]
    fn wildcards_match_anything() {
        let hay = [5u8, 1, 2, 3, 7];
        assert_eq!(find_in(&hay, b"\x01\x00\x03", "x?x"), Some(1));
        assert_eq!(find_in(&hay, b"\x01\x00\x04", "x?x"), None);
    }

    #[test]
    fn empty_or_mismatched_patterns_find_nothing() {
        assert_eq!(find_in(&[1, 2, 3], b"", ""), None);
        assert_eq!(find_in(&[1, 2, 3], b"\x01\x02", "x"), None);
        assert_eq!(find_in(&[1], b"\x01\x02", "xx"), None);
    }

    #[test]
    fn the_gwca_pattern_is_well_formed() {
        assert_eq!(AGENT_ARRAY_PATTERN.len(), AGENT_ARRAY_MASK.len());
    }

    #[test]
    fn sanity_rejects_garbage() {
        assert!(Pos { x: 100.0, y: -2500.5, z: 0.0 }.is_sane());
        assert!(!Pos { x: f32::NAN, y: 0.0, z: 0.0 }.is_sane());
        assert!(!Pos { x: 0.0, y: f32::INFINITY, z: 0.0 }.is_sane());
        assert!(!Pos { x: 5.0e6, y: 0.0, z: 0.0 }.is_sane());
    }

    #[test]
    fn pick_returns_only_sane_values_from_the_chosen_source() {
        let good = Pos { x: 1.0, y: 2.0, z: 3.0 };
        let bad = Pos { x: f32::NAN, y: 0.0, z: 0.0 };
        let r = Reading { agent: Some(bad), movement_a: Some(good), ..Reading::default() };
        assert_eq!(r.pick(Source::Agent), None);
        assert_eq!(r.pick(Source::MovementA), Some(good));
        assert_eq!(r.pick(Source::MovementB), None);
    }

    #[test]
    fn source_index_round_trips() {
        for s in Source::ALL {
            assert_eq!(Source::from_index(s.to_index()), s);
        }
        assert_eq!(Source::from_index(99), Source::Agent);
    }
}
