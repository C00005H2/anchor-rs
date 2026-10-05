use super::constants::ProtocolConst;
use thiserror::Error;

/// Upper bound used when decoding untrusted 16-bit collection lengths.
/// Captured game messages stay well below this limit; malformed counts should
/// not trigger huge allocations or long decode loops.
pub const MAX_COLLECTION_ITEMS: usize = 1024;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Not enough bytes: need {needed}, but only {remaining} remaining")]
    UnexpectedEof { needed: usize, remaining: usize },

    #[error("Invalid UTF-8 string: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
}


/// Maintains WPE index state (increments by 2, wraps at 127 → 0)
pub struct WpeCounter {
    index: u8,
}

impl WpeCounter {
    pub fn new() -> Self {
        Self { index: ProtocolConst::WPE_KEY_START_NUM }
    }

    pub fn next(&mut self) -> u8 {
        let current = self.index;
        self.index = if self.index >= ProtocolConst::WPE_KEY_END_NUM {
            ProtocolConst::WPE_KEY_START_NUM
        } else {
            self.index.wrapping_add(2)
        };
        current
    }
}

/// Buffer for handling TCP fragmentation
pub struct PacketBuffer {
    buffer: Vec<u8>,
    is_client_to_server: bool,
}

impl PacketBuffer {
    pub fn new(is_client_to_server: bool) -> Self {
        Self {
            buffer: Vec::new(),
            is_client_to_server,
        }
    }

    /// Add incoming TCP data to the buffer
    pub fn push_data(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Extract all complete packets from the buffer
    pub fn drain_complete_packets(&mut self) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        let mut pos = 0;

        let header_size = if self.is_client_to_server {
            ProtocolConst::SEND_MESSAGE_HEAD_BYTES
        } else {
            ProtocolConst::RECEIVE_MESSAGE_HEAD_BYTES
        };

        while pos + header_size <= self.buffer.len() {
            // Read length field (first 2 bytes)
            let len = u16::from_be_bytes([self.buffer[pos], self.buffer[pos + 1]]) as usize;

            let Some(frame_len) = header_size.checked_add(len) else {
                break;
            };

            // Check if we have the complete packet.
            if pos + frame_len <= self.buffer.len() {
                // Complete packet available - extract it
                let packet = self.buffer[pos..pos + frame_len].to_vec();
                packets.push(packet);
                pos += frame_len;
            } else {
                break;
            }
        }

        // Remove processed data from buffer
        if pos > 0 {
            self.buffer.drain(..pos);
        }

        packets
    }

    /// Get current buffer size for debugging
    pub fn buffer_size(&self) -> usize {
        self.buffer.len()
    }

    /// Check if buffer has partial data
    pub fn has_partial_data(&self) -> bool {
        !self.buffer.is_empty()
    }
}

/// Protocol byte buffer for reading/writing structured data
pub struct ProtocolByteBuf {
    data: Vec<u8>,
    pos: usize,
}

impl ProtocolByteBuf {
    pub fn new(data: &[u8]) -> Self {
        Self { data: data.to_vec(), pos: 0 }
    }

