use super::HResult;
use crate::{Action, User, mutex::MutexGuard};
use pso2packetlib::protocol::{Packet, objects};

pub async fn movement(mut user: MutexGuard<'_, User>, packet: objects::MovementPacket) -> HResult {
    // Nova3K: the client also reports the movement of enemies it simulates (entity type 6 = object).
    // Those must not move the player's own position (enemy spawn spots are picked around it); they go to
    // the map, which respawns an enemy that keeps falling out of the level.
    if packet.ent1_type == Some(6) {
        if let Some(enemy_id) = packet.ent1_id {
            nova3k_log_enemy_movement(&packet);
            let map = user.get_current_map();
            let zone = user.zone_pos;
            let pid = user.get_user_id();
            let pos = (packet.cur_x, packet.cur_y, packet.cur_z);
            drop(user);
            if let Some(map) = map {
                let mut lock = map.lock().await;
                lock.nova3k_enemy_moved(zone, pid, enemy_id as u32, pos).await;
                lock.send_movement(zone, Packet::Movement(packet), pid).await;
            }
            return Ok(Action::Nothing);
        }
    }
    if let Some(n) = packet.rot_x {
        user.position.rot_x = n;
    }
    if let Some(n) = packet.rot_y {
        user.position.rot_y = n;
    }
    if let Some(n) = packet.rot_z {
        user.position.rot_z = n;
    }
    if let Some(n) = packet.rot_w {
        user.position.rot_w = n;
    }
    if let Some(n) = packet.cur_x {
        user.position.pos_x = n;
    }
    if let Some(n) = packet.cur_y {
        user.position.pos_y = n;
    }
    if let Some(n) = packet.cur_z {
        user.position.pos_z = n;
    }
    // Nova3K: breadcrumb trail of walkable points (a new point every 4 m, last 96 points)
    let here = user.position;
    let far = user.nova3k_trail.last().is_none_or(|p| {
        let dx = p.pos_x.to_f32() - here.pos_x.to_f32();
        let dy = p.pos_y.to_f32() - here.pos_y.to_f32();
        let dz = p.pos_z.to_f32() - here.pos_z.to_f32();
        dx * dx + dy * dy + dz * dz > 16.0
    });
    if far && packet.cur_x.is_some() {
        if user.nova3k_trail.len() >= 96 {
            user.nova3k_trail.remove(0);
        }
        user.nova3k_trail.push(here);
    }
    // Nova3K: fell through the level (25 m below the zone start): reload the zone at a safe spot
    if let (Some(start_y), Some(_)) = (user.nova3k_start_y, packet.cur_y) {
        let fell = here.pos_y.to_f32() < start_y - 25.0;
        let recent = user.nova3k_last_rescue.is_some_and(|t| t.elapsed().as_secs() < 8);
        if fell && !recent {
            user.nova3k_last_rescue = Some(std::time::Instant::now());
            let map = user.get_current_map();
            let id = user.get_user_id();
            drop(user);
            if let Some(map) = map {
                map.lock().await.nova3k_rescue(id, "fall").await?;
            }
            return Ok(Action::Nothing);
        }
    }
    User::send_position(user, Packet::Movement(packet)).await
}

