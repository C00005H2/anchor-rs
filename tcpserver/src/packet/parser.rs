use super::constants::ProtocolConst;
use crypto::des;

/// Parse and decrypt a client→server packet. Client packets have a 7-byte
/// header containing the payload length, WPE byte, and command id.
pub fn parse_client_packet(pkt: &[u8], key: &str) -> Option<(u32, Vec<u8>)> {
    if pkt.len() < ProtocolConst::SEND_MESSAGE_HEAD_BYTES {
        return None;
    }

    let len = u16::from_be_bytes([pkt[0], pkt[1]]) as usize;
    let cmd_id = u32::from_be_bytes([pkt[3], pkt[4], pkt[5], pkt[6]]);
    let expected_len = ProtocolConst::SEND_MESSAGE_HEAD_BYTES.checked_add(len)?;
    if pkt.len() != expected_len {
        return None;
    }

    let body = &pkt[ProtocolConst::SEND_MESSAGE_HEAD_BYTES..];
    let decrypted = des::decrypt_bytes(body, key).ok()?;
    Some((cmd_id, decrypted))
}

/// Parse a server→client packet. Server packets have a 6-byte header and an
/// unencrypted payload.
pub fn parse_server_packet(pkt: &[u8], _key: &str) -> Option<(u32, Vec<u8>)> {
    if pkt.len() < ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES {
        return None;
    }

    let len = u16::from_be_bytes([pkt[0], pkt[1]]) as usize;
    let cmd_id = u32::from_be_bytes([pkt[2], pkt[3], pkt[4], pkt[5]]);
    let expected_len = ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES.checked_add(len)?;
    if pkt.len() != expected_len {
        return None;
    }

    Some((
        cmd_id,
        pkt[ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES..].to_vec(),
    ))
}

// DEPRECATED FUNCTIONS - Remove these once migration is complete.
/// Extract client→server packets (with WPE byte) - DEPRECATED, use
/// `PacketBuffer::drain_complete_packets` instead.
#[deprecated(note = "Use PacketBuffer::drain_complete_packets instead")]
pub fn extract_client_packets(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    extract_packets(buf, ProtocolConst::SEND_MESSAGE_HEAD_BYTES)
}

/// Extract server→client packets - DEPRECATED, use
/// `PacketBuffer::drain_complete_packets` instead.
#[deprecated(note = "Use PacketBuffer::drain_complete_packets instead")]
pub fn extract_server_packets(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    extract_packets(buf, ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES)
}

fn extract_packets(buf: &mut Vec<u8>, header_size: usize) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    let mut consumed = 0usize;

    while buf.len().saturating_sub(consumed) >= header_size {
        let len = u16::from_be_bytes([buf[consumed], buf[consumed + 1]]) as usize;
        let Some(frame_len) = header_size.checked_add(len) else {
            break;
        };
        if buf.len().saturating_sub(consumed) < frame_len {
            break;
        }
        packets.push(buf[consumed..consumed + frame_len].to_vec());
        consumed += frame_len;
    }

    if consumed > 0 {
        buf.drain(..consumed);
    }
    packets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto::EncryptUtil;

    const KEY: &str = "abcdefgh";

    #[test]
    fn client_packet_decrypts_body_and_reads_command_id() {
        let plaintext = b"client payload";
        let encrypted = EncryptUtil::try_des_encrypt_bytes(plaintext, KEY).unwrap();
        let mut packet = Vec::new();
        packet.extend_from_slice(&(encrypted.len() as u16).to_be_bytes());
        packet.push(0x2a); // WPE byte
        packet.extend_from_slice(&11000u32.to_be_bytes());
        packet.extend_from_slice(&encrypted);

        assert_eq!(parse_client_packet(&packet, KEY), Some((11000, plaintext.to_vec())));
    }

    #[test]
    fn rejects_mismatched_and_undecryptable_client_packets() {
        assert_eq!(parse_client_packet(&[0, 2, 0, 0, 0, 0, 1, 0], KEY), None);
        assert_eq!(parse_client_packet(&[0, 3, 0, 0, 0, 0, 1, 1, 2, 3], KEY), None);
    }

    #[test]
    fn server_packet_reads_payload_without_decrypting() {
        let packet = [0, 2, 0, 0, 0, 12, 0xaa, 0xbb];
        assert_eq!(parse_server_packet(&packet, KEY), Some((12, vec![0xaa, 0xbb])));
        assert_eq!(parse_server_packet(&packet[..7], KEY), None);
    }
}
