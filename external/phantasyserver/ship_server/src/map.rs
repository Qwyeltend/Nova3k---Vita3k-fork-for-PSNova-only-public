use crate::{
    BlockData, Error, User,
    battle_stats::{BattleResult, EnemyStats},
    mutex::{Mutex, MutexGuard},
};
use data_structs::map::{EventData, MapData, NPCData, ObjectData, TransporterData, ZoneData};
use mlua::{Lua, LuaSerdeExt, StdLib};
use pso2packetlib::protocol::{
    self, ObjectHeader, ObjectType, Packet, PacketType,
    flag::{CutsceneEndPacket, SkitItemAddRequestPacket},
    models::Position,
    objects::EnemyActionPacket,
    playerstatus::{DealDamagePacket, GainedEXPPacket, SetPlayerIDPacket},
    questlist::{MinimapRevealPacket, RevealedRegions},
    server::{LoadLevelPacket, MapTransferPacket},
    spawn::{CharacterSpawnPacket, CharacterSpawnType, ObjectSpawnPacket},
    symbolart::{ReceiveSymbolArtPacket, SendSymbolArtPacket},
};
use rand::{distr::Uniform, prelude::Distribution, seq::{IteratorRandom, SliceRandom}};
use std::{
    collections::HashMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicU32, Ordering},
    },
    time::Instant,
};

type ZoneId = u32;
type PlayerId = u32;

#[derive(Clone)]
struct MapPlayer {
    player_id: PlayerId,
    user: Weak<Mutex<User>>,
}

#[derive(Clone)]
struct OwnedMapPlayer {
    player_id: PlayerId,
    user: Arc<Mutex<User>>,
}

struct Zone {
    zone_pos: usize,
    lua: Arc<parking_lot::Mutex<LuaState>>,
    srv_zone_id: ZoneId,
    obj: ObjectHeader,
    players: Vec<MapPlayer>,
    enemies: Vec<(u32, EnemyStats)>,
    chunk_spawns: Vec<(u32, Instant)>,
    minimap_status: RevealedRegions,
    data: ZoneData,
    objects: Objects,
    /// Nova3K: boss zone (zone named "boss"): ids of the bosses spawned when the first player arrived
    /// (empty = not spawned yet), and whether the quest exit was opened after they died.
    nova3k_boss_ids: Vec<u32>,
    nova3k_boss_done: bool,
    nova3k_level: u32,
    /// Nova3K: enemy id -> (last x, y, z, times seen far below its spawn point)
    nova3k_enemy_track: HashMap<u32, (f32, f32, f32, u32)>,
    /// Nova3K: enemies killed in this zone, whether its gate (or end boss) appeared, the gate object id and
    /// the spot of its backup telepipe
    nova3k_kills: u32,
    nova3k_gate_open: bool,
    nova3k_gate_id: Option<u32>,
    nova3k_portal: Option<Position>,
    /// Nova3K: index of the spawn point used as the last rescue start
    nova3k_start_try: usize,
}

struct Objects {
    objects: Vec<ObjectData>,
    events: Vec<EventData>,
    npcs: Vec<NPCData>,
    transporters: Vec<TransporterData>,
}

pub enum MapType {
    Lobby,
    QuestMap,
}

struct LuaState {
    lua: Lua,
    // fighting with async recursion
    to_move: Vec<(PlayerId, String)>,
    to_lobby_move: Vec<PlayerId>,
    procs: HashMap<String, String>,
}

pub struct Map {
    // lua is not `Send` so i've put it in a mutex
    // this mutex shouldn't block, because `Map` is under a mutex itself.
    lua: Arc<parking_lot::Mutex<LuaState>>,
    zones: Box<[Zone]>,
    player_cache: Vec<(u32, usize)>,
    data: MapData,
    max_id: u32,
    block_data: Option<Arc<BlockData>>,
    enemy_level: u32,
    map_type: MapType,
    quest_obj: ObjectHeader,
}
impl Map {
    pub fn new_from_data(mut data: MapData, map_obj_id: &AtomicU32) -> Result<Self, Error> {
        // will be increased as needed
        let lua_libs = StdLib::NONE;
        let lua = Arc::new(parking_lot::Mutex::new(LuaState {
            lua: Lua::new_with(lua_libs, mlua::LuaOptions::default())?,
            to_move: vec![],
            to_lobby_move: vec![],
            procs: HashMap::new(),
        }));
        let map_obj = ObjectHeader {
            id: map_obj_id.fetch_add(1, Ordering::Relaxed),
            entity_type: ObjectType::Map,
            ..Default::default()
        };
        data.map_data.map_object = map_obj;
        nova3k_pick_variants(&mut data);
        data.map_data.settings = data.zones[0].settings.clone();
        data.map_data.other_settings.clear();
        let mut zones = vec![];
        for (i, zone) in std::mem::take(&mut data.zones).into_iter().enumerate() {
            if !zone.is_special_zone {
                data.map_data.other_settings.push(zone.settings.clone());
            }
            let objects = data
                .objects
                .iter()
                .filter(|o| o.zone_id == zone.zone_id)
                .cloned()
                .collect();
            let events = data
                .events
                .iter()
                .filter(|o| o.zone_id == zone.zone_id)
                .cloned()
                .collect();
            let npcs = data
                .npcs
                .iter()
                .filter(|o| o.zone_id == zone.zone_id)
                .cloned()
                .collect();
            let transporters = data
                .transporters
                .iter()
                .filter(|o| o.zone_id == zone.zone_id)
                .cloned()
                .collect();
            zones.push(Zone {
                zone_pos: i,
                lua: lua.clone(),
                srv_zone_id: zone.zone_id,
                players: vec![],
                obj: ObjectHeader {
                    id: map_obj_id.fetch_add(1, Ordering::Relaxed),
                    entity_type: ObjectType::Map,
                    ..Default::default()
                },
                enemies: vec![],
                chunk_spawns: vec![],
                minimap_status: Default::default(),
                data: zone,
                objects: Objects {
                    objects,
                    events,
                    npcs,
                    transporters,
                },
                nova3k_boss_ids: vec![],
                nova3k_boss_done: false,
                nova3k_level: 1,
                nova3k_enemy_track: HashMap::new(),
                nova3k_kills: 0,
                nova3k_gate_open: false,
                nova3k_gate_id: None,
                nova3k_portal: None,
                nova3k_start_try: 0,
            });
        }
        let mut map = Self {
            lua: lua.clone(),
            zones: zones.into(),
            player_cache: vec![],
            data,
            max_id: 0,
            block_data: None,
            enemy_level: 0,
            map_type: MapType::QuestMap,
            quest_obj: ObjectHeader {
                entity_type: ObjectType::Quest,
                ..Default::default()
            },
        };
        map.init_lua()?;
        map.find_max_id();
        log::trace!("Map {} created", map_obj.id);
        Ok(map)
    }
    pub const fn set_map_type(&mut self, map_type: MapType) {
        self.map_type = map_type;
    }
    pub fn set_block_data(&mut self, data: Arc<BlockData>) {
        self.block_data = Some(data);
    }
    pub const fn set_enemy_level(&mut self, level: u32) {
        self.enemy_level = level;
    }
    pub const fn set_quest_obj(&mut self, obj: ObjectHeader) {
        self.quest_obj = obj;
    }
    fn find_max_id(&mut self) {
        let obj_max = self
            .data
            .objects
            .iter()
            .map(|o| o.data.object.id)
            .max()
            .unwrap_or(0);
        let npc_max = self
            .data
            .npcs
            .iter()
            .map(|o| o.data.object.id)
            .max()
            .unwrap_or(0);
        let event_max = self
            .data
            .events
            .iter()
            .map(|o| o.data.object.id)
            .max()
            .unwrap_or(0);
        let transporter_max = self
            .data
            .transporters
            .iter()
            .map(|o| o.data.object.id)
            .max()
            .unwrap_or(0);
        self.max_id = obj_max.max(npc_max).max(event_max).max(transporter_max) + 1;
    }
    fn init_lua(&mut self) -> Result<(), Error> {
        // default object handler
        for object in self.data.objects.iter() {
            let name: &str = &object.data.name;
            if self.data.luas.contains_key(name) {
                continue;
            }
            self.data.luas.insert(
                name.to_owned(),
                "if call_type == \"interaction\" then
                    print(packet.object1.id, packet.action)
                elseif call_type == \"to_vita\" then
	                for i=1,size,2 do
		                if data[i] > 50 and data[i] < 80 then
			                data[i] = data[i] - 1
		                end
	                end
                end"
                .into(),
            );
        }
        // default npc handler
        for npc in self.data.npcs.iter() {
            let name: &str = &npc.data.name;
            if self.data.luas.contains_key(name) {
                continue;
            }
            self.data.luas.insert(
                name.to_owned(),
                "if call_type == \"interaction\" then
                    if packet.action == \"READY\" then
                        local ready_data = {}; 
                        local packet_data = {};
                        packet_data.attribute = \"FavsNeutral\";
                        packet_data.receiver = packet.object3;
                        packet_data.target = packet.object1;
                        packet_data.object3 = packet.object1;
                        ready_data.SetTag = packet_data; 
                        send(sender, ready_data);
                        ready_data.SetTag.attribute = \"AP\";
                        send(sender, ready_data);
                    else
                        print(packet.object1.id, packet.action);
                    end
                end"
                .into(),
            );
        }
        self.lua.lock().procs = std::mem::take(&mut self.data.luas);
        Ok(())
    }

    pub async fn init_add_player(&mut self, new_player: Arc<Mutex<User>>) -> Result<(), Error> {
        let mut np_lock = new_player.lock().await;
        let map_data = self.data.map_data.clone();
        let obj = np_lock.create_object_header();
        let Some(p) = &np_lock.get_current_party() else {
            return Err(Error::InvalidInput("init_add_player"));
        };
        let party_obj = p.read().await.get_obj();
        np_lock
            .send_packet(&Packet::LoadLevel(LoadLevelPacket {
                host: obj,
                party: party_obj,
                world_obj: ObjectHeader {
                    id: map_data.settings.world_id,
                    entity_type: ObjectType::World,
                    ..Default::default()
                },
                quest: self.quest_obj,
                ..map_data
            }))
            .await?;
        drop(np_lock);
        self.add_player(new_player, self.data.init_map).await
    }

    pub async fn move_player_named(&mut self, id: PlayerId, name: &str) -> Result<(), Error> {
        let Some(zone) = self.zones.iter().find(|z| z.data.name == name) else {
            return Err(Error::InvalidInput("move_player_named"));
        };
        self.move_player(id, zone.srv_zone_id).await
    }

