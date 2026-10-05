use super::constants::ProtocolConst;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PacketBuildError {
    #[error("packet body is too large for the 16-bit length field: {0} bytes")]
    BodyTooLarge(usize),
}

/// Build a server→client packet with a 6-byte header.
pub fn build_server_packet(cmd_id: u32, body: &[u8]) -> Result<Vec<u8>, PacketBuildError> {
    let len = u16::try_from(body.len()).map_err(|_| PacketBuildError::BodyTooLarge(body.len()))?;
    let mut out = Vec::with_capacity(ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES + body.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&cmd_id.to_be_bytes());
    out.extend_from_slice(body);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_header_and_payload() {
        assert_eq!(
            build_server_packet(0x0102_0304, &[0xaa, 0xbb]).unwrap(),
            vec![0, 2, 1, 2, 3, 4, 0xaa, 0xbb]
        );
    }

    #[test]
    fn rejects_a_body_that_cannot_be_represented_by_the_header() {
        let body = vec![0; u16::MAX as usize + 1];
        assert_eq!(
            build_server_packet(1, &body),
            Err(PacketBuildError::BodyTooLarge(body.len()))
        );
    }
}
