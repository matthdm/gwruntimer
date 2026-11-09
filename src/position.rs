//! Reads the controlled character's position (x, y inside the current map) and the map's coordinate bounds, for the
//! TIPS tracker.
//!
//! The position is in the game's `AgentMovement` vectors (+0x64, and the same layout at +0x74): the first float is
//! unused (always 0), then x, then y. Every read goes through `gw::read_into` (ReadProcessMemory on our own process),
//! which fails safely instead of faulting. It never reads the character's name, account, party or anything else; only
//! coordinates.

use crate::gw;

// GWCA `Array<T>`: { buffer, allocated, size, param } (16 bytes).
const ARRAY_BUFFER: usize = 0;
const ARRAY_SIZE: usize = 8;

// GWCA `AgentContext`: `Array<AgentMovement*> agent_movement` at +0xE8.
const AGENT_CTX_MOVEMENT: usize = 0xE8;
// GWCA `AgentMovement`: two Vec3f of the same layout (unused, x, y), at +0x64 and +0x74.
const MOVEMENT_VEC_A: usize = 0x64;
const MOVEMENT_VEC_B: usize = 0x74;

/// Anything beyond this is not a coordinate inside a map.
pub const MAX_COORD: f32 = 100_000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pos {
    pub x: f32,
    pub y: f32,
    /// The unused first float of the movement vector (kept for the debug panel).
    pub z: f32,
}

impl Pos {
    /// A usable coordinate: finite and in range.
    pub fn is_sane(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.x.abs() <= MAX_COORD && self.y.abs() <= MAX_COORD
    }
}

/// The position in an `AgentMovement` vector: the first float is unused (always 0), then x, then y.
/// Recorded in Plains of Jarin and Churrhir Fields.
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

/// The current map's bounds `[min_x, min_y, max_x, max_y]`, or None if they can't be read or don't make sense.
pub fn read_bounds() -> Option<[f32; 4]> {
    let ctx = gw::map_context_addr()?;
    let mut b = [0u8; 20];
    if !gw::read_into(ctx, &mut b) {
        return None;
    }
    bounds_from(std::array::from_fn(|i| f32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])))
}

/// Which movement vector the tracker trusts for the position it sends. Both read the same on this client.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Source {
    /// `AgentMovement` Vec3f at +0x64.
    #[default]
    MovementA,
    /// `AgentMovement` Vec3f at +0x74.
    MovementB,
}

impl Source {
    pub const ALL: [Source; 2] = [Source::MovementA, Source::MovementB];

    pub fn label(self) -> &'static str {
        match self {
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

/// Both movement vectors for the current frame, for the tracker and the debug panel.
#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub agent_id: u32,
    pub movement_a: Option<Pos>,
    pub movement_b: Option<Pos>,
}

impl Reading {
    pub fn pick(&self, source: Source) -> Option<Pos> {
        let p = match source {
            Source::MovementA => self.movement_a,
            Source::MovementB => self.movement_b,
        }?;
        p.is_sane().then_some(p)
    }
}

/// Reads both movement vectors for `agent_id` from the game's agent context.
pub fn read(agent_id: u32, agent_ctx: Option<usize>) -> Reading {
    let mut r = Reading { agent_id, ..Reading::default() };
    if let Some(ctx) = agent_ctx {
        if let Some(mv) = array_entry(ctx + AGENT_CTX_MOVEMENT, agent_id) {
            r.movement_a = read_movement(mv + MOVEMENT_VEC_A);
            r.movement_b = read_movement(mv + MOVEMENT_VEC_B);
        }
    }
    r
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

fn read_movement(addr: usize) -> Option<Pos> {
    Some(movement_pos([gw::read_f32(addr)?, gw::read_f32(addr + 4)?, gw::read_f32(addr + 8)?]))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let r = Reading { movement_a: Some(good), movement_b: Some(bad), ..Reading::default() };
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
    fn only_the_two_movement_vectors_are_sources() {
        // The agent-array source never worked on this client and was removed.
        assert_eq!(Source::ALL, [Source::MovementA, Source::MovementB]);
        assert_eq!(Source::from_index(0), Source::MovementA);
        assert_eq!(Source::from_index(1), Source::MovementB);
        assert_eq!(Source::from_index(2), Source::MovementA, "past the list means the default");
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