/// Nova3K: player actions (0x04 0x08). Using a consumable plays an action whose name the server never
/// answered, so the HP did not change (Monomate animation, no healing). Every new action name is logged once
/// (to learn the real names); a name that looks like item use / healing heals 50% of max HP and the new HP
/// is sent back as a 0x04 0x52 DamageReceive with a negative amount.
pub async fn nova3k_movement_action(
    mut user: MutexGuard<'_, User>,
    packet: objects::MovementActionPacket,
) -> HResult {
    let action = format!("{}", packet.action);
    {
        use std::sync::{Mutex, OnceLock};
        static SEEN: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
        let mut seen = SEEN.get_or_init(Default::default).lock().unwrap();
        if seen.len() < 300 && seen.insert(action.clone()) {
            log::info!("Nova3K action: '{action}' unk3 {} unk7 {} unk8 {} unk9 {:?} unk10 {}",
                packet.unk3, packet.unk7, packet.unk8, packet.unk9, packet.unk10);
        }
    }
    let lower = action.to_ascii_lowercase();
    if ["item", "mate", "heal", "recover", "drink"].iter().any(|k| lower.contains(k)) {
        let percent = if lower.contains("tri") { 100 } else if lower.contains("di") { 75 } else { 50 };
        let (healed, new_hp) = user.get_stats_mut().nova3k_heal(percent);
        let me = user.create_object_header();
        log::info!("Nova3K heal: action '{action}' +{healed} HP -> {new_hp}");
        let heal = Packet::DamageReceive(objects::DamageReceivePacket {
            receiver: me,
            dmg_target: me,
            dmg_inflicter: me,
            dmg_amount: -(healed as i32),
            new_hp,
            ..Default::default()
        });
        let _ = user.try_send_packet(&heal);
        // the used item leaves the inventory (Monomate 3/1/0/0; Dimate / Trimate assumed subid 1 / 2)
        let subid = if lower.contains("trimate") { 2 } else if lower.contains("dimate") { 1 } else { 0 };
        let item_id = pso2packetlib::protocol::items::ItemId { item_type: 3, id: 1, subid, ..Default::default() };
        let user_ref: &mut User = &mut user;
        if let Some(character) = user_ref.character.as_mut() {
            match character.inventory.nova3k_use_consumable(item_id) {
                Some(p) => {
                    let _ = user_ref.try_send_packet(&p);
                }
                None => log::info!("Nova3K heal: no {item_id:?} in the inventory to take"),
            }
        }
    }
    User::send_position(user, Packet::MovementAction(packet)).await
}

/// Nova3K: a few enemy movement packets in the log (is the falling enemy reported, and how).
fn nova3k_log_enemy_movement(packet: &objects::MovementPacket) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNT: AtomicU32 = AtomicU32::new(0);
    if COUNT.fetch_add(1, Ordering::Relaxed) % 200 == 0 {
        log::info!(
            "Nova3K enemy movement: ent1 {:?}/{:?} ent2 {:?}/{:?} pos {:?} {:?} {:?}",
            packet.ent1_id, packet.ent1_type, packet.ent2_id, packet.ent2_type,
            packet.cur_x.map(|v| v.to_f32()), packet.cur_y.map(|v| v.to_f32()), packet.cur_z.map(|v| v.to_f32())
        );
    }
}

pub async fn action(user: MutexGuard<'_, User>, packet: objects::InteractPacket) -> HResult {
    let id = user.get_user_id();
    let map = user.get_current_map();
    let zone = user.zone_pos;
    drop(user);
    if let Some(map) = map {
        map.lock().await.interaction(zone, packet, id).await?;
    }
    Ok(Action::Nothing)
}

pub async fn change_class(
    mut user: MutexGuard<'_, User>,
    packet: objects::ChangeClassRequestPacket,
) -> HResult {
    let Some(char) = user.character.as_mut() else {
        unreachable!("Character should be already setup");
    };
    char.character.classes.main_class = packet.main_class;
    char.character.classes.sub_class = packet.sub_class;

    let packet = Packet::ChangeClass(objects::ChangeClassPacket {
        new_info: char.character.classes.clone(),
        receiver: user.create_object_header(),
        player: user.create_object_header(),
        unk3: Default::default(),
    });
    user.send_packet(&packet).await?;
    let packet = Packet::Unk042C(objects::Unk042CPacket {
        unk1: user.create_object_header(),
        unk2: user.create_object_header(),
        unk3: 0x20,
        unk4: 0x1,
        ..Default::default()
    });
    user.send_packet(&packet).await?;

    Ok(Action::Nothing)
}