    async fn move_player(&mut self, id: PlayerId, srv_zone_id: ZoneId) -> Result<(), Error> {
        let Some(player) = self.remove_player(id).await else {
            return Err(Error::NoUserInMap(id, self.data.map_data.unk7.to_string()));
        };
        let Some(zone) = self.zones.iter().find(|z| z.srv_zone_id == srv_zone_id) else {
            return Err(Error::NoMapInMapSet(
                srv_zone_id,
                self.data.map_data.unk7.to_string(),
            ));
        };
        let mut lock = player.lock().await;
        let pid = lock.get_user_id();
        lock.send_packet(&Packet::MapTransfer(MapTransferPacket {
            map: zone.obj,
            target: ObjectHeader {
                id: pid,
                entity_type: ObjectType::Player,
                ..Default::default()
            },
            settings: zone.data.settings.clone(),
        }))
        .await?;
        drop(lock);
        self.add_player(player, srv_zone_id).await
    }

    pub async fn move_to_lobby(&mut self, id: PlayerId) -> Result<(), Error> {
        if matches!(self.map_type, MapType::Lobby) {
            return Ok(());
        }
        let Some(player) = self.remove_player(id).await else {
            return Err(Error::NoUserInMap(id, self.data.map_data.unk7.to_string()));
        };
        let lobby = player.lock().await.get_blockdata().lobby.clone();
        player.lock().await.set_map(lobby.clone());
        let mut lock = lobby.lock().await;
        lock.init_add_player(player).await
    }

    async fn add_player(
        &mut self,
        new_player: Arc<Mutex<User>>,
        srv_zone_id: ZoneId,
    ) -> Result<(), Error> {
        let Some((pos, zone)) = self
            .zones
            .iter_mut()
            .enumerate()
            .find(|(_, z)| z.srv_zone_id == srv_zone_id)
        else {
            return Err(Error::InvalidInput("add_player"));
        };
        let p_id = new_player.lock().await.get_user_id();
        zone.add_player(new_player.clone()).await?;
        self.player_cache.push((p_id, pos));
        self.nova3k_spawn_boss(pos).await;
        Ok(())
    }

    /// Nova3K: a zone named "boss" spawns its enemies (the boss) once, when the first player arrives,
    /// at the zone's first chunk spawn points (the arena center). Level: quest level + 2.
    async fn nova3k_spawn_boss(&mut self, zone_pos: usize) {
        let zone = &self.zones[zone_pos];
        if zone.data.name != "boss" || !zone.nova3k_boss_ids.is_empty() || zone.nova3k_boss_done {
            return;
        }
        let Some(block_data) = self.block_data.to_owned() else {
            return;
        };
        let points: Vec<Position> = zone
            .data
            .chunks
            .first()
            .map(|c| c.enemy_spawn_points.clone())
            .unwrap_or_default();
        let names: Vec<String> = zone.data.enemies.iter().map(|e| e.enemy_name.clone()).collect();
        let fallback = zone.data.default_location;
        let level = self.enemy_level + 2;
        self.zones[zone_pos].nova3k_level = self.enemy_level.max(1);
        for (i, name) in names.iter().enumerate() {
            let pos = points.get(i % points.len().max(1)).copied().unwrap_or(fallback);
            let id = self.max_id + 1;
            match self.zones[zone_pos]
                .nova3k_spawn_enemy(&block_data, &mut self.max_id, level, name, pos, 20)
                .await
            {
                Ok(()) => {
                    log::info!("Nova3K boss: spawned {name} (id {id}, level {level})");
                    self.zones[zone_pos].nova3k_boss_ids.push(id);
                }
                Err(e) => log::info!("Nova3K boss: could not spawn {name}: {e}"),
            }
        }
    }

    /// Nova3K: movement of an enemy reported by the client. An enemy far below its spawn point (fell
    /// through the level: it falls, is put back, falls again...) is removed and spawned again on the
    /// player's trail next to the player.
    pub async fn nova3k_enemy_moved(
        &mut self,
        zone_pos: usize,
        player_id: PlayerId,
        enemy_id: u32,
        pos: (Option<half::f16>, Option<half::f16>, Option<half::f16>),
    ) {
        let zone = &mut self.zones[zone_pos];
        let Some((idx, spawn_y, name, level)) = zone
            .enemies
            .iter()
            .position(|(id, _)| *id == enemy_id)
            .map(|i| {
                let e = &zone.enemies[i].1;
                (i, e.nova3k_pos().pos_y.to_f32(), e.nova3k_name().to_string(), e.nova3k_level())
            })
        else {
            return;
        };
        let t = zone.nova3k_enemy_track.entry(enemy_id).or_insert((0.0, spawn_y, 0.0, 0));
        if let Some(v) = pos.0 { t.0 = v.to_f32(); }
        if let Some(v) = pos.1 { t.1 = v.to_f32(); }
        if let Some(v) = pos.2 { t.2 = v.to_f32(); }
        if t.1 < spawn_y - 12.0 {
            t.3 += 1;
        }
        if t.3 < 3 {
            return;
        }
        // fell out of the level: despawn it and spawn the same enemy next to the player
        zone.nova3k_enemy_track.remove(&enemy_id);
        zone.enemies.remove(idx);
        let despawn = Packet::DespawnObject(protocol::objects::DespawnObjectPacket {
            player: ObjectHeader::default(),
            item: ObjectHeader { id: enemy_id, entity_type: ObjectType::Object, ..Default::default() },
        });
        let mut target = None;
        for p in zone.players.iter().filter_map(|p| p.user.upgrade()) {
            let mut l = p.lock().await;
            let mut d = despawn.clone();
            if let Packet::DespawnObject(data) = &mut d {
                data.player = l.create_object_header();
            }
            let _ = l.try_send_packet(&d);
            if l.get_user_id() == player_id {
                target = Some((l.position, l.nova3k_trail.clone()));
            }
        }
        let Some((ppos, trail)) = target else { return };
        let spot = nova3k_trail_point(&trail, &ppos).unwrap_or(ppos);
        let Some(block_data) = self.block_data.to_owned() else { return };
        log::info!("Nova3K enemy rescue: {name} (id {enemy_id}) fell below y {spawn_y:.1} -> respawned next to the player");
        let _ = self.zones[zone_pos]
            .spawn_enemy(&block_data, &mut self.max_id, level, &name, spot)
            .await;
    }

    /// Nova3K: the client uses a telepipe (oa_telepipe_start) by sending ReturnToCampship. The generated
    /// quests put a second telepipe (object id ending in 5) at the far end of every field: used there, it
    /// leads to the next zone instead of the campship (the real zone gate, oa_transfer_multi, is not drawn
    /// yet). Any other telepipe: back to the campship with full HP (after the boss: quest clear).
    pub async fn nova3k_telepipe(&mut self, id: PlayerId) -> Result<(), Error> {
        let Some(zone_pos) = self.find_player(id) else {
            return Ok(());
        };
        let user = self.zones[zone_pos]
            .players
            .iter()
            .find(|p| p.player_id == id)
            .and_then(|p| p.user.upgrade());
        let Some(user) = user else {
            return Ok(());
        };
        let pos = user.lock().await.position;
        let zone = &self.zones[zone_pos];
        let near_portal = zone.nova3k_portal.is_some_and(|p| {
            let dx = p.pos_x.to_f32() - pos.pos_x.to_f32();
            let dz = p.pos_z.to_f32() - pos.pos_z.to_f32();
            dx * dx + dz * dz < 15.0 * 15.0
        });
        let near = near_portal || zone.objects.objects.iter().any(|o| {
            let name: &str = &o.data.name;
            if name != "oa_telepipe_start" || o.data.object.id % 10 != 5 {
                return false;
            }
            let dx = o.data.position.pos_x.to_f32() - pos.pos_x.to_f32();
            let dz = o.data.position.pos_z.to_f32() - pos.pos_z.to_f32();
            dx * dx + dz * dz < 15.0 * 15.0
        });
        if near {
            log::info!("Nova3K gate: telepipe at the end of the field -> next zone");
            return self.nova3k_next_zone(id, zone_pos).await;
        }
        let clear = zone.nova3k_boss_done;
        {
            let mut u = user.lock().await;
            u.get_stats_mut().nova3k_full_heal();
            if clear {
                log::info!("Nova3K quest: cleared (telepipe after the boss)");
                let _ = u.send_system_msg("Quest clear!").await;
                // the result screen tells the client the quest is complete (no "abandon the quest?" later);
                // the reward itself was already paid when the boss died
                let reward = 100 + 25 * zone.nova3k_level;
                let kills = zone.nova3k_kills;
                let result = Packet::QuestResult(protocol::questlist::QuestResultPacket {
                    enemy_kills: protocol::questlist::QuestResultEntry { value: kills, ..Default::default() },
                    rank: protocol::questlist::QuestResultRank::S,
                    meseta_earned: reward,
                    ..Default::default()
                });
                let _ = u.try_send_packet(&result);
                u.nova3k_quest_cleared = true;
            }
        }
        drop(user);
        if self.move_player_named(id, "campship").await.is_err() {
            log::info!("Nova3K move: no campship zone in this map, going to the lobby");
            self.move_to_lobby(id).await?;
        }
        Ok(())
    }

    /// Nova3K: end of a generated field zone (ZoneData::nova3k). When the player reaches the layout's goal or
    /// has killed enough enemies in the zone, the gate to the next zone appears next to the player (on the
    /// walked trail, so on the ground) - or, in the last field of a quest without a boss arena, the zone's boss.
    async fn nova3k_progress(&mut self, zone_pos: usize) {
        self.nova3k_progress_inner(zone_pos, false).await;
    }

