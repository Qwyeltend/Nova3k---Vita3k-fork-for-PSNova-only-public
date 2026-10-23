use crate::{Error, User};
use data_structs::{ServerData, stats::EnemyHitbox};
use pso2packetlib::protocol::{
    models::{Position, character::Class},
    objects::{DamageReceivePacket, EnemyKilledPacket},
    playerstatus::DealDamagePacket,
    spawn::EnemySpawnPacket,
};
use rand::distr::{Distribution, Uniform};

#[derive(Debug, Clone, Default)]
pub struct PlayerStats {
    max_hp: u32,
    hp: u32,
    dex: u32,

    base_mel_pwr: u32,
    weapon_mel_pwr: u32,
    base_rng_pwr: u32,
    weapon_rng_pwr: u32,
    base_tec_pwr: u32,
    weapon_tec_pwr: u32,

    base_mel_def: u32,
    base_rng_def: u32,
    base_tec_def: u32,
}

#[derive(Debug, Clone, Default)]
pub struct EnemyStats {
    name: String,
    level: u32,
    exp: u32,
    pos: Position,

    max_hp: u32,
    hp: u32,
    dex: u32,

    max_mel_pwr: u32,
    min_mel_pwr: u32,
    max_rng_pwr: u32,
    min_rng_pwr: u32,
    max_tec_pwr: u32,
    min_tec_pwr: u32,

    mel_def: u32,
    rng_def: u32,
    tec_def: u32,

    hitboxes: Vec<EnemyHitbox>,
}

pub enum BattleResult {
    Damaged {
        dmg_packet: DamageReceivePacket,
    },
    Killed {
        dmg_packet: DamageReceivePacket,
        kill_packet: EnemyKilledPacket,
        exp_amount: u32,
    },
}

