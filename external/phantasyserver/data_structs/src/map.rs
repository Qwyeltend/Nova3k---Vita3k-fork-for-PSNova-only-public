use pso2packetlib::protocol::{
    models::Position,
    server::{LoadLevelPacket, ZoneSettings},
    spawn::{EventSpawnPacket, NPCSpawnPacket, ObjectSpawnPacket, TransporterSpawnPacket},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

pub type ZoneId = u32;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct MapData {
    pub map_data: LoadLevelPacket,
    pub objects: Vec<ObjectData>,
    pub events: Vec<EventData>,
    pub npcs: Vec<NPCData>,
    pub transporters: Vec<TransporterData>,
    pub luas: HashMap<String, String>,
    pub init_map: ZoneId,
    pub zones: Vec<ZoneData>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ObjectData {
    pub zone_id: ZoneId,
    pub is_active: bool,
    pub data: ObjectSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EventData {
    pub zone_id: ZoneId,
    pub is_active: bool,
    pub data: EventSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct NPCData {
    pub zone_id: ZoneId,
    pub is_active: bool,
    pub data: NPCSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct TransporterData {
    pub zone_id: ZoneId,
    pub is_active: bool,
    pub data: TransporterSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EnemySpawn {
    pub enemy_name: String,
    pub spawn_category: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ZoneData {
    pub name: String,
    pub is_special_zone: bool,
    pub zone_id: ZoneId,
    pub settings: ZoneSettings,
    pub default_location: Position,
    pub enemies: Vec<EnemySpawn>,
    pub chunks: Vec<ZoneChunk>,
    /// Nova3K: progression of a generated field zone (gate or boss at the end)
    pub nova3k: Option<Nova3kZone>,
}

/// Nova3K: how a generated field zone ends. The gate to the next zone (or, with `boss`, the zone's boss)
/// appears next to the player when the player reaches the goal of the layout (`goal`, x/z, within
/// `goal_radius`) or after `gate_kills` enemy kills in the zone, whichever comes first.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Nova3kZone {
    pub goal: Option<[f32; 2]>,
    pub goal_radius: f32,
    pub gate_kills: u32,
    pub boss: Option<String>,
    /// Field shapes (client area templates) to choose from: one is picked at random when the quest map is
    /// created, so the field differs from run to run. Empty = the zone's own settings.
    pub variants: Vec<Nova3kVariant>,
}

/// Nova3K: one field shape: client area template (`args`), start point (x, y, z, rotation in degrees), goal
/// (x, z) and fallback start points (x, y, z) for the rescue.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Nova3kVariant {
    pub args: u32,
    pub start: [f32; 4],
    pub goal: Option<[f32; 2]>,
    pub points: Vec<[f32; 3]>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub enum EnemySpawnType {
    #[default]
    Disabled,
    Automatic {
        min: u32,
        max: u32,
    },
    AutomaticWithRespawn {
        min: u32,
        max: u32,
        respawn_time: Duration,
    },
    Manual,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ChunkReveal {
    pub row: u32,
    pub column: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ZoneChunk {
    pub zone_id: ZoneId,
    pub chunk_id: u32,
    pub enemy_spawn_type: EnemySpawnType,
    pub enemy_spawn_points: Vec<Position>,
    pub reveals: Vec<ChunkReveal>,
}
