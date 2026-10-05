# What `requests_20261005_new.jsonl` lets us implement

Analysis and implementation plan for the captured session in
`requests_20261005_new.jsonl` (51 request groups, 24 client commands, 118 server
message types, 215 response packets).

The machine-readable tables live in [`capture_coverage.md`](capture_coverage.md)
and [`capture_coverage.json`](capture_coverage.json); regenerate them with:

```sh
python3 tools/capture_coverage.py requests_20261005_new.jsonl \
    --markdown docs/capture_coverage.md --json docs/capture_coverage.json
```

## 1. Done in this change

| Change | Files | Why the capture proves it |
| --- | --- | --- |
| 9 replay encoders added (`16002`, `16006`, `16008`, `17010`, `17013`, `24023`, `24099`, `24114`, `24222`) | `tcpserver/src/capture_replay.rs` | These 9 message types have a struct in `messages.rs` and decoded JSON in the capture, but no `encode_as!` arm, so `--replay-capture` silently dropped **15 packets** (mail read/enclosure, shop type, prop awards, achievement award, direct-gift buy, novice-training and novice-recruit rewards). |
| Hero-biography sequence now loads bag types `1..=8` | `tcpserver/src/data_loader.rs` | The captured init sequence sends `SC_BAG_INIT` for types 1-8; the loader only asked for 1-7, so the last bag was never sent to the client. |
| Proxy records raw bytes of undecodable **server** messages (`payload_hex`, ≤1 KiB, client packets excluded) | `tcpserver/src/proxy.rs` | 4 captured messages (`12106`, `12210`, `12220`, `19910`) have no schema in `messages.rs`; the capture only stored `decoded: null`, so they cannot be reconstructed. A new capture taken with this proxy can be decoded into a schema. |
| `tools/capture_coverage.py` | new | Cross-references a capture with `handle.rs`, `capture_replay.rs`, `data_loader.rs` and `messages.rs`; validates captured JSON against the generated structs (honouring `#[serde(rename)]`/`default`). |
| `tools/import_capture_data.py` | new | Materializes `DATA_DIR` files from the capture for the existing normal-mode handlers (86 files here). |
| `data/` populated from the capture (git-ignored) | new (not committed) | 86 JSON files + 5 synthesized empty panels, plus `data/capture/cmd_<n>.json` response dumps for every captured command. |

After these changes the replay path reproduces **211 of 215** captured packets
byte-for-byte; only the 4 schema-less messages are skipped.

## 2. Implementable next: normal-mode handlers

`--replay-capture` already serves the whole session. Implementing the missing
*normal-mode* handlers makes the emulator independent of the capture file.
The capture gives the exact response payloads for each of these commands; the
work is (a) a data file and (b) handler logic.

### Tier 1 - pure data, no state (1-2 h total)

| cmd | command | response | data file |
| --- | --- | --- | --- |
| 10054 | `CS_PUBLIC_CHAT_SETTING` | `SC_PUBLIC_CHAT_SETTING` | `public_chat/setting.json` |
| 10057 | `CS_REQ_MODULE_READ` | `SC_RES_MODULE_READ` | `module/read.json` |
| 16005 | `CS_MAIL_READ` | `SC_MAIL_READ` | `mail/read.json` |

Pattern: add a `load_packet!` entry in `data_loader.rs`, a `handle.rs` arm, and
let `tools/import_capture_data.py --all` provide the payload (already dumped in
`data/capture/cmd_<id>.json`). No new game state needed.

### Tier 2 - data + state mutation (the bulk of the captured session)

| cmd | command | captured responses | state needed |
| --- | --- | --- | --- |
| 16007 | `CS_MAIL_ENCLOSURE_REC` | `10059`, `16002`, `16008`, `17001` | mail read/claimed flags, bag/currency |
| 24022 | `CS_GAIN_ACHIEVEMENT_AWARD` | `12003`, `24023`, `24024`, `24027` | achievement points/stage |
| 24065 | `CS_GAIN_SEVEN_DAY_REWARD` | `17001`, `17013`, `24066` | seven-day sign-in day |
| 24098 | `CS_DIRECT_GIFT_BUY` | `12003`, `24097`, `24099` | titanium balance, purchase count |
| 24111 | `CS_NOVICE_TRAINING_PANEL` | `12003`, `17001`, `17013`, `24112`, `24114` | training task progress (`SC_NOVICE_TRAINING_PANEL` already exists in `data/hero_biography`) |
| 24113 | `CS_NOVICE_TRAINING_RECEIVE_TASK` | *(none captured)* | task claim state |
| 24221 | `CS_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE` | `10059`, `17001`, `17013`, `24222` | activity claim state |
| 24270 | `CS_GAIN_OPEN_SERVER_SIGN_REWARD` | `17001`, `17013`, `24271` | sign reward day |