    async fn nova3k_progress_inner(&mut self, zone_pos: usize, force: bool) {
        let zone = &self.zones[zone_pos];
        let Some(cfg) = zone.data.nova3k.clone() else {
            return;
        };
        if zone.nova3k_gate_open {
            return;
        }
        let Some(user) = zone.players.iter().filter_map(|p| p.user.upgrade()).next() else {
            return;
        };
        let (pos, trail) = {
            let l = user.lock().await;
            (l.position, l.nova3k_trail.clone())
        };
        let near_goal = cfg.goal.is_some_and(|g| {
            let dx = g[0] - pos.pos_x.to_f32();
            let dz = g[1] - pos.pos_z.to_f32();
            let r = if cfg.goal_radius > 0.0 { cfg.goal_radius } else { 45.0 };
            dx * dx + dz * dz < r * r
        });
        let kills = zone.nova3k_kills;
        // also open after exploring: 8 different map cells entered (small maps may not hold enough enemies)
        let explored = zone.chunk_spawns.len() >= 8;
        let enough = (cfg.gate_kills > 0 && kills >= cfg.gate_kills) || explored;
        if !near_goal && !enough && !force {
            return;
        }
        self.zones[zone_pos].nova3k_gate_open = true;
        let spot = nova3k_trail_point(&trail, &pos).unwrap_or(pos);
        let world = self.zones[zone_pos].data.settings.world_id as u16;
        let why = if force {
            "!next"
        } else if near_goal {
            "goal reached"
        } else if explored {
            "area explored"
        } else {
            "enemies defeated"
        };
        if let Some(boss) = cfg.boss.clone() {
            let Some(block_data) = self.block_data.to_owned() else {
                return;
            };
            let level = self.enemy_level + 2;
            self.zones[zone_pos].nova3k_level = self.enemy_level.max(1);
            let id = self.max_id + 1;
            match self.zones[zone_pos].nova3k_spawn_enemy(&block_data, &mut self.max_id, level, &boss, spot, 20).await {
                Ok(()) => {
                    log::info!("Nova3K progress: {why} ({kills} kills) -> boss {boss} (id {id}, level {level})");
                    self.zones[zone_pos].nova3k_boss_ids.push(id);
                    self.zones[zone_pos].nova3k_message("A powerful enemy appears!").await;
                }
                Err(e) => log::info!("Nova3K progress: could not spawn the boss {boss}: {e}"),
            }
            return;
        }
        // gate: oa_transfer_multi of quest 50570 (Vita layout: keys 51..79 lowered by one) and a telepipe next to
        // it that also leads to the next zone (backup while the gate's own answer is unknown)
        let gate_id = 70000 + zone_pos as u32 * 10 + 1;
        let gate = ObjectSpawnPacket {
            object: ObjectHeader { id: gate_id, entity_type: ObjectType::Object, map_id: world, ..Default::default() },
            position: spot,
            name: String::from("oa_transfer_multi").into(),
            unk2: [17, 0, 0, 0, 0],
            flags: 4,
            data: vec![
                68120, 0, 2, 0, 0, 0, 0, 1, 57, 0, 54, 1, 56, 1, 59, 2, 66, 1, 74, 1, 67, 4294967295, 65, 0, 70, 2, 73, 1, 70,
                1082130432,
            ]
            .into(),
            ..Default::default()
        };
        // second gate kind: oa_transporter_universal of quest 50570 (in that quest it stands exactly on the client's
        // set position of a transporter) - maybe this one is drawn as the gate ring
        let mut ring_pos = spot;
        ring_pos.pos_x = half::f16::from_f32(spot.pos_x.to_f32() - 4.0);
        let ring = ObjectSpawnPacket {
            object: ObjectHeader { id: gate_id + 1, entity_type: ObjectType::Object, map_id: world, ..Default::default() },
            position: ring_pos,
            name: String::from("oa_transporter_universal").into(),
            unk2: [17, 0, 0, 0, 0],
            flags: 4,
            data: vec![1816, 6488064, 0, 0, 0, 0, 256, 7602288, 57, 4294967295, 54, 2, 56, 1, 59, 0, 69, 0, 27, 0, 66, 1].into(),
            ..Default::default()
        };
        let mut pipe_pos = spot;
        pipe_pos.pos_x = half::f16::from_f32(spot.pos_x.to_f32() + 4.0);
        let pipe = ObjectSpawnPacket {
            object: ObjectHeader { id: gate_id + 4, entity_type: ObjectType::Object, map_id: world, ..Default::default() },
            position: pipe_pos,
            name: String::from("oa_telepipe_start").into(),
            unk2: [16, 0, 0, 0, 0],
            flags: 4,
            data: vec![1560, 15795201, 2, 0, 0, 0, 256, 4, 57, 4294967295, 54, 1, 56, 0, 65, 1, 66, 0, 70, 1, 2, 0, 0, 0].into(),
            ..Default::default()
        };
        let zone = &mut self.zones[zone_pos];
        zone.nova3k_gate_id = Some(gate_id);
        zone.nova3k_portal = Some(pipe_pos);
        log::info!(
            "Nova3K progress: {why} ({kills} kills) -> gate at {:.1} {:.1} {:.1}",
            spot.pos_x.to_f32(),
            spot.pos_y.to_f32(),
            spot.pos_z.to_f32()
        );
        let (g, r, p) = (Packet::ObjectSpawn(gate), Packet::ObjectSpawn(ring), Packet::ObjectSpawn(pipe));
        exec_users(&zone.players, |_, mut player| {
            let _ = player.try_send_packet(&g);
            let _ = player.try_send_packet(&r);
            let _ = player.try_send_packet(&p);
        })
        .await;
        zone.nova3k_message("The gate to the next area is open!").await;
    }

    /// Nova3K: the player fell through the level (far below the start point) or used `!start`: the zone is
    /// loaded again with a new start point - the last spot of the walked trail, or else the next spawn point of
    /// the layout (the computed start of a generated field can be inside the ground or on a closed-off island).
    pub async fn nova3k_rescue(&mut self, id: PlayerId, why: &str) -> Result<(), Error> {
        let Some(zone_pos) = self.find_player(id) else {
            return Ok(());
        };
        let user = self.zones[zone_pos].players.iter().find(|p| p.player_id == id).and_then(|p| p.user.upgrade());
        let Some(user) = user else {
            return Ok(());
        };
        let trail = user.lock().await.nova3k_trail.clone();
        drop(user);
        let zone = &mut self.zones[zone_pos];
        let next = if why == "fall" && trail.len() >= 2 {
            Some(trail[trail.len() - 2])
        } else {
            let points: Vec<Position> = zone.data.chunks.first().map(|c| c.enemy_spawn_points.clone()).unwrap_or_default();
            if points.is_empty() {
                None
            } else {
                zone.nova3k_start_try = (zone.nova3k_start_try + 1) % points.len();
                Some(points[zone.nova3k_start_try])
            }
        };
        let Some(mut next) = next else {
            return Ok(());
        };
        next.pos_y = half::f16::from_f32(next.pos_y.to_f32() + 1.0);
        zone.data.default_location = next;
        log::info!(
            "Nova3K rescue ({why}): zone {} reloaded at {:.1} {:.1} {:.1}",
            zone.data.name,
            next.pos_x.to_f32(),
            next.pos_y.to_f32(),
            next.pos_z.to_f32()
        );
        let srv = zone.srv_zone_id;
        self.move_player(id, srv).await
    }

    /// Nova3K: `!next` chat command - the next zone of the quest (test help while zone gates are unproven).
    pub async fn nova3k_next_zone(&mut self, id: PlayerId, zone_pos: usize) -> Result<(), Error> {
        // a generated field whose gate (or end boss) did not appear yet: `!next` makes it appear first
        let zone = &self.zones[zone_pos];
        if zone.data.nova3k.is_some() && !zone.nova3k_gate_open {
            self.nova3k_progress_inner(zone_pos, true).await;
            return Ok(());
        }
        let current = self.zones[zone_pos].data.settings.zone_id;
        self.nova3k_move_zone(id, current, u32::MAX).await
    }

    /// Nova3K: zone gate (MoveZone 0x03 0x05). The quest's warp table (map_data.warps, client zone ids)
    /// names the destination; without a matching warp the next zone of the quest is used.
    pub async fn nova3k_move_zone(
        &mut self,
        id: PlayerId,
        current_zone_id: u32,
        door_id: u32,
    ) -> Result<(), Error> {
        let warp = self
            .data
            .map_data
            .warps
            .iter()
            .find(|w| w.zone_id == current_zone_id && w.door_id == door_id)
            .or_else(|| self.data.map_data.warps.iter().find(|w| w.zone_id == current_zone_id))
            .map(|w| w.dest_zone);
        let dest_client_zone = warp.unwrap_or(current_zone_id.wrapping_add(1));
        let dest = self
            .zones
            .iter()
            .find(|z| z.data.settings.zone_id == dest_client_zone)
            .map(|z| z.srv_zone_id);
        log::info!(
            "Nova3K move zone: player {id} zone {current_zone_id} door {door_id} -> client zone {dest_client_zone} ({})",
            if warp.is_some() { "warp table" } else { "next zone" }
        );
        let Some(dest) = dest else {
            log::info!("Nova3K move zone: no zone {dest_client_zone} in this quest");
            return Ok(());
        };
        self.move_player(id, dest).await
    }

    pub async fn remove_player(&mut self, id: PlayerId) -> Option<Arc<Mutex<User>>> {
        let zone_pos = self.find_player(id)?;
        let (player_pos, _) = self
            .player_cache
            .iter()
            .enumerate()
            .find(|(_, (i, _))| *i == id)?;
        self.player_cache.remove(player_pos);
        self.zones[zone_pos].remove_player(id).await
    }

    fn find_player(&self, id: PlayerId) -> Option<usize> {
        self.player_cache
            .iter()
            .find(|(p_id, _)| *p_id == id)
            .map(|(_, z_pos)| *z_pos)
    }

    pub async fn send_palette_change(
        &self,
        zone_pos: usize,
        sender_id: PlayerId,
    ) -> Result<(), Error> {
        self.zones[zone_pos].send_palette_change(sender_id).await
    }
    pub async fn send_to_all(&self, zone_pos: usize, packet: &Packet) {
        exec_users(&self.zones[zone_pos].players, |_, mut player| {
            let _ = player.try_send_packet(packet);
        })
        .await;
    }

    pub async fn send_movement(&self, zone_pos: usize, packet: Packet, sender_id: PlayerId) {
        self.zones[zone_pos].send_movement(packet, sender_id).await;
    }

    pub async fn send_message(&self, zone_pos: usize, packet: Packet, id: PlayerId) {
        self.zones[zone_pos].send_message(packet, id).await;
    }

    pub async fn send_sa(&self, zone_pos: usize, data: SendSymbolArtPacket, id: PlayerId) {
        self.zones[zone_pos].send_sa(data, id).await;
    }

    pub async fn spawn_enemy(
        &mut self,
        zone_pos: usize,
        name: &str,
        pos: Position,
    ) -> Result<(), Error> {
        let Some(block_data) = self.block_data.to_owned() else {
            return Err(Error::NoEnemyData(name.to_string()));
        };
        self.zones[zone_pos]
            .spawn_enemy(&block_data, &mut self.max_id, self.enemy_level, name, pos)
            .await?;
        Ok(())
    }
    pub async fn deal_damage(
        &mut self,
        zone_pos: usize,
        dmg: DealDamagePacket,
    ) -> Result<(), Error> {
        let Some(block_data) = self.block_data.to_owned() else {
            return Err(Error::InvalidInput("deal_damage"));
        };
        self.zones[zone_pos].deal_damage(block_data, dmg).await?;
        self.nova3k_progress(zone_pos).await;
        Ok(())
    }