impl PlayerStats {
    pub fn build(user: &User) -> Result<Self, Error> {
        let Some(char) = &user.character else {
            unreachable!("User should be in state >= `PreInGame`")
        };
        let server_data = &user.get_blockdata().server_data;

        let char_data = &char.character;
        let class = char_data.classes.main_class as usize;
        let level = char_data.get_level().level1 as usize;
        let mut resulting_stats = Self::calculate_class_stats(user, class, level);

        if char_data.classes.sub_class != Class::Unknown {
            // source: arks-visiphone
            let class = char_data.classes.sub_class as usize;
            let level = (char_data.get_sublevel().level1 as usize).min(level);
            let subclass_stats = Self::calculate_class_stats(user, class, level);
            resulting_stats.hp += subclass_stats.hp / 4;
            resulting_stats.max_hp = resulting_stats.hp;
            resulting_stats.dex += subclass_stats.dex / 4;
            resulting_stats.base_mel_pwr += subclass_stats.base_mel_pwr / 4;
            resulting_stats.base_rng_pwr += subclass_stats.base_rng_pwr / 4;
            resulting_stats.base_tec_pwr += subclass_stats.base_tec_pwr / 4;
            resulting_stats.base_mel_def += subclass_stats.base_mel_def / 4;
            resulting_stats.base_rng_def += subclass_stats.base_rng_def / 4;
            resulting_stats.base_tec_def += subclass_stats.base_tec_def / 4;
        }

        if let Some(equiped_item) = char.palette.get_current_item(&char.inventory)? {
            let ids = equiped_item.id;
            let weapon_stats = server_data
                .item_params
                .attrs
                .weapons
                .iter()
                .find(|a| a.id == ids.id && a.subid == ids.subid)
                .cloned()
                .ok_or(Error::NoItemInAttrs(ids.id, ids.subid))?;
            resulting_stats.weapon_mel_pwr = weapon_stats.melee_dmg as _;
            resulting_stats.weapon_rng_pwr = weapon_stats.range_dmg as _;
            resulting_stats.weapon_tec_pwr = weapon_stats.gender_force_dmg.force_dmg as _;
        }
        Ok(resulting_stats)
    }
    fn calculate_class_stats(user: &User, class: usize, level: usize) -> Self {
        let Some(char) = &user.character else {
            unreachable!("User should be in state >= `PreInGame`")
        };
        let mut resulting_stats = Self::default();
        let player_stats = &user.get_blockdata().server_data.player_stats;

        let stats = &player_stats.stats[class][level - 1];

        let char_data = &char.character;
        let modifier_offset = char_data.look.race as usize * 2 + char_data.look.gender as usize;
        let modifiers = &player_stats.modifiers[modifier_offset];

        resulting_stats.hp = (stats.hp + (stats.hp * 0.01 * modifiers.hp as f32).floor()) as _;
        resulting_stats.max_hp = resulting_stats.hp;
        resulting_stats.dex = (stats.dex + (stats.dex * 0.01 * modifiers.dex as f32).floor()) as _;
        resulting_stats.base_mel_pwr =
            (stats.mel_pow + (stats.mel_pow * 0.01 * modifiers.mel_pow as f32).floor()) as _;
        resulting_stats.base_rng_pwr =
            (stats.rng_pow + (stats.rng_pow * 0.01 * modifiers.rng_pow as f32).floor()) as _;
        resulting_stats.base_tec_pwr =
            (stats.tec_pow + (stats.tec_pow * 0.01 * modifiers.tec_pow as f32).floor()) as _;
        resulting_stats.base_mel_def =
            (stats.mel_def + (stats.mel_def * 0.01 * modifiers.mel_def as f32).floor()) as _;
        resulting_stats.base_rng_def =
            (stats.rng_def + (stats.rng_def * 0.01 * modifiers.rng_def as f32).floor()) as _;
        resulting_stats.base_tec_def =
            (stats.tec_def + (stats.tec_def * 0.01 * modifiers.tec_def as f32).floor()) as _;

        resulting_stats
    }
    pub fn update(player: &mut User) -> Result<(), Error> {
        let old_hp = player.get_stats().hp;
        let mut new_stats = Self::build(player)?;
        new_stats.hp = old_hp;
        *player.get_stats_mut() = new_stats;

        Ok(())
    }
    pub const fn get_hp(&self) -> (u32, u32) {
        (self.hp, self.max_hp)
    }
    /// Nova3K: heal by a percentage of max HP; returns (healed amount, new HP).
    pub fn nova3k_heal(&mut self, percent: u32) -> (u32, u32) {
        let old = self.hp;
        let add = (self.max_hp as u64 * percent as u64 / 100) as u32;
        self.hp = self.hp.saturating_add(add.max(1)).min(self.max_hp);
        (self.hp - old, self.hp)
    }
    /// Nova3K: back to full HP (return to the campship / lobby after death).
    pub fn nova3k_full_heal(&mut self) {
        self.hp = self.max_hp;
    }
    pub fn damage_enemy(
        &mut self,
        enemy: &mut EnemyStats,
        srv_data: &ServerData,
        attack: DealDamagePacket,
    ) -> Result<BattleResult, Error> {
        // Nova3K: unknown attacks (e.g. Rifle:PA2/PA3) no longer drop the hit, see nova3k_fallback_attack
        let damage = match srv_data
            .attack_stats
            .iter()
            .find(|a| a.attack_id == attack.attack_id)
            .cloned()
        {
            Some(d) => d,
            None => nova3k_fallback_attack(
                attack.attack_id,
                [self.weapon_mel_pwr, self.weapon_rng_pwr, self.weapon_tec_pwr],
            ),
        };
        let Some(hitbox) = enemy
            .hitboxes
            .iter()
            .find(|h| h.hitbox_id == attack.hitbox_id)
            .cloned()
        else {
            return Err(Error::NoHitboxInfo(
                enemy.name.to_string(),
                attack.hitbox_id,
            ));
        };
        let (base_pwr, weapon_pwr, part_mul) = match damage.attack_type {
            data_structs::stats::AttackType::Mel => {
                (self.base_mel_pwr, self.weapon_mel_pwr, hitbox.mel_mul)
            }
            data_structs::stats::AttackType::Rng => {
                (self.base_rng_pwr, self.weapon_rng_pwr, hitbox.rng_mul)
            }
            data_structs::stats::AttackType::Tec => {
                (self.base_tec_pwr, self.weapon_tec_pwr, hitbox.tec_mul)
            }
        };
        let def = match damage.defense_type {
            data_structs::stats::AttackType::Mel => enemy.mel_def,
            data_structs::stats::AttackType::Rng => enemy.rng_def,
            data_structs::stats::AttackType::Tec => enemy.tec_def,
        };
        let total_mul = 1.0 * hitbox.damage_mul;
        let min_pure_attack =
            (base_pwr as f32 + weapon_pwr as f32 * 0.9 - def as f32).clamp(1.0, f32::MAX);
        let pure_attack = (base_pwr + weapon_pwr)
            .saturating_sub(def)
            .clamp(1, u32::MAX) as f32;
        let damage_mul = match damage.damage {
            data_structs::stats::DamageType::Generic(m) => m,
            data_structs::stats::DamageType::PA(_) => todo!(),
        };
        let min_weapon_attack = min_pure_attack / 5.0 * 1.05 * part_mul * damage_mul * total_mul;
        let max_weapon_attack = pure_attack / 5.0 * 1.05 * part_mul * damage_mul * total_mul;

        //TODO: elemental dmg

        let mut rng = rand::rng();
        let crit_chance = Uniform::new(0, 100).unwrap().sample(&mut rng);
        let dmg = if crit_chance < 5 || min_weapon_attack == max_weapon_attack {
            max_weapon_attack
        } else {
            Uniform::new(min_weapon_attack, max_weapon_attack)
                .unwrap()
                .sample(&mut rng)
        }
        .round() as u32;
        enemy.hp = enemy.hp.saturating_sub(dmg);
        let dmg_packet = DamageReceivePacket {
            dmg_target: attack.target,
            dmg_inflicter: attack.inflicter,
            damage_id: damage.damage_id,
            dmg_amount: dmg as _,
            new_hp: enemy.hp,
            hitbox_id: attack.hitbox_id,
            x_pos: attack.x_pos,
            y_pos: attack.y_pos,
            z_pos: attack.z_pos,
            ..Default::default()
        };
        Ok(if enemy.hp == 0 {
            let kill_packet = EnemyKilledPacket {
                receiver: dmg_packet.receiver,
                dmg_target: dmg_packet.dmg_target,
                dmg_inflicter: dmg_packet.dmg_inflicter,
                damage_id: dmg_packet.damage_id,
                dmg_amount: dmg_packet.dmg_amount,
                new_hp: dmg_packet.new_hp,
                hitbox_id: dmg_packet.hitbox_id,
                x_pos: dmg_packet.x_pos,
                y_pos: dmg_packet.y_pos,
                z_pos: dmg_packet.z_pos,
                unk1: dmg_packet.unk1,
                unk2: dmg_packet.unk3,
                unk3: dmg_packet.unk4,
                unk4: dmg_packet.unk4,
                unk5: dmg_packet.unk5,
                unk6: dmg_packet.unk6,
                unk7: dmg_packet.unk7,
            };
            BattleResult::Killed {
                dmg_packet,
                kill_packet,
                exp_amount: enemy.exp,
            }
        } else {
            BattleResult::Damaged { dmg_packet }
        })
    }
}

