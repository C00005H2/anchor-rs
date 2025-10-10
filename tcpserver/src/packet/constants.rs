/// Protocol header constants (mirrors ProtocolConst.cs)
pub struct ProtocolConst;

impl ProtocolConst {
    pub const WPE_KEY_START_NUM: u8 = 0;
    pub const WPE_KEY_END_NUM: u8 = 127;

    pub const DATA_LENGTH_BYTES: usize = 2;
    pub const ID_BYTES: usize = 4;
    pub const WPE_BYTES: usize = 1;

    pub const RECEIVE_MESSAGE_HEAD_BYTES: usize = Self::DATA_LENGTH_BYTES + Self::ID_BYTES; // 6
    pub const SEND_MESSAGE_HEAD_BYTES: usize =
        Self::DATA_LENGTH_BYTES + Self::WPE_BYTES + Self::ID_BYTES; // 7
}