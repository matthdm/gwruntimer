
use std::time::Duration;


#[repr(C)]
pub struct GamePos {
    x: f32,
    y: f32,
    plane: u32
}



#[repr(C)]
pub struct AgentContext {
    pad1: [u8; 0x1AC],
    cli_instance_up_time: u32
}


#[repr(C)]
pub struct MapContext;



#[repr(C)]
pub struct CharControlledCharacterData {
    pad_0: [u8; 0x14],
    pub agent_id: u32,
    pad_18: [u8; 0x50],
    pub move_timestamp: u32
}


#[repr(C)]
pub struct CharContext {
    pad_0: [u8; 0x680],
    controlled: *const CharControlledCharacterData
}

#[repr(C)]
pub struct CinematicContext;

#[repr(C)]
 pub struct MissionCharData {
    pub character_uuid: [u8; 16],
    character_name: [u16; 20]
 }


#[repr(C)]
 pub struct MissionMapData {
    pub load_state: u32,
    unk: u32,
    pub map_id: u32,
    pub is_explorable: u32
 }

#[repr(C)]
pub struct MissionContext {
    pad_0: [u8; 0x64],
    pub chardata: MissionCharData,
    pad_9c: [u8; 0xF4],
    pub mapdata: MissionMapData
}


#[repr(C)]
pub struct PartyContext;

#[repr(C)]
pub struct GameContext {
    rand      : *const (),
    time      : *const (),
pub agent     : *const AgentContext,
    event     : *const (),
    unk       : [u8; 4],
pub map       : *const MapContext,
    text      : *const (),
    unk2      : [u8; 8],
    prop      : *const (),
    account   : *const (),
pub character : *const CharContext,
pub cinematic : *const CinematicContext,
    unk3      : [u8; 4],
    gadget    : *const (),
    guild     : *const (),
    item      : *const (),
pub mission   : *const MissionContext,
    unk4      : [u8; 4],
pub party     : *const PartyContext,
    unk5      : [u8; 8],
    trade     : *const ()
}

#[cfg(all(windows, target_arch = "x86"))]
#[inline]
// unsafe outside of gw render thread
 pub fn get_context_tls() -> *const GameContext {
    use core::arch::asm;
    let mut ctx :usize = 0;

    unsafe {
        asm!(
            "mov eax, fs:[0x2C]",
            "mov eax, [eax]",
            "mov eax, [eax+8]",
            "mov {}, eax",
            out(reg) ctx,
            options(nostack, readonly, preserves_flags),
        );
    }
    ctx as *const GameContext
 }


pub fn get_instance_uptime() -> Option<& 'static u32> {
    unsafe {
        let pctx = get_context_tls().as_ref()?;
        let agctx = pctx.agent.as_ref()?;
        Some(&agctx.cli_instance_up_time)
    }
}





 impl MissionMapData {

    pub fn current() -> Option<& 'static Self> {
        unsafe {
            let pctx = get_context_tls().as_ref()?;
            let msctx = pctx.mission.as_ref()?;
            Some(&msctx.mapdata)
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.load_state == 0xC8
    }

 }

 impl MissionCharData {

    pub fn current() -> Option<& 'static Self> {
        unsafe {
            let pctx = get_context_tls().as_ref()?;
            let msctx = pctx.mission.as_ref()?;
            Some(&msctx.chardata)
        }
    }

    pub fn name(&self) -> String {
        String::from_utf16_lossy(&self.character_name)
    }

 }


 impl CharControlledCharacterData {

    // 'a lifetime is lifetime of controlled character presence within a zone
    // TODO: confirm controlled char changes on load to confirm lifetime
    pub fn current<'a>() -> Option<& 'a Self> {
        unsafe {
            let pctx: &GameContext = get_context_tls().as_ref()?;
            let charctx = pctx.character.as_ref()?;
            Some(charctx.controlled.as_ref()?)
        }
    }

    pub fn is_moving(&self) -> bool {
        self.move_timestamp > 0
    }
 }

