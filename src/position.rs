//! Reads the controlled character's position (x, y inside the current map) for the TIPS tracker.
//!
//! The position that works on this client is the `AgentMovement` vector at +0x64 (`Source::MovementA`): its
//! first float is always 0 and the next two are x and y (see `movement_pos`). GWCA's agent-array pattern does
//! not match this client, so the agent source finds nothing. Every read goes through `gw::read_into`
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

/// The position in an `AgentMovement` vector: the first float is unused (always 0), then x, then y.
/// Recorded in Plains of Jarin and Churrhir Fields. `z` carries the unused first float, for the debug panel.
pub fn movement_pos(v: [f32; 3]) -> Pos {
    Pos { x: v[1], y: v[2], z: v[0] }
}

/// The map's coordinate bounds `[min_x, min_y, max_x, max_y]` from the five floats at the start of the map
/// context (`[unknown, min_x, min_y, max_x, max_y]`). None if any is not finite or beyond `MAX_COORD` (the server
/// refuses those), or a min is not below its max.
pub fn bounds_from(raw: [f32; 5]) -> Option<[f32; 4]> {
    let [_, min_x, min_y, max_x, max_y] = raw;
    let ok = [min_x, min_y, max_x, max_y].iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) && min_x < max_x && min_y < max_y;
    ok.then_some([min_x, min_y, max_x, max_y])
}

/// The five floats at the start of the current map's context, as the game keeps them. None if unreadable.
pub fn read_bounds_raw() -> Option<[f32; 5]> {
    let ctx = gw::map_context_addr()?;
    let mut b = [0u8; 20];
    if !gw::read_into(ctx, &mut b) {
        return None;
    }
    Some(std::array::from_fn(|i| f32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])))
}

/// The current map's bounds `[min_x, min_y, max_x, max_y]`, or None if they can't be read or don't make sense.
pub fn read_bounds() -> Option<[f32; 4]> {
    read_bounds_raw().and_then(bounds_from)
}

/// Which memory location the tracker trusts for the position it sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Source {
    /// `Agent.pos` found through GWCA's agent array. Not found on this client.
    Agent,
    /// `AgentMovement` Vec3f at +0x64: unused, x, y. The one that works.
    #[default]
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
                r.movement_a = read_movement(mv + MOVEMENT_VEC_A);
                r.movement_b = read_movement(mv + MOVEMENT_VEC_B);
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

fn read_movement(addr: usize) -> Option<Pos> {
    Some(movement_pos([gw::read_f32(addr)?, gw::read_f32(addr + 4)?, gw::read_f32(addr + 8)?]))
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
        assert_eq!(Source::from_index(99), Source::MovementA);
    }

    #[test]
    fn movement_vector_is_unused_then_x_then_y() {
        // Recorded in Plains of Jarin: the first float is always 0, then x, then y.
        assert_eq!(movement_pos([0.0, 18353.0, 81.0]), Pos { x: 18353.0, y: 81.0, z: 0.0 });
    }

    #[test]
    fn the_movement_vector_is_the_default_source() {
        assert_eq!(Source::default(), Source::MovementA);
    }

    #[test]
    fn bounds_come_from_the_four_floats_after_the_unused_one() {
        assert_eq!(bounds_from([0.0, -18432.0, -21504.0, 21504.0, 21504.0]), Some([-18432.0, -21504.0, 21504.0, 21504.0]));
    }

    #[test]
    fn bounds_must_be_finite_with_min_below_max() {
        assert_eq!(bounds_from([0.0, 5.0, 0.0, 5.0, 10.0]), None); // minX == maxX
        assert_eq!(bounds_from([0.0, 0.0, 9.0, 10.0, 5.0]), None); // minY > maxY
        assert_eq!(bounds_from([0.0, f32::NAN, 0.0, 10.0, 10.0]), None);
        assert_eq!(bounds_from([0.0, 0.0, 0.0, f32::INFINITY, 10.0]), None);
        assert_eq!(bounds_from([0.0; 5]), None); // an unset context
    }

    #[test]
    fn bounds_the_server_would_refuse_are_not_sent() {
        // The server rejects anything past +-100000 with a 400, which would stall the run's updates.
        assert_eq!(bounds_from([0.0, -1.0e6, 0.0, 10.0, 10.0]), None);
        assert_eq!(bounds_from([0.0, 0.0, 0.0, 10.0, 100_001.0]), None);
        assert_eq!(bounds_from([0.0, -100_000.0, -100_000.0, 100_000.0, 100_000.0]), Some([-100_000.0, -100_000.0, 100_000.0, 100_000.0]));
    }
}