    pub async fn minimap_reveal(
        &mut self,
        zone_pos: usize,
        sender_id: PlayerId,
        packet: protocol::questlist::MinimapRevealRequestPacket,
    ) -> Result<(), Error> {
        let Some(block_data) = self.block_data.to_owned() else {
            return Err(Error::InvalidInput("minimap_reveal: no block data"));
        };

        self.zones[zone_pos]
            .minimap_reveal(
                sender_id,
                &block_data,
                &mut self.max_id,
                self.enemy_level,
                &packet,
            )
            .await?;
        self.nova3k_progress(zone_pos).await;

        self.check_move_lua().await?;
        Ok(())
    }

    pub async fn interaction(
        &mut self,
        zone_pos: usize,
        packet: protocol::objects::InteractPacket,
        sender_id: PlayerId,
    ) -> Result<(), Error> {
        let zone_id = self.zones[zone_pos].srv_zone_id;
        let target_name = self
            .data
            .objects
            .iter()
            .filter(|o| o.zone_id == zone_id)
            .map(|x| (x.data.object.id, &x.data.name))
            .chain(
                self.data
                    .npcs
                    .iter()
                    .filter(|o| o.zone_id == zone_id)
                    .map(|x| (x.data.object.id, &x.data.name)),
            )
            .find(|(id, _)| *id == packet.object1.id)
            .map(|(_, name)| name.to_string())
            .or_else(|| {
                let gate = self.zones[zone_pos].nova3k_gate_id?;
                if packet.object1.id == gate {
                    Some("oa_transfer_multi".to_string())
                } else if packet.object1.id == gate + 1 {
                    Some("oa_transporter_universal".to_string())
                } else {
                    None
                }
            });
        // Nova3K: every (object, action) pair once in the log - the zone gates / quest exit need an answer
        // whose name is not known yet
        {
            use std::sync::{Mutex as StdMutex, OnceLock};
            static SEEN: OnceLock<StdMutex<std::collections::HashSet<(u32, String)>>> = OnceLock::new();
            let key = (packet.object1.id, format!("{}", packet.action));
            let mut seen = SEEN.get_or_init(Default::default).lock().unwrap();
            if seen.len() < 500 && seen.insert(key) {
                log::info!(
                    "Nova3K interact log: object {} '{}' action '{}' from {:?} unk2 {:?} object4 {:?}",
                    packet.object1.id,
                    target_name.as_deref().unwrap_or("?"),
                    packet.action,
                    packet.object3,
                    packet.unk2,
                    packet.object4
                );
            }
        }
        // Nova3K: zone gate / quest exit objects: acknowledge every action like the scripted objects do
        // ("<action>Success"); an action that reads as using the gate moves the player to the next zone.
        if let Some(name) = target_name.as_deref() {
            let ring = name.starts_with("oa_transporter_universal")
                && self.zones[zone_pos].nova3k_gate_id.is_some_and(|g| packet.object1.id == g + 1);
            if name.starts_with("oa_transfer_multi") || name.starts_with("oa_telepipe_clear") || ring {
                let action = format!("{}", packet.action);
                let reply = Packet::SetTag(protocol::objects::SetTagPacket {
                    receiver: ObjectHeader {
                        id: sender_id,
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                    target: packet.object1,
                    object3: packet.object3,
                    attribute: format!("{action}Success").into(),
                    ..Default::default()
                });
                if let Some(user) = self.zones[zone_pos]
                    .players
                    .iter()
                    .find(|p| p.player_id == sender_id)
                    .and_then(|p| p.user.upgrade())
                {
                    let _ = user.lock().await.try_send_packet(&reply);
                }
                // GetList is the client's own query right after the zone loads; any other action on the gate
                // comes from the player using it (its exact name is still to be seen in the log)
                let lower = action.to_ascii_lowercase();
                let uses_ring = ring && ["transfer", "use", "warp", "move", "enter", "access"].iter().any(|k| lower.contains(k));
                if (name.starts_with("oa_transfer_multi") && action != "GetList") || uses_ring {
                    let current = self.zones[zone_pos].data.settings.zone_id;
                    log::info!("Nova3K gate: action '{action}' on {name} -> next zone");
                    return self.nova3k_move_zone(sender_id, current, u32::MAX).await;
                }
                return Ok(());
            }
        }
        let lua_data = target_name
            .as_ref()
            .and_then(|name| self.lua.lock().procs.get(name.as_str()).cloned());
        let Some(lua_data) = lua_data else {
            // Nova3K: talking to an NPC without a script (shops, exchanges...) left the client waiting for
            // an answer forever. Answer like the scripted objects do ("Sit" -> "SitSuccess") so the
            // interaction ends, and log it: the action name tells what the NPC needs later.
            let attribute = format!("{}Success", packet.action);
            log::info!(
                "Nova3K interact: object {} '{}' action '{}' has no script, answering SetTag '{}'",
                packet.object1.id,
                target_name.as_deref().unwrap_or("?"),
                packet.action,
                attribute
            );
            let reply = Packet::SetTag(protocol::objects::SetTagPacket {
                receiver: ObjectHeader {
                    id: sender_id,
                    entity_type: ObjectType::Player,
                    ..Default::default()
                },
                target: packet.object1,
                object3: packet.object3,
                attribute: attribute.into(),
                ..Default::default()
            });
            if let Some(user) = self.zones[zone_pos]
                .players
                .iter()
                .find(|p| p.player_id == sender_id)
                .and_then(|p| p.user.upgrade())
            {
                let _ = user.lock().await.try_send_packet(&reply);
            }
            return Ok(());
        };
        self.run_lua(sender_id, zone_pos, &packet, "interaction", &lua_data)
            .await?;
        Ok(())
    }
    pub async fn on_questwork(
        &mut self,
        zone_pos: usize,
        player: PlayerId,
        packet: SkitItemAddRequestPacket,
    ) -> Result<(), Error> {
        let Some(lua) = self.lua.lock().procs.get("on_questwork").cloned() else {
            return Ok(());
        };
        self.run_lua(player, zone_pos, &packet, "on_questwork", &lua)
            .await?;
        self.check_move_lua().await?;
        Ok(())
    }
    pub async fn on_cutscene_end(
        &mut self,
        zone_pos: usize,
        player: PlayerId,
        packet: CutsceneEndPacket,
    ) -> Result<(), Error> {
        let Some(lua) = self.lua.lock().procs.get("on_cutscene_end").cloned() else {
            return Ok(());
        };
        self.run_lua(player, zone_pos, &packet, "on_cutscene_end", &lua)
            .await?;
        self.check_move_lua().await?;
        Ok(())
    }

    pub async fn on_map_loaded(&mut self, zone_pos: usize, player: PlayerId) -> Result<(), Error> {
        let Some(lua) = self.lua.lock().procs.get("on_map_loaded").cloned() else {
            return Ok(());
        };
        self.run_lua(player, zone_pos, &Packet::None, "on_map_loaded", &lua)
            .await?;
        self.check_move_lua().await?;
        Ok(())
    }
    pub fn get_close_objects<F>(&self, zone_pos: usize, pred: F) -> Vec<ObjectSpawnPacket>
    where
        F: Fn(&Position) -> bool,
    {
        let mut obj = vec![];
        let zone_id = self.zones[zone_pos].srv_zone_id;
        for self_obj in self.data.objects.iter().filter(|o| o.zone_id == zone_id) {
            if pred(&self_obj.data.position) {
                obj.push(self_obj.data.clone());
            }
        }

        obj
    }

    async fn run_lua<S: serde::Serialize + Sync>(
        &mut self,
        sender_id: PlayerId,
        zone_id: usize,
        packet: &S,
        call_type: &str,
        lua_data: &str,
    ) -> Result<(), Error> {
        spawn_blocking(|| {
            self.zones[zone_id].run_lua_blocking(sender_id, packet, call_type, lua_data)
        })
        .await?
    }
    async fn check_move_lua(&mut self) -> Result<(), Error> {
        let mut lua = self.lua.lock();
        let to_move: Vec<_> = lua.to_move.drain(..).collect();
        let to_lobby_move: Vec<_> = lua.to_lobby_move.drain(..).collect();
        drop(lua);
        for (player, zone) in to_move {
            self.move_player_named(player, &zone).await?;
        }
        for player in to_lobby_move {
            self.move_to_lobby(player).await?;
        }
        Ok(())
    }
}

impl Zone {
    async fn add_player(&mut self, new_player: Arc<Mutex<User>>) -> Result<(), Error> {
        let mut other_equipment = Vec::with_capacity(self.players.len() * 2);
        let mut other_characters = Vec::with_capacity(self.players.len());
        for player in self.players.iter().filter_map(|p| p.user.upgrade()) {
            let p = player.lock().await;
            let pid = p.get_user_id();
            let Some(char_data) = &p.character else {
                unreachable!("User should be in state >= `PreInGame`")
            };
            other_equipment.push(char_data.palette.send_change_palette(pid));
            other_equipment.push(char_data.palette.send_cur_weapon(pid, &char_data.inventory));
            other_equipment.push(char_data.inventory.send_equiped(pid));
            other_characters.push((char_data.character.clone(), p.position, p.user_data.isgm));
        }
        let mut np_lock = new_player.lock().await;
        np_lock.map_id = self.data.settings.map_id;
        np_lock.zone_pos = self.zone_pos;
        let np_id = np_lock.get_user_id();
        let Some(new_character) = np_lock.character.to_owned() else {
            unreachable!("User should be in state >= `PreInGame`")
        };
        np_lock
            .send_packet(&Packet::SetPlayerID(SetPlayerIDPacket {
                player_id: np_id,
                unk2: 4,
                ..Default::default()
            }))
            .await?;
        let pos = self.data.default_location;
        np_lock.position = pos;
        np_lock.nova3k_trail.clear();
        np_lock.nova3k_start_y = if self.data.nova3k.is_some() { Some(pos.pos_y.to_f32()) } else { None };
        let np_gm = np_lock.user_data.isgm as u32;
        np_lock
            .spawn_character(CharacterSpawnPacket {
                position: pos,
                character: new_character.character.clone(),
                spawn_type: CharacterSpawnType::Myself,
                gm_flag: np_gm,
                player_obj: ObjectHeader {
                    id: np_id,
                    entity_type: ObjectType::Player,
                    ..Default::default()
                },
                ..Default::default()
            })
            .await?;
        Self::load_objects(&self.lua, &self.objects, &mut np_lock)?;
        for (character, position, isgm) in other_characters {
            let player_id = character.player_id;
            np_lock
                .spawn_character(CharacterSpawnPacket {
                    position,
                    spawn_type: CharacterSpawnType::Other,
                    gm_flag: isgm as u32,
                    player_obj: ObjectHeader {
                        id: player_id,
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                    character,
                    ..Default::default()
                })
                .await?;
        }
        for equipment in other_equipment {
            np_lock.send_packet(&equipment).await?;
        }
        let new_eqipment = (
            new_character.palette.send_change_palette(np_id),
            new_character
                .palette
                .send_cur_weapon(np_id, &new_character.inventory),
            new_character.inventory.send_equiped(np_id),
        );

        for (id, enemy) in &self.enemies {
            let (packet, mut packet2) = Self::prepare_enemy_packets(*id, enemy);
            if let Packet::EnemyAction(data) = &mut packet2 {
                data.receiver = np_lock.create_object_header();
                data.action_starter = np_lock.create_object_header();
            }
            np_lock.send_packet(&packet).await?;
            np_lock.send_packet(&packet2).await?;
        }
        drop(np_lock);

        exec_users(&self.players, |_, mut player| {
            let _ = player.try_spawn_character(CharacterSpawnPacket {
                position: pos,
                spawn_type: CharacterSpawnType::Other,
                gm_flag: np_gm,
                player_obj: ObjectHeader {
                    id: new_character.character.player_id,
                    entity_type: ObjectType::Player,
                    ..Default::default()
                },
                character: new_character.character.clone(),
                ..Default::default()
            });
            let _ = player.try_send_packet(&new_eqipment.0);
            let _ = player.try_send_packet(&new_eqipment.1);
            let _ = player.try_send_packet(&new_eqipment.2);
        })
        .await;
        self.players.push(MapPlayer {
            player_id: np_id,
            user: Arc::downgrade(&new_player),
        });

        let Some(lua) = self.lua.lock().procs.get("on_player_load").cloned() else {
            return Ok(());
        };
        self.run_lua(np_id, &Packet::None, "on_player_load", &lua)
            .await?;

        Ok(())
    }

    fn load_objects(
        lua: &parking_lot::Mutex<LuaState>,
        map_data: &Objects,
        user: &mut User,
    ) -> Result<(), Error> {
        let lua_lock = lua.lock();
        let lua = &lua_lock.lua;
        for mut obj in map_data.objects.iter().cloned() {
            if user.user_data.packet_type == PacketType::Vita {
                let lua_code = lua_lock
                    .procs
                    .get(obj.data.name.as_str())
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let globals = lua.globals();
                globals.set("data", obj.data.data.as_slice())?;
                globals.set("call_type", "to_vita")?;
                globals.set("size", obj.data.data.len())?;
                let chunk = lua.load(lua_code);
                chunk.exec()?;
                obj.data.data = globals.get::<Vec<u32>>("data")?.into();
                globals.raw_remove("data")?;
                globals.raw_remove("call_type")?;
                globals.raw_remove("size")?;
            }
            user.try_send_packet(&Packet::ObjectSpawn(obj.data))?;
        }
        for npc in map_data.npcs.iter().cloned() {
            user.try_send_packet(&Packet::NPCSpawn(npc.data))?;
        }
        for event in map_data.events.iter().cloned() {
            user.try_send_packet(&Packet::EventSpawn(event.data))?;
        }
        for tele in map_data.transporters.iter().cloned() {
            user.try_send_packet(&Packet::TransporterSpawn(tele.data))?;
        }

        Ok(())
    }

    async fn remove_player(&mut self, id: PlayerId) -> Option<Arc<Mutex<User>>> {
        let (pos, _) = self
            .players
            .iter()
            .enumerate()
            .find(|(_, p)| p.player_id == id)?;
        let user = self.players.swap_remove(pos);
        let mut packet = Packet::DespawnPlayer(protocol::objects::DespawnPlayerPacket {
            receiver: ObjectHeader {
                id: 0,
                entity_type: ObjectType::Player,
                ..Default::default()
            },
            removed_player: ObjectHeader {
                id,
                entity_type: ObjectType::Player,
                ..Default::default()
            },
        });
        exec_users(&self.players, |_, mut player| {
            if let Packet::DespawnPlayer(data) = &mut packet {
                data.receiver.id = player.get_user_id();
                let _ = player.try_send_packet(&packet);
            }
        })
        .await;
        user.user.upgrade()
    }

    async fn send_palette_change(&self, sender_id: PlayerId) -> Result<(), Error> {
        let Some(user) = self.players.iter().find(|p| p.player_id == sender_id) else {
            return Err(Error::NoUserInMap(sender_id, self.data.name.clone()));
        };
        let Some(player) = user.user.upgrade() else {
            return Err(Error::InvalidInput("send_palette_change"));
        };
        let new_eqipment = {
            let p = player.lock().await;
            let Some(character) = &p.character else {
                unreachable!("Users in map should have characters")
            };
            (
                character.palette.send_change_palette(sender_id),
                character
                    .palette
                    .send_cur_weapon(sender_id, &character.inventory),
            )
        };
        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&new_eqipment.0);
            let _ = player.try_send_packet(&new_eqipment.1);
        })
        .await;

