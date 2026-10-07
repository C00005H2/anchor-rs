use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::info;

use crate::{
    data_loader::GameDataLoader,
    messages::{
        pt_prop_bag, CS_MAIL_ENCLOSURE_REC, CS_MAIL_READ, SC_BAG_UPDATE, SC_MAIL_ADD,
        SC_MAIL_ENCLOSURE_REC, SC_MAIL_LIST, SC_MAIL_READ,
    },
    packet::build_server_packet,
    progression::{unread_packets, UnreadNotice},
    state::ConnectionContext,
};

/// Mail snapshot used by the claim flows; the biography sequence serves the
/// same file, so there is exactly one mail list per data directory.
const MAIL_LIST_DATA: &str = "hero_biography/mail_list.json";

/// `SC_NEW_UNREAD` notices sent when enclosure rewards are claimed.
const ENCLOSURE_UNREAD_DATA: &str = "mail/enclosure_unread.json";

const FLOW_MAIL_READ: &str = "mail_read";
const FLOW_MAIL_CLAIMED: &str = "mail_claimed";

fn load_mail_list() -> Result<SC_MAIL_LIST, anyhow::Error> {
    GameDataLoader::load_struct(MAIL_LIST_DATA)
}

/// Handle CS_MAIL_READ (16005) -> SC_MAIL_READ.
///
/// Unknown mail ids are dropped instead of echoed back.
pub async fn handle_mail_read(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_MAIL_READ,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let mails = load_mail_list()?;
    let known: HashSet<i32> = mails.mail_list.iter().map(|mail| mail.id).collect();
    let accepted: Vec<i32> = request
        .mail_id_list
        .into_iter()
        .filter(|id| known.contains(id))
        .collect();

    {
        let mut connection = ctx.lock().await;
        for mail_id in &accepted {
            connection.claim_once(FLOW_MAIL_READ, i64::from(*mail_id));
        }
    }

    info!(mails = accepted.len(), "Marked mails read");
    Ok(vec![build_server_packet(
        16006,
        &SC_MAIL_READ {
            mail_id_list: accepted,
        }
        .encode(),
    )?])
}

/// Handle CS_MAIL_ENCLOSURE_REC (16007): claim mail attachments.
///
/// Claiming grants each mail's attachment items to the local bag (stacks merge
/// by template id), reports the claim result, and re-sends the claimed mails
/// with `state = 2`.  Mails without attachments, unknown mails and mails that
/// were already claimed contribute nothing.
pub async fn handle_mail_enclosure_rec(
    ctx: Arc<Mutex<ConnectionContext>>,
    request: CS_MAIL_ENCLOSURE_REC,
) -> Result<Vec<Vec<u8>>, anyhow::Error> {
    let mails = load_mail_list()?;
    let notices: Vec<UnreadNotice> = GameDataLoader::load_struct(ENCLOSURE_UNREAD_DATA).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Failed to load mail enclosure unread notices; using empty list");
        Vec::new()
    });

    let mut packets = unread_packets(&notices)?;
    let mut granted: Vec<pt_prop_bag> = Vec::new();
    let mut claimed: Vec<crate::messages::pt_mail_info> = Vec::new();
    let now = chrono::Utc::now().timestamp() as i32;

    {
        let mut connection = ctx.lock().await;
        for mail_id in &request.mail_id_list {
            let Some(mail) = mails.mail_list.iter().find(|entry| entry.id == *mail_id) else {
                continue;
            };
            if mail.award_list.is_empty()
                || connection.is_claimed(FLOW_MAIL_CLAIMED, i64::from(*mail_id))
            {
                continue;
            }
            connection.claim_once(FLOW_MAIL_CLAIMED, i64::from(*mail_id));
            connection.claim_once(FLOW_MAIL_READ, i64::from(*mail_id));

            for award in &mail.award_list {
                let (id, count, color) =
                    connection.grant_bag_item(award.tid, award.count, award.color);
                granted.push(pt_prop_bag {
                    id,
                    tid: award.tid,
                    count,
                    createdTime: now,
                    expiredTime: award.expiredTime,
                    color,
                    is_lock: award.is_lock,
                    hero_id: award.hero_id,
                    strength_lv: award.strength_lv,
                    strength_exp: award.strength_exp,
                    breakup_rank: award.breakup_rank,
                    refine_lv: award.refine_lv,
                    remake_attr: award.remake_attr.clone(),
                    bracelet_remake_attr: award.bracelet_remake_attr.clone(),
                    filter_base_attr: Vec::new(),
                    filter_subjoin_attr: Vec::new(),
                    empower_slot_info: award.empower_slot_info.clone(),
                });
            }

            let mut updated = mail.clone();
            updated.state = 2;
            claimed.push(updated);
        }
    }

    if !granted.is_empty() {
        packets.push(build_server_packet(
            17001,
            &SC_BAG_UPDATE {
                msg_type: 1,
                updateList: granted,
                delList: Vec::new(),
            }
            .encode(),
        )?);
    }

    let result = if claimed.is_empty() { 1 } else { 0 };
    info!(
        claimed = claimed.len(),
        result = result,
        "Processed mail enclosure claim"
    );

    packets.push(build_server_packet(
        16008,
        &SC_MAIL_ENCLOSURE_REC {
            rec_result: result,
            mail_id_list: request.mail_id_list.clone(),
        }
        .encode(),
    )?);

    for mail in claimed {
        packets.push(build_server_packet(
            16002,
            &SC_MAIL_ADD { mail_info: mail }.encode(),
        )?);
    }

    Ok(packets)
}
