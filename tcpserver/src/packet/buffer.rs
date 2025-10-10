use super::constants::ProtocolConst;
use thiserror::Error;

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

            println!("PacketBuffer: Found packet at pos {}, header says length {}, need total {}",
                     pos, len, header_size + len);

            // Check if we have the complete packet
            if pos + header_size + len <= self.buffer.len() {
                // Complete packet available - extract it
                let packet = self.buffer[pos..pos + header_size + len].to_vec();
                packets.push(packet);
                pos += header_size + len;
            } else {
                println!("PacketBuffer: Incomplete packet, waiting for {} more bytes",
                         (pos + header_size + len) - self.buffer.len());
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
        self.write_i16(s.len() as i16);
        self.data.extend_from_slice(s.as_bytes());
    }
    pub fn write_bool(&mut self, v: bool) {
        self.data.push(if v { 1 } else { 0 });
    }
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_i16(bytes.len() as i16);
        self.data.extend_from_slice(bytes);
    }
    pub fn write_raw_bytes(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
    }
}