        Ok(())
    }
    async fn send_movement(&self, packet: Packet, sender_id: PlayerId) {
        let mut out_packet = match packet {
            Packet::Movement(_) => packet,
            Packet::MovementEnd(mut data) => {
                if data.unk1.id == 0 && data.unk2.id != 0 {
                    data.unk1 = data.unk2;
                }
                Packet::MovementEnd(data)
            }
            Packet::MovementAction(data) => {
                let packet = protocol::objects::MovementActionServerPacket {
                    performer: data.performer,
                    receiver: ObjectHeader {
                        id: 0,
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                    unk3: data.unk3,
                    unk4: data.unk4,
                    unk5: data.unk5,
                    unk6: data.unk6,
                    action: data.action,
                    unk7: data.unk7,
                    unk8: data.unk8,
                    unk9: data.unk9,
                    unk10: data.unk10,
                };
                Packet::MovementActionServer(packet)
            }
            Packet::ActionUpdate(data) => {
                let packet = protocol::objects::ActionUpdateServerPacket {
                    performer: data.performer,
                    unk2: data.unk2,
                    receiver: ObjectHeader {
                        id: 0,
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                };
                Packet::ActionUpdateServer(packet)
            }
            Packet::ActionEnd(mut data) => {
                data.unk1 = data.performer;
                Packet::ActionEnd(data)
            }
            _ => return,
        };
        exec_users(&self.players, |user, mut player| {
            if let Packet::MovementActionServer(ref mut data) = out_packet {
                data.receiver.id = player.get_user_id();
            } else if let Packet::ActionUpdateServer(ref mut data) = out_packet {
                data.receiver.id = player.get_user_id();
            }
            if user.player_id != sender_id {
                let _ = player.try_send_packet(&out_packet);
            }
        })
        .await;
    }

    async fn send_message(&self, mut packet: Packet, id: PlayerId) {
        if let Packet::ChatMessage(ref mut data) = packet {
            data.object = ObjectHeader {
                id,
                entity_type: ObjectType::Player,
                ..Default::default()
            };
        }
        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&packet);
        })
        .await;
    }

    async fn send_sa(&self, data: SendSymbolArtPacket, id: PlayerId) {
        let packet = Packet::ReceiveSymbolArt(ReceiveSymbolArtPacket {
            object: ObjectHeader {
                id,
                entity_type: ObjectType::Player,
                ..Default::default()
            },
            uuid: data.uuid,
            area: data.area,
            unk1: data.unk1,
            unk2: data.unk2,
            unk3: data.unk3,
        });
        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&packet);
        })
        .await;
    }

    fn prepare_enemy_packets(enemy_id: u32, enemy: &EnemyStats) -> (Packet, Packet) {
        let packet = enemy.create_spawn_packet(enemy_id);
        // techically this is a response to 0x04 0x2B
        let packet2 = Packet::EnemyAction(EnemyActionPacket {
            actor: packet.object,
            action_id: 7,
            ..Default::default()
        });
        let packet = Packet::EnemySpawn(packet);
        (packet, packet2)
    }

    async fn spawn_enemy(
        &mut self,
        block_data: &BlockData,
        max_id: &mut u32,
        enemy_lvl: u32,
        name: &str,
        pos: Position,
    ) -> Result<(), Error> {
        self.nova3k_spawn_enemy(block_data, max_id, enemy_lvl, name, pos, 1).await
    }

    /// Nova3K: spawn_enemy with an HP multiplier (bosses: 20x).
    async fn nova3k_spawn_enemy(
        &mut self,
        block_data: &BlockData,
        max_id: &mut u32,
        enemy_lvl: u32,
        name: &str,
        pos: Position,
        hp_mul: u32,
    ) -> Result<(), Error> {
        let id = *max_id + 1;
        *max_id += 1;
        let mut data = EnemyStats::build(name, enemy_lvl, pos, &block_data.server_data)?;
        data.nova3k_boost(hp_mul);
        let (packet, mut packet2) = Zone::prepare_enemy_packets(id, &data);
        self.enemies.push((id, data));

        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&packet);
            if let Packet::EnemyAction(data) = &mut packet2 {
                data.receiver = player.create_object_header();
                data.action_starter = player.create_object_header();
                let _ = player.try_send_packet(&packet2);
            }
        })
        .await;

        Ok(())
    }

    async fn deal_damage(
        &mut self,
        block_data: Arc<BlockData>,
        dmg: DealDamagePacket,
    ) -> Result<(), Error> {
        let (inflicter, target) = (dmg.inflicter, dmg.target);
        if inflicter.entity_type == ObjectType::Player && target.entity_type == ObjectType::Object {
            let Some((enemy_pos, (_, target))) = self
                .enemies
                .iter_mut()
                .enumerate()
                .find(|(_, (id, _))| *id == target.id)
            else {
                return Ok(());
            };
            let Some(inflicter) = self
                .players
                .iter()
                .find(|u| u.player_id == inflicter.id)
                .and_then(|p| p.user.upgrade())
            else {
                return Err(Error::InvalidInput("deal_damage"));
            };
            let mut lock = inflicter.lock().await;
            let result = lock
                .get_stats_mut()
                .damage_enemy(target, &block_data.server_data, dmg)?;
            drop(lock);
            match result {
                BattleResult::Damaged { dmg_packet } => {
                    let mut packet = Packet::DamageReceive(dmg_packet);
                    exec_users(&self.players, |_, mut player| {
                        if let Packet::DamageReceive(data) = &mut packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&packet);
                        }
                    })
                    .await;
                }
                BattleResult::Killed {
                    dmg_packet,
                    kill_packet,
                    exp_amount,
                } => {
                    let mut action_packet = Packet::EnemyAction(EnemyActionPacket {
                        actor: dmg_packet.dmg_target,
                        action_starter: dmg_packet.dmg_inflicter,
                        action_id: 4,
                        ..Default::default()
                    });
                    let mut dmg_packet = Packet::DamageReceive(dmg_packet);
                    let mut kill_packet = Packet::EnemyKilled(kill_packet);
                    let mut exp_packets = vec![];
                    exec_users(&self.players, |_, mut player| {
                        exp_packets.push(player.add_exp(exp_amount))
                    })
                    .await;
                    let exp_packets = exp_packets.into_iter().collect::<Result<Vec<_>, _>>()?;
                    // Nova3K: loot for every player of the map: meseta always, sometimes a Monomate
                    // (the server has no ground drops yet, so the loot goes straight into the inventory)
                    let killed_level = target.nova3k_level();
                    exec_users(&self.players, |_, mut player| {
                        nova3k_give_loot(&mut *player, killed_level);
                    })
                    .await;
                    let mut exp_packet = Packet::GainedEXP(GainedEXPPacket {
                        receivers: exp_packets,
                        ..Default::default()
                    });
                    exec_users(&self.players, |_, mut player| {
                        if let Packet::DamageReceive(data) = &mut dmg_packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&dmg_packet);
                        }
                        if let Packet::EnemyKilled(data) = &mut kill_packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&kill_packet);
                        }
                        if let Packet::GainedEXP(data) = &mut exp_packet {
                            data.sender = player.create_object_header();
                            let _ = player.try_send_packet(&exp_packet);
                        }
                        if let Packet::EnemyAction(data) = &mut action_packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&action_packet);
                        }
                    })
                    .await;
                    let killed_id = self.enemies[enemy_pos].0;
                    self.enemies.remove(enemy_pos);
                    self.nova3k_on_kill(killed_id).await;
                }
            }
        } else if inflicter.entity_type == ObjectType::Object
            && target.entity_type == ObjectType::Player
        {
            let Some(target) = self
                .players
                .iter_mut()
                .find(|u| u.player_id == target.id)
                .and_then(|p| p.user.upgrade())
            else {
                return Err(Error::InvalidInput("deal_damage"));
            };
            let Some((_, inflicter)) = self.enemies.iter_mut().find(|(id, _)| *id == inflicter.id)
            else {
                return Ok(());
            };
            let mut lock = target.lock().await;
            let result =
                inflicter.damage_player(lock.get_stats_mut(), &block_data.server_data, dmg)?;
            drop(lock);

            match result {
                BattleResult::Damaged { dmg_packet } => {
                    let mut packet = Packet::DamageReceive(dmg_packet);
                    exec_users(&self.players, |_, mut player| {
                        if let Packet::DamageReceive(data) = &mut packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&packet);
                        }
                    })
                    .await;
                }
                BattleResult::Killed { dmg_packet, kill_packet, .. } => {
                    let mut dmg_packet = Packet::DamageReceive(dmg_packet);
                    // Nova3K: also the "object killed" packet (0x04 0x0F) for the player, like for enemies:
                    // with only HP 0 the client kept playing
                    let mut kill_packet = Packet::EnemyKilled(kill_packet);
                    exec_users(&self.players, |_, mut player| {
                        if let Packet::DamageReceive(data) = &mut dmg_packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&dmg_packet);
                        }
                        if let Packet::EnemyKilled(data) = &mut kill_packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&kill_packet);
                        }
                    })
                    .await;
                    // Nova3K: was todo!() - a player death panicked the server task (disconnect).
                    // The client handles death itself (revive / DeathToCampship 0x03 0x1A).
                    log::info!("Nova3K: player killed");
                }
            }
        }

        Ok(())
    }
    /// Nova3K: after the last boss of a "boss" zone dies, open the quest exit (oa_telepipe_clear, the
    /// telepipe of a cleared quest: using it sends ReturnToCampshipFinal 0x03 0x19) at the boss spawn point
    /// and give the clear reward (meseta 100 + 25 x quest level).
    /// Nova3K: system message to every player of the zone.
    async fn nova3k_message(&self, text: &str) {
        let msg = Packet::SystemMessage(protocol::unk19::SystemMessagePacket {
            message: text.to_string(),
            msg_type: protocol::unk19::MessageType::SystemMessage,
            ..Default::default()
        });
        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&msg);
        })
        .await;
    }

    async fn nova3k_on_kill(&mut self, killed_id: u32) {
        self.nova3k_kills += 1;
        if let Some(cfg) = self.data.nova3k.as_ref() {
            let need = cfg.gate_kills;
            if !self.nova3k_gate_open && need > 0 && self.nova3k_kills < need && self.nova3k_kills % 5 == 0 {
                let what = if cfg.boss.is_some() { "the boss" } else { "the gate" };
                let text = format!("Enemies defeated: {}/{} ({what} appears at {need})", self.nova3k_kills, need);
                self.nova3k_message(&text).await;
            }
        }
        if self.nova3k_boss_done || !self.nova3k_boss_ids.contains(&killed_id) {
            return;
        }
        if self.enemies.iter().any(|(id, _)| self.nova3k_boss_ids.contains(id)) {
            return;
        }
        self.nova3k_boss_done = true;
        // Nova3K: the exit is a telepipe (the oa_telepipe_clear object of quest 50570 was never drawn) next to
        // the player who landed the last hit - a spot on the ground. Using it ends the quest (campship).
        let mut pos = self.data.default_location;
        for p in self.players.iter().filter_map(|p| p.user.upgrade()) {
            pos = p.lock().await.position;
            break;
        }
        pos.pos_x = half::f16::from_f32(pos.pos_x.to_f32() + 3.0);
        // oa_telepipe_start of quest 50570, already in the Vita layout (the default to_vita handler lowers
        // the keys 51..79 by one)
        let exit = ObjectSpawnPacket {
            object: ObjectHeader {
                id: 60000 + self.zone_pos as u32 * 10 + 3,
                entity_type: ObjectType::Object,
                map_id: self.data.settings.world_id as u16,
                ..Default::default()
            },
            position: pos,
            name: String::from("oa_telepipe_start").into(),
            unk2: [16, 0, 0, 0, 0],
            flags: 4,
            data: vec![1560, 15795201, 2, 0, 0, 0, 256, 4, 57, 4294967295, 54, 1, 56, 0, 65, 1, 66, 0, 70, 1, 2, 0, 0, 0].into(),
            ..Default::default()
        };
        let level = self.nova3k_level;
        let reward = 100 + 25 * level as u64;
        log::info!("Nova3K boss: all bosses dead -> quest exit (telepipe) opened, clear reward {reward} meseta");
        let packet = Packet::ObjectSpawn(exit);
        let msg = Packet::SystemMessage(protocol::unk19::SystemMessagePacket {
            message: format!("Boss defeated! Reward: {reward} meseta. Use the telepipe to finish the quest."),
            msg_type: protocol::unk19::MessageType::SystemMessage,
            ..Default::default()
        });
        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&packet);
            let _ = player.try_send_packet(&msg);
            let user: &mut User = &mut player;
            if let Some(character) = user.character.as_mut() {
                let p = character.inventory.nova3k_add_meseta(reward);
                let _ = user.try_send_packet(&p);
            }
        })
        .await;
    }

    async fn minimap_reveal(
        &mut self,
        sender_id: PlayerId,
        block_data: &BlockData,
        max_id: &mut u32,
        enemy_lvl: u32,
        packet: &protocol::questlist::MinimapRevealRequestPacket,
    ) -> Result<(), Error> {
        let Some(user) = self
            .players
            .iter()
            .find(|p| p.player_id == sender_id)
            .cloned()
        else {
            return Err(Error::InvalidInput("minimap_reveal"));
        };

        if let Some(chunk) = self
            .data
            .chunks
            .iter()
            .find(|c| c.chunk_id == packet.chunk_id)
        {
            for reveal in &chunk.reveals {
                self.minimap_status[reveal.row as usize - 1].set(reveal.column as usize - 1, true);
            }
            // Nova3K: the client names the minimap cell it entered; the generated quests do not know the
            // real chunk -> cell table, so their guessed reveals left the minimap empty. Reveal that cell too.
            // (the reveal grid is 8 rows x 10 columns)
            if (1..=8).contains(&(packet.map_row as usize)) && (1..=10).contains(&(packet.map_column as usize)) {
                self.minimap_status[packet.map_row as usize - 1].set(packet.map_column as usize - 1, true);
            }

            // wow, how nested
            match chunk.enemy_spawn_type {
                data_structs::map::EnemySpawnType::Disabled => {}
                data_structs::map::EnemySpawnType::Automatic { min, max } => {
                    let count = Uniform::new_inclusive(min, max)
                        .unwrap()
                        .sample(&mut rand::rng());
                    // Nova3K: generated fields spawn on the walked trail; right after the zone loads there is none
                    // yet (the position is still the computed start, maybe inside the ground): wait for the
                    // player's first steps, the chunk spawns when it is entered again
                    let no_trail = chunk.enemy_spawn_points.len() >= 9 && {
                        let u = user.user.upgrade().unwrap();
                        let l = u.lock().await;
                        l.nova3k_trail.len() < 2
                    };
                    if !no_trail && !self.chunk_spawns.iter().any(|s| s.0 == chunk.chunk_id) {
                        self.chunk_spawns
                            .push((chunk.chunk_id, std::time::Instant::now()));
                        // this is length biased
                        let spawn_category = self
                            .data
                            .enemies
                            .iter()
                            .map(|e| e.spawn_category)
                            .choose(&mut rand::rng())
                            .unwrap_or_default();
                        // Nova3K: generated quests list many points per chunk -> nearest to the player
                        let (player_pos, trail) = {
                            let u = user.user.upgrade().unwrap();
                            let l = u.lock().await;
                            (l.position, l.nova3k_trail.clone())
                        };
                        let spots = nova3k_spawn_spots(&trail, &player_pos, &chunk.enemy_spawn_points, count as usize);
                        for spawn_point in spots {
                            let enemy = self
                                .data
                                .enemies
                                .iter()
                                .filter(|e| e.spawn_category == spawn_category)
                                .choose(&mut rand::rng());
                            if let Some(enemy) = enemy {
                                let enemy_name = enemy.enemy_name.clone();
                                self.spawn_enemy(
                                    block_data,
                                    max_id,
                                    enemy_lvl,
                                    &enemy_name,
                                    spawn_point,
                                )
                                .await?;
                            }
                        }
                    }
                }
                data_structs::map::EnemySpawnType::AutomaticWithRespawn {
                    min,
                    max,
                    respawn_time,
                } => {
                    let count = Uniform::new_inclusive(min, max)
                        .unwrap()
                        .sample(&mut rand::rng());
                    let (spawn, is_first) = if let Some(spawn) =
                        self.chunk_spawns.iter().find(|s| s.0 == chunk.chunk_id)
                    {
                        (spawn, false)
                    } else {
                        self.chunk_spawns
                            .push((chunk.chunk_id, std::time::Instant::now()));
                        (self.chunk_spawns.last().unwrap(), true)
                    };

                    if is_first || spawn.1.elapsed() > respawn_time {
                        // this is length biased
                        let spawn_category = self
                            .data
                            .enemies
                            .iter()
                            .map(|e| e.spawn_category)
                            .choose(&mut rand::rng())
                            .unwrap_or_default();
                        // Nova3K: generated quests list many points per chunk -> nearest to the player
                        let (player_pos, trail) = {
                            let u = user.user.upgrade().unwrap();
                            let l = u.lock().await;
                            (l.position, l.nova3k_trail.clone())
                        };
                        let spots = nova3k_spawn_spots(&trail, &player_pos, &chunk.enemy_spawn_points, count as usize);
                        for spawn_point in spots {
                            let enemy = self
                                .data
                                .enemies
                                .iter()
                                .filter(|e| e.spawn_category == spawn_category)
                                .choose(&mut rand::rng());
                            if let Some(enemy) = enemy {
                                let enemy_name = enemy.enemy_name.clone();
                                self.spawn_enemy(
                                    block_data,
                                    max_id,
                                    enemy_lvl,
                                    &enemy_name,
                                    spawn_point,
                                )
                                .await?;
                            }
                        }
                    }
                }
                data_structs::map::EnemySpawnType::Manual => {
                    let proc = self.lua.lock().procs.get("spawn_enemy").cloned();
                    if let Some(lua) = proc {
                        self.run_lua(user.player_id, &packet, "spawn_enemy", &lua)
                            .await?;
                    };
                }
            }

            let mut packet = Packet::MinimapReveal(MinimapRevealPacket {
                world: ObjectHeader {
                    id: self.data.settings.world_id,
                    entity_type: ObjectType::World,
                    ..Default::default()
                },
                zone_id: self.data.settings.zone_id,
                revealed_zones: self.minimap_status.clone(),
                ..Default::default()
            });
            for player in &self.players {
                let Some(player) = player.user.upgrade() else {
                    continue;
                };
                let mut p_lock = player.lock().await;
                let party_lock = p_lock
                    .party
                    .as_ref()
                    .expect("Player should have a party at this point")
                    .read()
                    .await;
                let party_obj = party_lock.get_obj();
                drop(party_lock);
                if let Packet::MinimapReveal(p) = &mut packet {
                    p.party = party_obj;
                }
                let _ = p_lock.send_packet(&packet).await;
            }
        }

        let proc = self.lua.lock().procs.get("on_minimap_reveal").cloned();
        if let Some(lua) = proc {
            self.run_lua(user.player_id, &packet, "on_minimap_reveal", &lua)
                .await?;
        };

        Ok(())
    }

    async fn run_lua<S: serde::Serialize + Sync>(
        &mut self,
        sender_id: PlayerId,
        packet: &S,
        call_type: &str,
        lua_data: &str,
    ) -> Result<(), Error> {
        spawn_blocking(|| self.run_lua_blocking(sender_id, packet, call_type, lua_data)).await?
    }

    fn run_lua_blocking<S: serde::Serialize + Sync>(
        &mut self,
        sender_id: PlayerId,
        packet: &S,
        call_type: &str,
        lua_data: &str,
    ) -> Result<(), Error> {
        let mut scheduled_move = vec![];
        let mut lobby_moves = vec![];

        let Some(caller) = self
            .players
            .iter()
            .find(|p| p.player_id == sender_id)
            .and_then(|p| p.user.upgrade())
        else {
            unreachable!("Sender should exist in the current map");
        };
        let caller_lock = caller.lock_blocking();
        let zone_set = &self.data;
        drop(caller_lock);
        let mut lua_lock = self.lua.lock();
        {
            let lua = &lua_lock.lua;
            let globals = lua.globals();
            let player_ids: Vec<_> = self.players.iter().map(|p| p.player_id).collect();
            globals.set("zone", zone_set.name.clone())?;
            globals.set("packet", lua.to_value(&packet)?)?;
            globals.set("sender", sender_id)?;
            globals.set("players", player_ids)?;
            globals.set("call_type", call_type)?;
            lua.scope(|scope| {
                self.setup_scope(&globals, scope, &mut scheduled_move, &mut lobby_moves)?;

                /* LUA FUNCTIONS */

                // get account flag
                globals.set(
                    "get_account_flag",
                    scope.create_function_mut(|_, flag: u32| -> Result<u8, _> {
                        Ok(caller.lock_blocking().get_account_flags().get(flag as _))
                    })?,
                )?;
                // get character flag
                globals.set(
                    "get_character_flag",
                    scope.create_function_mut(|_, flag: u32| -> Result<u8, _> {
                        if let Some(f) = caller.lock_blocking().get_char_flags() {
                            Ok(f.get(flag as _))
                        } else {
                            unreachable!("Users in maps should have loaded characters")
                        }
                    })?,
                )?;

                /* LUA FUNCTIONS END */

                let chunk = lua.load(lua_data);
                chunk.exec()?;
                Ok(())
            })?;
            globals.raw_remove("packet")?;
            globals.raw_remove("sender")?;
            globals.raw_remove("players")?;
            globals.raw_remove("call_type")?;
            globals.raw_remove("zone")?;
        }
        for (receiver, mapid) in scheduled_move {
            lua_lock.to_move.push((receiver, mapid));
        }
        for receiver in lobby_moves {
            lua_lock.to_lobby_move.push(receiver);
        }
        Ok(())
    }

    fn setup_scope<'s>(
        &'s self,
        globals: &mlua::Table,
        scope: &'s mlua::Scope<'s, '_>,
        scheduled_move: &'s mut Vec<(PlayerId, String)>,
        lobby_moves: &'s mut Vec<PlayerId>,
    ) -> Result<(), mlua::Error> {
        /* LUA FUNCTIONS */

        // send packet
        let send =
            scope.create_function_mut(move |lua, (receiver, packet): (u32, mlua::Value)| {
                let packet: Packet = lua.from_value(packet)?;
                if let Some(p) = self
                    .players
                    .iter()
                    .find(|p| p.player_id == receiver)
                    .and_then(|p| p.user.upgrade())
                {
                    p.lock_blocking()
                        .send_packet_block(&packet)
                        .map_err(mlua::Error::external)?;
                }
                Ok(())
            })?;
        globals.set("send", send)?;
        // get object data
        let get_object = scope.create_function(move |lua, id: u32| {
            let object = self
                .objects
                .objects
                .iter()
                .find(|obj| obj.data.object.id == id)
                .ok_or(mlua::Error::runtime("Couldn't find requested object"))?;
            lua.to_value(&object.data)
        })?;
        globals.set("get_object", get_object)?;
        // get npc data
        let get_npc = scope.create_function(move |lua, id: u32| {
            let object = self
                .objects
                .npcs
                .iter()
                .find(|obj| obj.data.object.id == id)
                .ok_or(mlua::Error::runtime("Couldn't find requested npc"))?;
            lua.to_value(&object.data)
        })?;
        globals.set("get_npc", get_npc)?;
        // get additional data
        let get_extra_data = scope.create_function(move |lua, id: u32| {
            let object = self
                .objects
                .objects
                .iter()
                .map(|x| (x.data.object.id, &x.lua_data))
                .chain(
                    self.objects
                        .npcs
                        .iter()
                        .map(|x| (x.data.object.id, &x.lua_data)),
                )
                .find(|(obj_id, _)| *obj_id == id)
                .map(|(_, data)| data)
                .ok_or(mlua::Error::runtime("Couldn't find requested object"))?;
            match object {
                Some(d) => lua.load(d).eval::<mlua::Value>(),
                None => Ok(mlua::Value::Nil),
            }
        })?;
        globals.set("get_extra_data", get_extra_data)?;
        // move player to another submap
        globals.set(
            "move_player",
            scope.create_function_mut(|_, (receiver, zone): (u32, String)| {
                scheduled_move.push((receiver, zone));
                Ok(())
            })?,
        )?;
        // move player to lobby
        globals.set(
            "move_lobby",
            scope.create_function_mut(|_, receiver: u32| {
                lobby_moves.push(receiver);
                Ok(())
            })?,
        )?;
        // set account flag
        globals.set(
            "set_account_flag",
            scope.create_function_mut(move |_, (receiver, flag, value): (u32, u32, u8)| {
                if let Some(p) = self
                    .players
                    .iter()
                    .find(|p| p.player_id == receiver)
                    .and_then(|p| p.user.upgrade())
                {
                    p.lock_blocking()
                        .set_account_flag_block(flag, value != 0)
                        .map_err(mlua::Error::external)?;
                }
                Ok(())
            })?,
        )?;
        // set character flag
        globals.set(
            "set_character_flag",
            scope.create_function_mut(move |_, (receiver, flag, value): (u32, u32, u8)| {
                if let Some(p) = self
                    .players
                    .iter()
                    .find(|p| p.player_id == receiver)
                    .and_then(|p| p.user.upgrade())
                {
                    p.lock_blocking()
                        .set_char_flag_block(flag, value != 0)
                        .map_err(mlua::Error::external)?;
                }
                Ok(())
            })?,
        )?;
        // delete all npcs from the client
        globals.set(
            "delete_all_npcs_packets",
            scope.create_function_mut(move |lua, receiver: u32| -> Result<mlua::Value, _> {
                let mut packets = vec![];
                for object in self.objects.npcs.iter().filter(|n| n.is_active) {
                    packets.push(Packet::DespawnObject(
                        protocol::objects::DespawnObjectPacket {
                            player: ObjectHeader {
                                id: receiver,
                                entity_type: ObjectType::Player,
                                ..Default::default()
                            },
                            item: object.data.object,
                        },
                    ))
                }
                lua.to_value(&packets)
            })?,
        )?;
        // unlock one quest
        globals.set(
            "unlock_quest",
            scope.create_function_mut(
                move |_, (receiver, name_id): (u32, u32)| -> Result<(), _> {
                    if let Some(p) = self
                        .players
                        .iter()
                        .find(|p| p.player_id == receiver)
                        .and_then(|p| p.user.upgrade())
                    {
                        let mut lock = p.lock_blocking();
                        let char = lock
                            .character
                            .as_mut()
                            .expect("Character should be loaded for users in map");
                        if !char.unlocked_quests.contains(&name_id) {
                            char.unlocked_quests.push(name_id);
                            char.unlocked_quests_notif.push(name_id);
                        }
                    }
                    Ok(())
                },
            )?,
        )?;
        // unlock multiple quests
        globals.set(
            "unlock_quests",
            scope.create_function_mut(
                move |_, (receiver, name_id): (u32, Vec<u32>)| -> Result<(), _> {
                    if let Some(p) = self
                        .players
                        .iter()
                        .find(|p| p.player_id == receiver)
                        .and_then(|p| p.user.upgrade())
                    {
                        let mut lock = p.lock_blocking();
                        let char = lock
                            .character
                            .as_mut()
                            .expect("Character should be loaded for users in map");
                        for name in name_id {
                            if !char.unlocked_quests.contains(&name) {
                                char.unlocked_quests.push(name);
                                char.unlocked_quests_notif.push(name);
                            }
                        }
                    }
                    Ok(())
                },
            )?,
        )?;

        /* LUA FUNCTIONS END */
        Ok(())
    }
}