impl EnemyStats {
    /// Nova3K: name / level / spawn position (respawning an enemy that fell out of the map).
    pub fn nova3k_name(&self) -> &str {
        &self.name
    }
    pub const fn nova3k_level(&self) -> u32 {
        self.level
    }
    pub const fn nova3k_pos(&self) -> Position {
        self.pos
    }
    pub fn build(name: &str, level: u32, pos: Position, data: &ServerData) -> Result<Self, Error> {
        let mut resulting_stats = Self {
            name: name.to_string(),
            pos,
            ..Default::default()
        };
        let base_stats = &data.enemy_stats.base;
        let enemy_stats = &data
            .enemy_stats
            .enemies
            .get(name)
            .ok_or(Error::NoEnemyData(name.to_string()))?;
        resulting_stats.hitboxes.clone_from(&enemy_stats.hitboxes);
        let base_level_stats = &base_stats.levels[level as usize - 1];
        let level_stats = &enemy_stats.levels[level as usize - 1];

        resulting_stats.level = level_stats.level;
        resulting_stats.exp = (base_level_stats.exp * level_stats.exp).floor() as _;
        resulting_stats.max_hp = (base_level_stats.hp * level_stats.hp).floor() as _;
        resulting_stats.hp = resulting_stats.max_hp;
        resulting_stats.dex = (base_level_stats.dex * level_stats.dex).floor() as _;
        resulting_stats.max_mel_pwr =
            (base_level_stats.max_mel_dmg * level_stats.max_mel_dmg).floor() as _;
        resulting_stats.min_mel_pwr =
            (base_level_stats.min_mel_dmg * level_stats.min_mel_dmg).floor() as _;
        resulting_stats.max_rng_pwr =
            (base_level_stats.max_rng_dmg * level_stats.max_rng_dmg).floor() as _;
        resulting_stats.min_rng_pwr =
            (base_level_stats.min_rng_dmg * level_stats.min_rng_dmg).floor() as _;
        resulting_stats.max_tec_pwr =
            (base_level_stats.max_tec_dmg * level_stats.max_tec_dmg).floor() as _;
        resulting_stats.min_tec_pwr =
            (base_level_stats.min_tec_dmg * level_stats.min_tec_dmg).floor() as _;
        resulting_stats.mel_def = (base_level_stats.mel_def * level_stats.mel_def).floor() as _;
        resulting_stats.rng_def = (base_level_stats.rng_def * level_stats.rng_def).floor() as _;
        resulting_stats.tec_def = (base_level_stats.tec_def * level_stats.tec_def).floor() as _;

        Ok(resulting_stats)
    }
    /// Nova3K: boss version of a normal enemy: `mul` times the HP (the stat files are shared by all
    /// quests, so a boss is made at spawn time).
    pub fn nova3k_boost(&mut self, mul: u32) {
        self.max_hp = self.max_hp.saturating_mul(mul.max(1));
        self.hp = self.max_hp;
    }
    pub fn create_spawn_packet(&self, id: u32) -> EnemySpawnPacket {
        EnemySpawnPacket {
            object: pso2packetlib::protocol::ObjectHeader {
                id,
                entity_type: pso2packetlib::protocol::ObjectType::Object,
                ..Default::default()
            },
            position: self.pos,
            name: self.name.to_string().into(),
            hp: self.hp,
            level: self.level,
            unk2: 2,
            unk4: 2,
            // unk5: 269877691,

            // contains emergency marker color
            // 0x505 - blue
            // 0x605 - purple
            // 0x705 - green
            // unk6: 0x405,
            // 0x1 - ??
            // 0x2 - ??
            // 0x4 - ??
            // 0x8 - ??
            // 0x10 - ??
            // 0x20 - ??
            // 0x40 - big enemy marker
            // 0x80 - hide enemy from minimap
            // 0x300 - emergency color
            // 0x400 - ??
            unk6: 0x405,
            unk7: 255,
            unk8: 65535,
            // last nonzero value - model id
            // Nova3K: every enemy appeared as a Dagan with this fixed value (taken from a captured Dagan
            // spawn); `!model` changes it for tests, nova3k_model_id() holds the known values.
            unk9: [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 1062804813, 3212836864, 0, nova3k_model_id(&self.name), 0, 0, 0,
            ],
            unk11: 1,
            unk12: 255,
            ..Default::default()
        }
    }
    pub fn damage_player(
        &mut self,
        player: &mut PlayerStats,
        srv_data: &ServerData,
        attack: DealDamagePacket,
    ) -> Result<BattleResult, Error> {
        // Nova3K: unknown enemy attacks hit as a plain melee attack (see nova3k_fallback_attack)
        let damage = match srv_data
            .attack_stats
            .iter()
            .find(|a| a.attack_id == attack.attack_id)
            .cloned()
        {
            Some(d) => d,
            None => nova3k_fallback_attack(attack.attack_id, [1, 0, 0]),
        };
        let (min_pwr, max_pwr) = match damage.attack_type {
            data_structs::stats::AttackType::Mel => (self.min_mel_pwr, self.max_mel_pwr),
            data_structs::stats::AttackType::Rng => (self.min_rng_pwr, self.max_rng_pwr),
            data_structs::stats::AttackType::Tec => (self.min_tec_pwr, self.max_tec_pwr),
        };
        let def = match damage.defense_type {
            data_structs::stats::AttackType::Mel => player.base_mel_def,
            data_structs::stats::AttackType::Rng => player.base_rng_def,
            data_structs::stats::AttackType::Tec => player.base_tec_def,
        };
        let total_mul = 1.0;
        let min_pure_attack = min_pwr.saturating_sub(def).clamp(1, u32::MAX) as f32;
        let pure_attack = max_pwr.saturating_sub(def).clamp(1, u32::MAX) as f32;
        let damage_mul = match damage.damage {
            data_structs::stats::DamageType::Generic(m) => m,
            data_structs::stats::DamageType::PA(_) => unimplemented!(),
        };
        let min_weapon_attack = min_pure_attack / 5.0 * 1.05 * damage_mul * total_mul;
        let max_weapon_attack = pure_attack / 5.0 * 1.05 * damage_mul * total_mul;

        //TODO: elemental res

        let mut rng = rand::rng();
        let crit_chance = Uniform::new(0, 100).unwrap().sample(&mut rng);
        let dmg = if crit_chance < 5 || min_weapon_attack == max_weapon_attack {
            max_weapon_attack
        } else {
            Uniform::new(min_weapon_attack, max_weapon_attack)
                .unwrap()
                .sample(&mut rng)
        }
        .round() as u32;
        player.hp = player.hp.saturating_sub(dmg);
        let dmg_packet = DamageReceivePacket {
            dmg_target: attack.target,
            dmg_inflicter: attack.inflicter,
            damage_id: damage.damage_id,
            dmg_amount: dmg as _,
            new_hp: player.hp,
            hitbox_id: attack.hitbox_id,
            x_pos: attack.x_pos,
            y_pos: attack.y_pos,
            z_pos: attack.z_pos,
            ..Default::default()
        };
        Ok(if player.hp == 0 {
            let kill_packet = EnemyKilledPacket {
                receiver: dmg_packet.receiver,
                dmg_target: dmg_packet.dmg_target,
                dmg_inflicter: dmg_packet.dmg_inflicter,
                damage_id: dmg_packet.damage_id,
                dmg_amount: dmg_packet.dmg_amount,
                new_hp: dmg_packet.new_hp,
                hitbox_id: dmg_packet.hitbox_id,
                x_pos: dmg_packet.x_pos,
                y_pos: dmg_packet.y_pos,
                z_pos: dmg_packet.z_pos,
                unk1: dmg_packet.unk1,
                unk2: dmg_packet.unk3,
                unk3: dmg_packet.unk4,
                unk4: dmg_packet.unk4,
                unk5: dmg_packet.unk5,
                unk6: dmg_packet.unk6,
                unk7: dmg_packet.unk7,
            };
            BattleResult::Killed {
                dmg_packet,
                kill_packet,
                exp_amount: 0,
            }
        } else {
            BattleResult::Damaged { dmg_packet }
        })
    }
}