Each command is an independent unit: load the response sequence, apply the
attribute/bag deltas the capture shows (`SC_PLAYER_UPDATE_ATTR_*`,
`SC_BAG_UPDATE`, `SC_PROP_AWARD_SEND`), and answer. Because the capture contains
several samples of the same reward flow, the responses can be checked against
each other (e.g. `24111` appears twice).

### Tier 3 - battle flow (largest, needs a small state machine)

* `20100 CS_BATTLE_FIELD_ENTER` → `13045`, `13047`, `13062`, `20101`
* `20102 CS_BATTLE_START` → nothing captured (client-side start)
* `20113 CS_BATTLE_AUTO` → `12100`, `10059`, `20103`, `20114`, `20125`
* `20104 CS_BATTLE_VIDEO_END` (14 calls) → per-call action/result batches
  (`20103`, `20105`, `20106`, `20125`, plus achievement/task/level updates)

The capture contains 14 complete `CS_BATTLE_VIDEO_END` groups in order, enough
to replay a full battle with the existing replay path. A normal-mode version
needs a per-connection battle cursor plus a rule for choosing the next action
batch (`SC_BATTLE_ACTION`, `SC_BATTLE_ACTION_END`, `SC_BATTLE_RESULT`) - the
recorded batches can be used as templates.

### Not implementable from this capture

| cmd | payload | note |
| --- | --- | --- |
| 12106 | 3 B | only appears inside the hero-biography chain |
| 12210 | 2 B | idem |
| 12220 | 6 B | idem |
| 19910 | 2 B | idem |

The proxy stored `decoded: null` for these (no schema, no raw bytes). Two ways
forward:

1. Re-run the proxy (it now records `payload_hex` for undecoded server packets)
   against the live server, then add the message structs.
2. Add a *raw packet* fallback to `capture_replay.rs` so a capture that carries
   `payload_hex` replays those messages byte-exact even without a schema
   (planned follow-up, not implemented yet).

Also note `13044`, `13046`, `13061`, `20102`, `24113` have **no captured
response**: the real server answered nothing (or the reply was empty). Handlers
for them should stay no-ops or be derived from game rules, not from this file.

## 3. Using the generated data set

```sh
# 1. This capture only populates login/, shop/shop_type_1.json,
#    shop/direct_gift_panel.json and hero_biography/*.
python3 tools/import_capture_data.py requests_20261005_new.jsonl --all

# 2. The hero-biography sequence wants 5 panels this capture does not contain.
#    Write empty placeholder panels for them (reviewed: all fields are lists/0):
python3 tools/import_capture_data.py requests_20261005_new.jsonl \
    --fill-defaults --only hero_biography/

# 3. Normal-mode emulation now answers CS_ACCOUNT_LOGIN, CS_SHOP_TYPE_DATA,
#    CS_DIRECT_GIFT_PANEL and CS_HERO_BIOGRAPHY_INFO without --replay-capture.
cargo run -p tcpserver -- --bind 127.0.0.1 --port 8702
```

`shop/shop_type_2..5.json` are deliberately **not** generated: the capture never
requested those shop types, and a guessed shop payload would be wrong. The
handler returns a descriptive error for them, as designed.

## 4. Suggested order

1. Tier 1 handlers (small, self-contained).
2. Tier 2 reward/claim flows, starting with `16007` and `24065` (mail + sign-in
   touch only bag/attribute updates that already have encoders).
3. Raw-packet replay fallback in `capture_replay.rs` + a fresh proxy capture to
   decode `12106`, `12210`, `12220`, `19910`.
4. Tier 3 battle state machine.