impl Drop for Map {
    fn drop(&mut self) {
        log::trace!("Map {} dropped", self.data.map_data.map_object.id);
    }
}

async fn exec_users<F>(users: &[MapPlayer], mut f: F)
where
    F: FnMut(OwnedMapPlayer, MutexGuard<User>) + Send,
{
    for user in users.iter().filter_map(|u| {
        u.user.upgrade().map(|p| OwnedMapPlayer {
            player_id: u.player_id,
            user: p,
        })
    }) {
        let arc = user.user.clone();
        let lock = arc.lock().await;
        f(user, lock)
    }
}

async fn spawn_blocking<F, R>(func: F) -> Result<R, Error>
where
    F: FnOnce() -> R + Send,
    R: Send + 'static,
{
    let val: Box<dyn FnOnce() -> R + Send> = Box::new(func);
    // SAFETY: this should be safe because we immediately await the function
    let func: Box<dyn FnOnce() -> R + Send + 'static> = unsafe { std::mem::transmute(val) };
    Ok(tokio::task::spawn_blocking(func).await?)
}

/// Nova3K: choose where a chunk's enemies appear (no longer used: see nova3k_spawn_spots).
#[allow(dead_code)]
/// Chunks with few points (captured server data) keep the original random choice. Generated quests
/// (tools/quest_gen.py) do not know the real chunk ids yet, so their chunks list every point of the
/// layout: there the nearest point that is not right on top of the player is used.
fn nova3k_pick_spawn_point(points: &[Position], player: &Position) -> Option<Position> {
    const GENERATED_MIN_POINTS: usize = 9;
    const MIN_DISTANCE_SQ: f32 = 8.0 * 8.0;
    if points.len() < GENERATED_MIN_POINTS {
        return points.iter().copied().choose(&mut rand::rng());
    }
    let dist_sq = |p: &Position| {
        let dx = p.pos_x.to_f32() - player.pos_x.to_f32();
        let dy = p.pos_y.to_f32() - player.pos_y.to_f32();
        let dz = p.pos_z.to_f32() - player.pos_z.to_f32();
        dx * dx + dy * dy + dz * dz
    };
    let nearest = |min: f32| {
        points
            .iter()
            .copied()
            .filter(|p| dist_sq(p) >= min)
            .min_by(|a, b| dist_sq(a).total_cmp(&dist_sq(b)))
    };
    // Nova3K: only points close to the player and at about the same height; layouts also hold points of
    // closed-off event areas (enemies ended up under the ground or inside rocks). Otherwise: next to the player.
    const MAX_DISTANCE_SQ: f32 = 40.0 * 40.0;
    let usable = |p: &Position| {
        let d = dist_sq(p);
        d >= MIN_DISTANCE_SQ && d <= MAX_DISTANCE_SQ && (p.pos_y.to_f32() - player.pos_y.to_f32()).abs() < 3.0
    };
    if let Some(p) = points.iter().copied().filter(usable).min_by(|a, b| dist_sq(a).total_cmp(&dist_sq(b))) {
        return Some(p);
    }
    let angle = Uniform::new(0.0f32, std::f32::consts::TAU).unwrap().sample(&mut rand::rng());
    let radius = Uniform::new(6.0f32, 10.0).unwrap().sample(&mut rand::rng());
    let mut p = *player;
    p.pos_x = half::f16::from_f32(player.pos_x.to_f32() + radius * angle.cos());
    p.pos_z = half::f16::from_f32(player.pos_z.to_f32() + radius * angle.sin());
    Some(p)
}

