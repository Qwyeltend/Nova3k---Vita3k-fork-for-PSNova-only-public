use super::HResult;
use crate::{Action, User, mutex::MutexGuard, quests::PartyQuest};
use pso2packetlib::protocol::{
    Packet, PacketHeader,
    flag::{CutsceneEndPacket, SkitItemAddRequestPacket},
    questlist::{
        self, AcceptQuestPacket, AcceptStoryQuestPacket, MinimapRevealRequestPacket,
        NewUnlockedQuestsPacket, QuestCategoryRequestPacket, QuestDifficultyPacket,
        QuestDifficultyRequestPacket, UnlockedQuest,
    },
};

pub async fn counter_request(user: &mut User) -> HResult {
    log::info!("Nova3K quests: quest counter opened");
    let data = vec![
        0x78, 0x00, 0x78, 0x00, 0x00, 0x00, 0x04, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0xF4, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xAC, 0x0D,
        0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0xB8, 0x0B, 0x00, 0x00,
    ];
    user.send_packet(&Packet::Unknown((
        PacketHeader {
            id: 0x49,
            subid: 0x01,
            ..Default::default()
        },
        data,
    )))
    .await?;
    let data = vec![
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x78, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    ];
    // Nova3K (experiment): 0x0E 0x65 is sent with the counter and its first u32 is 1 in the captured data;
    // the "Emergency Quests" item then said "no quests" and the counter hung. Read as "1 emergency quest
    // active" (that does not exist offline) -> send 0.
    let mut data = data;
    data[0] = 0;
    user.send_packet(&Packet::Unknown((
        PacketHeader {
            id: 0x0E,
            subid: 0x65,
            ..Default::default()
        },
        data,
    )))
    .await?;
    let quests = user.blockdata.quests.clone();
    let char = user
        .character
        .as_mut()
        .expect("Character should be loaded at this moment");
    // 51 is the size of the array inside NewUnlockedQuestsPacket
    let max_unlocks = char.unlocked_quests_notif.len().min(51);
    let unlocks: Vec<_> = char
        .unlocked_quests_notif
        .drain(..max_unlocks)
        .filter_map(|id| quests.get_quest_by_nameid(id))
        .map(|q| UnlockedQuest {
            name_id: q.definition.name_id,
            quest_type: q.definition.quest_type,
            ..Default::default()
        })
        .collect();
    let packet = Packet::NewUnlockedQuests(NewUnlockedQuestsPacket {
        unlocks: unlocks.into(),
    });
    user.send_packet(&packet).await?;
    Ok(Action::Nothing)
}

pub async fn avaliable_quests(
    user: &mut User,
    _: questlist::AvailableQuestsRequestPacket,
) -> HResult {
    let char = user
        .character
        .as_ref()
        .expect("Character should be loaded at this moment");
    let real = user.blockdata.quests.get_availiable(&char.unlocked_quests);
    log::info!("Nova3K quests: available quests requested, {} unlocked quests, real answer: {:?}", char.unlocked_quests.len(), real);
    // Nova3K: the counts are in the right fields for the 6.31 client (checked with a numbered probe:
    // limited 5, Free Field = expedition 13, Free (Debug) = expedition_debug 14, Bonus 37), but with only
    // the STORY_EP1 | EXPEDITION type bits the client listed nothing: the Vita type bits are laid out
    // differently. All type bits are set; only the real counts decide which categories appear.
    let mut answer = real;
    answer.available_types = questlist::AvailableQuestType::from_bits_retain(u128::MAX);
    answer.unk19 = questlist::AvailableQuestType::from_bits_retain(u128::MAX);
    let packet = Packet::AvailableQuests(answer);
    log::info!("Nova3K quests: answer sent: {:?}", packet);
    user.send_packet(&packet).await?;
    Ok(Action::Nothing)
}

pub async fn quest_category(user: &mut User, packet: QuestCategoryRequestPacket) -> HResult {
    log::info!("Nova3K quests: category {:?} requested", packet.category);
    let char = user
        .character
        .as_ref()
        .expect("Character should be loaded at this moment");
    let packet = user
        .blockdata
        .quests
        .get_category(packet.category, &char.unlocked_quests);
    user.send_packet(&Packet::QuestCategory(packet)).await?;
    user.send_packet(&Packet::QuestCategoryStopper).await?;

    Ok(Action::Nothing)
}

