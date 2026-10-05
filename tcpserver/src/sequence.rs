//! Response groups loaded from DATA_DIR for scripted flows.
//!
//! Battle scripts, chat channels and similar multi-message flows store their
//! decoded payloads as JSON files.  Each file contains an ordered list of
//! response groups; a group is the batch the real server sent for one client
//! request.  Handlers consume groups in order, re-encoding them with the same
//! machinery capture replay uses, so the wire format stays consistent.

use serde::Deserialize;
use serde_json::Value;

use crate::{
    capture_replay::{decode_payload_hex, encode_captured_response, CapturedResponse, ReplayCursor},
    data_loader::GameDataLoader,
};

/// One captured response payload.  Exactly one of `decoded` (known schema) or
/// `payload_hex` (schema-less message) is expected to be set.
#[derive(Clone, Debug, Deserialize)]
pub struct TemplateResponse {
    pub cmd: u32,
    #[serde(default)]
    pub decoded: Option<Value>,
    #[serde(default)]
    pub payload_hex: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TemplateGroup {
    #[serde(default)]
    pub responses: Vec<TemplateResponse>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TemplateFile {
    #[serde(default)]
    pub groups: Vec<TemplateGroup>,
}

impl TemplateFile {
    pub fn load(relative_path: &str) -> Result<Self, anyhow::Error> {
        GameDataLoader::load_struct(relative_path)
    }

    pub fn first_group(&self) -> Option<&TemplateGroup> {
        self.groups.first()
    }

    pub fn group(&self, index: usize) -> Option<&TemplateGroup> {
        self.groups.get(index)
    }

    pub fn group_count(&self) -> usize {
        self.groups.len()
    }
}

impl TemplateGroup {
    /// Re-encode every response of this group into server packets.
    ///
    /// Responses with an unknown schema are sent byte-exact when raw bytes are
    /// stored (`payload_hex`); truncated raw payloads are skipped and logged.
    pub fn encode(&self, cursor: &ReplayCursor) -> Result<Vec<Vec<u8>>, anyhow::Error> {
        let mut packets = Vec::with_capacity(self.responses.len());
        for response in &self.responses {
            let captured = match &response.decoded {
                Some(decoded) if !decoded.is_null() => {
                    let decoded = cursor.rehydrate(decoded);
                    CapturedResponse {
                        cmd: response.cmd,
                        decoded,
                        raw: None,
                    }
                }
                _ => match response.payload_hex.as_deref().and_then(decode_payload_hex) {
                    Some(raw) => CapturedResponse {
                        cmd: response.cmd,
                        decoded: Value::Null,
                        raw: Some(raw),
                    },
                    None => {
                        tracing::warn!(
                            command = response.cmd,
                            "Skipping scripted response without usable payload"
                        );
                        continue;
                    }
                },
            };

            match encode_captured_response(&captured) {
                Ok(Some(packet)) => packets.push(packet),
                Ok(None) => tracing::warn!(
                    command = captured.cmd,
                    "Skipping scripted response with no known message schema"
                ),
                Err(error) => tracing::warn!(
                    command = captured.cmd,
                    error = %error,
                    "Could not encode scripted response"
                ),
            }
        }
        Ok(packets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::parse_server_packet;
    use serde_json::json;

    fn file(json_text: &str) -> TemplateFile {
        serde_json::from_str(json_text).unwrap()
    }

    #[test]
    fn groups_decode_from_the_capture_style_layout() {
        let script = file(
            r#"{"groups":[[{"cmd":10002,"decoded":{"time":1,"open_date":0,"merge_date":0}},
                             {"cmd":10002,"decoded":{"time":2,"open_date":0,"merge_date":0}}]]}"#,
        );
        assert_eq!(script.group_count(), 1);
        assert_eq!(script.first_group().unwrap().responses.len(), 2);
        assert_eq!(script.group(0).unwrap().responses[0].cmd, 10002);
        assert!(script.group(5).is_none());
    }

    #[test]
    fn scripted_group_encodes_known_and_raw_payloads() {
        let group = file(
            r#"{"groups":[[
                {"cmd":10002,"decoded":{"time":123,"open_date":100,"merge_date":0}},
                {"cmd":19910,"payload_hex":"0001"}
            ]]}"#,
        )        .groups
        .pop()
        .unwrap();

        let cursor = ReplayCursor::default();
        let packets = group.encode(&cursor).unwrap();
        assert_eq!(packets.len(), 2);

        let (cmd, body) = parse_server_packet(&packets[0], "").unwrap();
        assert_eq!(cmd, 10002);
        assert_eq!(
            crate::dispatch::dispatch_cmd(cmd, &body),
            Some(json!({"time": 123, "open_date": 100, "merge_date": 0}))
        );

        let (cmd, body) = parse_server_packet(&packets[1], "").unwrap();
        assert_eq!(cmd, 19910);
        assert_eq!(body, vec![0u8, 1]);
    }

    #[test]
    fn truncated_raw_payloads_are_skipped() {
        let group = TemplateGroup {
            responses: vec![TemplateResponse {
                cmd: 12106,
                decoded: None,
                payload_hex: Some("0001...".to_owned()),
            }],
        };
        let cursor = ReplayCursor::default();
        assert!(group.encode(&cursor).unwrap().is_empty());
    }
}
