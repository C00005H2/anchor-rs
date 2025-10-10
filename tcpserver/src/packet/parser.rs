use crypto::des;
use super::constants::ProtocolConst;

/// Parse client→server packet
pub fn parse_client_packet(pkt: &[u8], key: &str) -> Option<(u32, Vec<u8>)> {
    if pkt.len() < ProtocolConst::SEND_MESSAGE_HEAD_BYTES {
        return None;
    }

    let len = u16::from_be_bytes([pkt[0], pkt[1]]) as usize;
    let wpe = pkt[2]; // this is the extra obfuscation/padding byte
    let cmd_id = u32::from_be_bytes([pkt[3], pkt[4], pkt[5], pkt[6]]);

    // Verify packet integrity
    if pkt.len() != ProtocolConst::SEND_MESSAGE_HEAD_BYTES + len {
        println!(
            "WARNING: Client packet size mismatch - expected {}, got {} (cmd_id: {}, wpe: 0x{:02X})",
            ProtocolConst::SEND_MESSAGE_HEAD_BYTES + len,
            pkt.len(),
            cmd_id,
            wpe
        );
        return None;
    }

    let body = &pkt[7..7 + len];
    let dec = des::decrypt_bytes(body, key).ok()?;
    Some((cmd_id, dec))
}

/// Parse server→client packet
pub fn parse_server_packet(pkt: &[u8], key: &str) -> Option<(u32, Vec<u8>)> {
    if pkt.len() < ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES {
        return None;
    }

    let len = u16::from_be_bytes([pkt[0], pkt[1]]) as usize;
    let cmd_id = u32::from_be_bytes([pkt[2], pkt[3], pkt[4], pkt[5]]);

    if pkt.len() != ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES + len {
        println!(
            "WARNING: Server packet size mismatch - expected {}, got {} (cmd_id: {})",
            ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES + len,
            pkt.len(),
            cmd_id
        );
        return None;
    }

    let body = pkt[6..6 + len].to_vec();
    Some((cmd_id, body))
}

// DEPRECATED FUNCTIONS - Remove these once migration is complete
/// Extract client→server packets (with WPE byte) - DEPRECATED, use PacketBuffer instead
#[deprecated(note = "Use PacketBuffer::drain_complete_packets instead")]
pub fn extract_client_packets(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    loop {
        if buf.len() < ProtocolConst::SEND_MESSAGE_HEAD_BYTES { break; }
        let len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
        if buf.len() < ProtocolConst::SEND_MESSAGE_HEAD_BYTES + len { break; }
        let pkt = buf.drain(..ProtocolConst::SEND_MESSAGE_HEAD_BYTES + len).collect::<Vec<u8>>();
        packets.push(pkt);
    }
    packets
}

/// Extract server→client packets - DEPRECATED, use PacketBuffer instead
#[deprecated(note = "Use PacketBuffer::drain_complete_packets instead")]
pub fn extract_server_packets(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    loop {
        if buf.len() < ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES { break; }
        let len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
        if buf.len() < ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES + len { break; }
        let pkt = buf.drain(..ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES + len).collect::<Vec<u8>>();
        packets.push(pkt);
    }
    packets
}