pub async fn quest_difficulty(user: &mut User, packet: QuestDifficultyRequestPacket) -> HResult {
    log::info!("Nova3K quests: difficulty requested for {} quests", packet.quests.len());
    for quest in packet.quests {
        let diff = user.blockdata.quests.get_diff(quest.id);
        if let Some(packet) = diff {
            user.send_packet(&Packet::QuestDifficulty(QuestDifficultyPacket {
                quests: vec![packet],
            }))
            .await?;
        }
    }
    user.send_packet(&Packet::QuestDifficultyStopper).await?;
    Ok(Action::Nothing)
}

pub async fn set_quest(user: MutexGuard<'_, User>, packet: AcceptQuestPacket) -> HResult {
    log::info!("Nova3K quests: accept quest {:?}", packet);
    let quest = user
        .blockdata
        .quests
        .get_quest(packet, &user.blockdata.latest_mapid)?;
    start_quest(user, quest).await
}

pub async fn questwork(user: MutexGuard<'_, User>, packet: SkitItemAddRequestPacket) -> HResult {
    if let Some(map) = user.get_current_map() {
        let playerid = user.get_user_id();
        let zone = user.zone_pos;
        drop(user);
        map.lock()
            .await
            .on_questwork(zone, playerid, packet)
            .await?;
    }

    Ok(Action::Nothing)
}

pub async fn cutscene_end(user: MutexGuard<'_, User>, packet: CutsceneEndPacket) -> HResult {
    if let Some(map) = user.get_current_map() {
        let playerid = user.get_user_id();
        let zone = user.zone_pos;
        drop(user);
        map.lock()
            .await
            .on_cutscene_end(zone, playerid, packet)
            .await?;
    }

    Ok(Action::Nothing)
}

pub async fn set_story_quest(
    user: MutexGuard<'_, User>,
    packet: AcceptStoryQuestPacket,
) -> HResult {
    let quest = user
        .blockdata
        .quests
        .get_story_quest(packet, &user.blockdata.latest_mapid)?;
    start_quest(user, quest).await
}

pub async fn start_quest(user: MutexGuard<'_, User>, quest: PartyQuest) -> HResult {
    let is_insta = quest.is_insta_transfer();
    let user_id = user.get_user_id();
    let old_map = user.get_current_map().expect("User should have a map");
    let map = quest.get_map();
    // we are the only owner of the map, so this never blocks
    map.lock_blocking().set_block_data(user.blockdata.clone());
    // Nova3K: generated quests scale with the player: enemy level = main class level (boss +2, clear reward
    // 100 + 25 x level, kill meseta grows with the enemy level)
    if quest.nova3k_generated() {
        let level = user
            .character
            .as_ref()
            .map(|c| c.character.get_level().level1 as u32)
            .unwrap_or(1)
            .max(1);
        log::info!("Nova3K quests: generated quest, enemy level = player level {level}");
        map.lock_blocking().set_enemy_level(level);
    }
    let party = user.get_current_party();
    drop(user);
    if let Some(party) = party {
        party.write().await.set_quest(quest).await;
    }
    if is_insta {
        let mut lock = old_map.lock().await;
        let player = lock
            .remove_player(user_id)
            .await
            .expect("User should exist");
        drop(lock);
        player.lock().await.set_map(map.clone());
        let mut lock = map.lock().await;
        lock.init_add_player(player).await?;
    }
    Ok(Action::Nothing)
}

pub async fn minimap_reveal(
    mut user: MutexGuard<'_, User>,
    data: MinimapRevealRequestPacket,
) -> HResult {
    user.send_packet(&Packet::SystemMessage(
        pso2packetlib::protocol::unk19::SystemMessagePacket {
            message: format!(
                "Chunk ID: {}, {}:{}",
                data.chunk_id, data.map_row, data.map_column
            ),
            msg_type: pso2packetlib::protocol::unk19::MessageType::EventInformationYellow,
            ..Default::default()
        },
    ))
    .await?;
    // Nova3K: real chunk ids of the generated quests are not known yet; this line in vita3k.log gives
    // the table chunk -> map cell -> player position for tools/quest_gen.py
    log::info!(
        "Nova3K chunk: zone_pos {} chunk {} row {} column {} player {:.1} {:.1} {:.1}",
        user.zone_pos,
        data.chunk_id,
        data.map_row,
        data.map_column,
        user.position.pos_x.to_f32(),
        user.position.pos_y.to_f32(),
        user.position.pos_z.to_f32()
    );
    if let Some(map) = user.get_current_map() {
        let playerid = user.get_user_id();
        let zone = user.zone_pos;
        drop(user);
        map.lock()
            .await
            .minimap_reveal(zone, playerid, data)
            .await?;
    }
    Ok(Action::Nothing)
}

