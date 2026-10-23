use crate::map::{MapData, ZoneId};
use pso2packetlib::protocol::{
    questlist::{Quest, QuestDifficulty},
    spawn::EnemySpawnPacket,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct QuestData {
    pub definition: Quest,
    pub difficulties: QuestDifficulty,
    pub map: MapData,
    pub enemies: Vec<EnemyData>,
    pub immediate_move: bool,
    /// Nova3K: shown at the quest counter for every character, without an unlock
    /// (quests generated from the client data for the offline mode)
    pub nova3k_always_unlocked: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EnemyData {
    pub difficulty: u16,
    pub mapid: ZoneId,
    pub data: EnemySpawnPacket,
}