/// Nova3K: a spot the player really stood on (its breadcrumb trail), 8-25 m away: walkable ground, so
/// enemies no longer appear inside walls or under the floor. None when the trail is too short yet.
fn nova3k_trail_point(trail: &[Position], player: &Position) -> Option<Position> {
    let d2 = |p: &Position| {
        let dx = p.pos_x.to_f32() - player.pos_x.to_f32();
        let dz = p.pos_z.to_f32() - player.pos_z.to_f32();
        dx * dx + dz * dz
    };
    trail
        .iter()
        .copied()
        .filter(|p| (64.0..=625.0).contains(&d2(p)) && (p.pos_y.to_f32() - player.pos_y.to_f32()).abs() < 6.0)
        .choose(&mut rand::rng())
}

/// Nova3K: where the `count` enemies of a chunk appear. Only spots the player really stood on (the trail):
/// a different trail point 8-25 m away for each enemy when there are enough, else any trail point at least
/// 3 m away, else the player's own spot. Layout points (generated quests) and the random ring around the
/// player put enemies under the floor or inside rocks, so they are used only by captured quests (few points).
fn nova3k_spawn_spots(trail: &[Position], player: &Position, points: &[Position], count: usize) -> Vec<Position> {
    if (1..9).contains(&points.len()) {
        // captured server data: keep its own spawn points
        return (0..count).filter_map(|_| points.iter().copied().choose(&mut rand::rng())).collect();
    }
    let d2 = |p: &Position| {
        let dx = p.pos_x.to_f32() - player.pos_x.to_f32();
        let dz = p.pos_z.to_f32() - player.pos_z.to_f32();
        dx * dx + dz * dz
    };
    let level = |p: &Position| (p.pos_y.to_f32() - player.pos_y.to_f32()).abs() < 6.0;
    let mut good: Vec<Position> = trail.iter().copied().filter(|p| (64.0..=625.0).contains(&d2(p)) && level(p)).collect();
    if good.is_empty() {
        good = trail.iter().copied().filter(|p| d2(p) >= 9.0 && level(p)).collect();
    }
    if good.is_empty() {
        good.push(*player);
    }
    good.shuffle(&mut rand::rng());
    (0..count)
        .map(|i| {
            let mut p = good[i % good.len()];
            // a little apart when several share one spot, slightly above the ground
            let k = (i / good.len()) as f32;
            p.pos_x = half::f16::from_f32(p.pos_x.to_f32() + 1.5 * k);
            p.pos_y = half::f16::from_f32(p.pos_y.to_f32() + 0.3);
            p
        })
        .collect()
}