    pub fn new_write() -> Self {
        Self { data: Vec::new(), pos: 0 }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.remaining() < n {
            return None;
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Some(slice)
    }

    pub fn ensure_capacity(&mut self, bytes_needed: usize) {
        let available = self.data.len() - self.pos;
        if available < bytes_needed {
            let padding_needed = bytes_needed - available;
            self.data.resize(self.data.len() + padding_needed, 0);
        }
    }

    // --- Safe Readers ---
    pub fn read_i8(&mut self) -> Option<i8> {
        self.take(1).map(|b| b[0] as i8)
    }

    pub fn read_i16(&mut self) -> Option<i16> {
        self.take(2).map(|b| i16::from_be_bytes([b[0], b[1]]))
    }

    pub fn read_i32(&mut self) -> Option<i32> {
        self.take(4).map(|b| i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn read_i64(&mut self) -> Option<i64> {
        self.take(8).map(|b| i64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn read_u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn read_u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn read_u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn read_string(&mut self) -> Option<String> {
        let len_raw = self.read_i16()?;

        // Validate string length is reasonable (i16 max is 32767)
        if len_raw < 0 {
            return None;
        }

        let len = len_raw as usize;
        let slice = self.take(len)?;
        Some(String::from_utf8_lossy(slice).into_owned())
    }

    pub fn read_bool(&mut self) -> Option<bool> {
        self.read_i8().map(|b| b != 0)
    }

    // --- Padded Readers (guaranteed to succeed) ---
    pub fn read_i8_padded(&mut self) -> i8 {
        self.ensure_capacity(1);
        self.read_i8().unwrap()
    }

    pub fn read_i16_padded(&mut self) -> i16 {
        self.ensure_capacity(2);
        self.read_i16().unwrap()
    }

    /// Read a signed 16-bit collection count and clamp malformed values to a
    /// safe range before converting to `usize`.
    pub fn read_count_padded(&mut self) -> usize {
        self.read_i16_padded()
            .clamp(0, MAX_COLLECTION_ITEMS as i16) as usize
    }

    pub fn read_i32_padded(&mut self) -> i32 {
        self.ensure_capacity(4);
        self.read_i32().unwrap()
    }

    pub fn read_i64_padded(&mut self) -> i64 {
        self.ensure_capacity(8);
        self.read_i64().unwrap()
    }

    pub fn read_u16_padded(&mut self) -> u16 {
        self.ensure_capacity(2);
        self.read_u16().unwrap()
    }

    pub fn read_u32_padded(&mut self) -> u32 {
        self.ensure_capacity(4);
        self.read_u32().unwrap()
    }

    pub fn read_u64_padded(&mut self) -> u64 {
        self.ensure_capacity(8);
        self.read_u64().unwrap()
    }

    pub fn read_string_padded(&mut self) -> String {
        // First ensure we can read the length
        self.ensure_capacity(2);
        let len_raw = self.read_i16().unwrap();

        // Validate string length (i16 max is 32767)
        let len = if len_raw < 0 {
            0
        } else {
            len_raw as usize
        };

        // Ensure we can read the string data
        self.ensure_capacity(len);
        let slice = self.take(len).unwrap();
        String::from_utf8_lossy(slice).into_owned()
    }

    pub fn read_bool_padded(&mut self) -> bool {
        self.ensure_capacity(1);
        self.read_bool().unwrap()
    }

    // --- Writers (unchanged) ---
    pub fn write_i8(&mut self, v: i8) {
        self.data.push(v as u8);
    }
    pub fn write_i16(&mut self, v: i16) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }
    pub fn write_count(&mut self, count: usize) {
        self.write_i16(count.min(MAX_COLLECTION_ITEMS) as i16);
    }
    pub fn write_i32(&mut self, v: i32) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }
    pub fn write_i64(&mut self, v: i64) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }
    pub fn write_u16(&mut self, v: u16) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }
    pub fn write_u32(&mut self, v: u32) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }
    pub fn write_u64(&mut self, v: u64) {
        self.data.extend_from_slice(&v.to_be_bytes());
    }
    pub fn write_string(&mut self, s: &str) {
        let mut len = s.len().min(i16::MAX as usize);
        while !s.is_char_boundary(len) {
            len -= 1;
        }
        if len < s.len() {
            tracing::warn!(original_len = s.len(), encoded_len = len, "Truncating oversized protocol string");
        }
        self.write_i16(len as i16);
        self.data.extend_from_slice(&s.as_bytes()[..len]);
    }
    pub fn write_bool(&mut self, v: bool) {
        self.data.push(if v { 1 } else { 0 });
    }
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        let len = bytes.len().min(i16::MAX as usize);
        if len < bytes.len() {
            tracing::warn!(original_len = bytes.len(), encoded_len = len, "Truncating oversized protocol byte field");
        }
        self.write_i16(len as i16);
        self.data.extend_from_slice(&bytes[..len]);
    }
    pub fn write_raw_bytes(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_buffer_handles_fragmented_and_coalesced_frames() {
        let first = [0, 1, 0, 0, 0, 0, 10, 0xaa];
        let second = [0, 2, 0, 0, 0, 0, 11, 0xbb, 0xcc];
        let mut buffer = PacketBuffer::new(true);

        buffer.push_data(&first[..4]);
        assert!(buffer.drain_complete_packets().is_empty());
        assert_eq!(buffer.buffer_size(), 4);

        let mut remainder = first[4..].to_vec();
        remainder.extend_from_slice(&second);
        buffer.push_data(&remainder);
        assert_eq!(
            buffer.drain_complete_packets(),
            vec![first.to_vec(), second.to_vec()]
        );
        assert!(!buffer.has_partial_data());
    }

    #[test]
    fn packet_buffer_uses_six_byte_server_header() {
        let frame = [0, 1, 0, 0, 0, 12, 0xaa];
        let mut buffer = PacketBuffer::new(false);
        buffer.push_data(&frame);
        assert_eq!(buffer.drain_complete_packets(), vec![frame.to_vec()]);
    }

    #[test]
    fn padded_collection_count_is_nonnegative_and_bounded() {
        let mut negative = ProtocolByteBuf::new(&i16::MIN.to_be_bytes());
        assert_eq!(negative.read_count_padded(), 0);

        let mut too_large = ProtocolByteBuf::new(&i16::MAX.to_be_bytes());
        assert_eq!(too_large.read_count_padded(), MAX_COLLECTION_ITEMS);

        let mut writer = ProtocolByteBuf::new_write();
        writer.write_count(usize::MAX);
        assert_eq!(
            writer.into_bytes(),
            (MAX_COLLECTION_ITEMS as i16).to_be_bytes().to_vec()
        );
    }

    #[test]
    fn string_writer_clamps_length_without_splitting_utf8() {
        let value = "é".repeat(16_384);
        let mut writer = ProtocolByteBuf::new_write();
        writer.write_string(&value);
        let encoded = writer.into_bytes();
        let length = i16::from_be_bytes([encoded[0], encoded[1]]) as usize;
        assert_eq!(length, i16::MAX as usize - 1);
        assert_eq!(encoded.len(), length + 2);
        assert!(std::str::from_utf8(&encoded[2..]).is_ok());
    }
}