/// Nova3K: stand-in for an attack that is not in data/attack_stats (most player PAs, techniques and
/// enemy attacks are not described yet). Multiplier 1.0; the attack type is the one with the highest
/// weapon power (= the type of the equipped weapon) for players, melee for enemies. Each unknown id is
/// logged once, so the table can be completed later (id = data_structs::name_to_id(attack name)).
fn nova3k_fallback_attack(attack_id: u32, weapon_pwr: [u32; 3]) -> data_structs::stats::AttackStats {
    use data_structs::stats::{AttackStats, AttackType, DamageType};
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static LOGGED: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    let attack_type = if weapon_pwr[1] >= weapon_pwr[0] && weapon_pwr[1] >= weapon_pwr[2] {
        AttackType::Rng
    } else if weapon_pwr[2] > weapon_pwr[0] {
        AttackType::Tec
    } else {
        AttackType::Mel
    };
    if let Ok(mut set) = LOGGED.get_or_init(|| Mutex::new(HashSet::new())).lock() {
        if set.insert(attack_id) {
            log::info!("Nova3K damage: attack id {attack_id} not in attack_stats, using {attack_type:?} x1.0");
        }
    }
    AttackStats {
        attack_id,
        damage_id: attack_id,
        attack_type: attack_type.clone(),
        defense_type: attack_type,
        damage: DamageType::Generic(1.0),
    }
}

/// Nova3K: test override of the enemy spawn value unk9[12] (`!model` chat command); u64::MAX = none.
pub static NOVA3K_MODEL_OVERRIDE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);

/// Nova3K: value unk9[12] of the enemy spawn packet (0x08 0x0D). It is the key of the enemy's record in the
/// client's enemy table (the memory search found 0x5DA08B21 in the record of `SoldierAnt` = Dagan): PhantasyServer
/// sent the Dagan key for every enemy, so every enemy was a Dagan. With 0 the client finds the enemy by the name
/// in the same packet - tested in game (0b2b69f, `!model 0`): the Forest enemies got their own models.
pub fn nova3k_model_id(_name: &str) -> u32 {
    let o = NOVA3K_MODEL_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed);
    if o != u64::MAX {
        return o as u32;
    }
    0
}