/// Nova3K: position from x, y, z and a rotation around the vertical axis (degrees).
fn nova3k_position(x: f32, y: f32, z: f32, rot_deg: f32) -> Position {
    let a = rot_deg.to_radians() / 2.0;
    Position {
        rot_x: half::f16::from_f32(0.0),
        rot_y: half::f16::from_f32(a.sin()),
        rot_z: half::f16::from_f32(0.0),
        rot_w: half::f16::from_f32(a.cos()),
        pos_x: half::f16::from_f32(x),
        pos_y: half::f16::from_f32(y),
        pos_z: half::f16::from_f32(z),
    }
}

/// Nova3K: generated fields with several shapes (ZoneData.nova3k.variants): pick one at random for this run
/// and apply it - area template (`args`, also in the LoadLevel zone list), a new seed, start point, goal, the
/// rescue start points and the arrival telepipe of the zone.
fn nova3k_pick_variants(data: &mut MapData) {
    // fields of one quest get different shapes while the zone has others left
    let mut used: Vec<u32> = vec![];
    for zone in data.zones.iter_mut() {
        let Some(cfg) = zone.nova3k.as_mut() else {
            continue;
        };
        let fresh = cfg.variants.iter().filter(|v| !used.contains(&v.args)).choose(&mut rand::rng()).cloned();
        let Some(v) = fresh.or_else(|| cfg.variants.iter().choose(&mut rand::rng()).cloned()) else {
            continue;
        };
        used.push(v.args);
        zone.settings.args = v.args;
        zone.settings.seed = Uniform::new_inclusive(1u32, u32::MAX).unwrap().sample(&mut rand::rng());
        zone.default_location = nova3k_position(v.start[0], v.start[1], v.start[2], v.start[3]);
        cfg.goal = v.goal;
        let mut points: Vec<Position> = v.points.iter().map(|p| nova3k_position(p[0], p[1], p[2], 0.0)).collect();
        points.insert(0, zone.default_location);
        while points.len() < 9 {
            points.push(zone.default_location);
        }
        for chunk in zone.chunks.iter_mut() {
            chunk.enemy_spawn_points = points.clone();
        }
        for e in data.map_data.unk14.iter_mut() {
            if e.unk1 == zone.settings.zone_id {
                e.unk4 = v.args;
            }
        }
        for o in data.objects.iter_mut() {
            let name: &str = &o.data.name;
            if o.zone_id == zone.zone_id && name == "oa_telepipe_start" && o.data.object.id % 10 == 0 {
                o.data.position = zone.default_location;
            }
        }
        log::info!(
            "Nova3K field: zone {} uses area template {} (start {:.1} {:.1} {:.1})",
            zone.name,
            v.args,
            v.start[0],
            v.start[1],
            v.start[2]
        );
    }
}

/// Nova3K: ask the emulator to search the game memory for the enemy model ids (it watches for the file
/// nova3k_memscan.req in the server folder, the server's working directory; results go to vita3k.log).
pub fn nova3k_request_memscan(why: &str) {
    match std::fs::write("nova3k_memscan.req", why) {
        Ok(()) => log::info!("Nova3K memscan: requested ({why})"),
        Err(e) => log::info!("Nova3K memscan: could not request ({why}): {e}"),
    }
}

/// Nova3K: loot for one enemy kill (meseta 5..=15 + 2 x enemy level, Monomate with a 25% chance).
fn nova3k_give_loot(player: &mut User, enemy_level: u32) {
    let meseta = Uniform::new_inclusive(5u64, 15).unwrap().sample(&mut rand::rng()) + 2 * enemy_level as u64;
    let monomate = Uniform::new_inclusive(0u32, 99).unwrap().sample(&mut rand::rng()) < 25;
    let user: &mut User = player;
    let Some(character) = user.character.as_mut() else {
        return;
    };
    let mut packets = vec![character.inventory.nova3k_add_meseta(meseta)];
    if monomate {
        let item_id = pso2packetlib::protocol::items::ItemId {
            item_type: 3,
            id: 1,
            subid: 0,
            ..Default::default()
        };
        if let Some(packet) = character
            .inventory
            .nova3k_add_consumable(&mut user.user_data.last_uuid, item_id, 1)
        {
            packets.push(packet);
        }
    }
    for packet in &packets {
        let _ = user.try_send_packet(packet);
    }
}
