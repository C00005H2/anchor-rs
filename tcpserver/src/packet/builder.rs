use super::constants::ProtocolConst;

/// Build a server→client packet with the proper 6-byte header
pub fn build_server_packet(cmd_id: u32, body: &[u8]) -> Vec<u8> {
    let len = body.len() as u16;
    let mut out = Vec::with_capacity(ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES + body.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&cmd_id.to_be_bytes());
    out.extend_from_slice(body);
    out
}