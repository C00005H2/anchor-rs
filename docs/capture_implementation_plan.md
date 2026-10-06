# What `requests_20261005_new.jsonl` lets us implement — status

Capture: 51 request groups, 24 client commands, 118 server message types, 215
response packets.  Machine-readable tables: [`capture_coverage.md`](capture_coverage.md),
[`capture_coverage.json`](capture_coverage.json) — regenerate with:

```sh
python3 tools/capture_coverage.py requests_20261005_new.jsonl \
    --markdown docs/capture_coverage.md --json docs/capture_coverage.json
```

## Implemented

### Replay path (all captured commands)

| Change | Files |
| --- | --- |
| 9 replay encoders added (`16002`, `16006`, `16008`, `17010`, `17013`, `24023`, `24099`, `24114`, `24222`) — recovers the 15 packets replay used to drop | `tcpserver/src/capture_replay.rs` |
| Schema-less responses replay byte-exact when the capture carries `payload_hex` (truncated payloads are skipped) | `tcpserver/src/capture_replay.rs`, `tcpserver/src/proxy.rs` |
| Captured server timestamps (`time`, `next_refresh_time`) are shifted to "now" using the capture's own `SC_SYS_DATE` | `tcpserver/src/capture_replay.rs` |
| Proxy records `payload_hex` (≤1 KiB) for undecodable server messages so future captures can decode `12106`, `12210`, `12220`, `19910` | `tcpserver/src/proxy.rs` |

`--replay-capture requests_20261005_new.jsonl` reproduces 211 of 215 packets;
the four schema-less messages stay skipped until a capture with raw bytes
exists (the bundled capture predates `payload_hex`).

### Normal-mode handlers (stateful, not replay)

All 18 previously unhandled captured commands now have handlers.  Claim flows
are driven by data tables the importer extracts from the capture; claims are
once per id, bag stacks merge by template id, and attribute rewards are applied
as **deltas** on the local profile (`attr_delta`) so repeated sessions keep
accumulating instead of resetting to recorded values.

| cmd | command | handler | data read |
| --- | --- | --- | --- |
| 10054 | `CS_PUBLIC_CHAT_SETTING` | `system::handle_public_chat_setting` | `chat/public_chat.json` |
| 10057 | `CS_REQ_MODULE_READ` | `system::handle_req_module_read` | — (echo) |
| 13044 / 13046 / 13061 | ready / formation / cannot-delete | `cmd/hero.rs` (stored for battle entry + immediate `SC_SET_READY`/`SC_CHANGE_HERO`/`SC_CANNOT_DEL_HERO_LIST` acks; plus `13040`/`13042`/`13048` formation handlers) | — |
| 16005 | `CS_MAIL_READ` | `mail::handle_mail_read` | `hero_biography/mail_list.json` |
| 16007 | `CS_MAIL_ENCLOSURE_REC` | `mail::handle_mail_enclosure_rec` | `hero_biography/mail_list.json`, `mail/enclosure_unread.json` |
| 20100 | `CS_BATTLE_FIELD_ENTER` | `battle::handle_battle_field_enter` | `battle/enter.json` (formation patched in) |
| 20102 | `CS_BATTLE_START` | `battle::handle_battle_start` | — (no reply in capture) |
| 20104 | `CS_BATTLE_VIDEO_END` | `battle::handle_battle_video_end` | `battle/video_end.json` (14 scripted batches) |
| 20113 | `CS_BATTLE_AUTO` | `battle::handle_battle_auto` | `battle/auto.json` |
| 24022 | `CS_GAIN_ACHIEVEMENT_AWARD` | `activity::handle_gain_achievement_award` | `progression/achievement.json` |
| 24065 | `CS_GAIN_SEVEN_DAY_REWARD` | `activity::handle_gain_seven_day_reward` | `progression/seven_day.json` |
| 24098 | `CS_DIRECT_GIFT_BUY` | `shop::handle_direct_gift_buy` | `progression/direct_gift.json`, `shop/direct_gift_panel.json` |
| 24111 / 24113 | novice training panel / receive | `cmd/activity.rs` | `progression/novice_training.json` |
| 24221 | `CS_ACTIVITY_NOVICE_RECRUIT_HERO_RECEIVE` | `activity::handle_novice_recruit_receive` | `progression/novice_recruit.json` |
| 24270 | `CS_GAIN_OPEN_SERVER_SIGN_REWARD` | `activity::handle_gain_open_server_sign_reward` | `progression/open_server_sign.json` |

Battle is a **scripted** flow: the capture's 13 `CS_BATTLE_VIDEO_END` batches
(42-message finale included) are consumed in order per connection, and the
attribute updates inside the script are absorbed into the local profile.  This
reproduces the recorded fight, not arbitrary battle logic.  Recorded no-op
batches are skipped, result-less sessions are avoided (with a synthesized
victory fallback), turns of benched heroes are skipped, and a missing
formation falls back to the recorded roster, so a re-deployed lineup can never
stall the match with an empty reply.

### Tooling and data

* `tools/capture_coverage.py` — coverage report: handler/replay/data-file status
  per captured command, schema validation of captured payloads.
* `tools/import_capture_data.py` — writes the loader files, the flow tables and
  (`--all`) response dumps for a capture; `--fill-defaults` adds empty panels.
* `data/` (git-ignored) is fully generated from this capture: 86 loader files,
  11 flow tables, 118 response dumps.

## Known gaps

| Gap | Why | Next step |
| --- | --- | --- |
| `12106`, `12210`, `12220`, `19910` payloads | this capture has neither schema nor raw bytes | re-run the proxy (it now stores `payload_hex`), then add structs or rely on raw replay |
| shop types 2–5 data files | never requested in the capture; guessing payloads would be wrong | capture those shop types or hand-write `shop/shop_type_2..5.json` |
| 5 biography panels (`activity_novice_turntable`, `activity_pay_sign2_panel`, `pack_bag_panel`, `happy_farm_*`) | not present in the capture | empty placeholders were generated (`--fill-defaults`); replace with real data when captured |
| achievement/mail failure replies | no failure sample in the capture | implemented as `result: 0` responses; verify against a live server |
| attribute key map | inferred from the capture (`611`=exp, `612`=gold, `600`=level, `608`=max exp) | confirm against client code before relying on it further |
| `cargo build` verification | no Rust toolchain in the analysis sandbox | run `cargo build --workspace && cargo test --workspace` |

## Regenerating the data set

```sh
# loader files + flow tables + per-command dumps
python3 tools/import_capture_data.py requests_20261005_new.jsonl --all

# empty placeholder panels for biography files the capture lacks
python3 tools/import_capture_data.py requests_20261005_new.jsonl \
    --fill-defaults --only hero_biography/

cargo run -p tcpserver -- --bind 127.0.0.1 --port 8702      # normal mode
cargo run -p tcpserver -- --replay-capture requests_20261005_new.jsonl
```

- 2026-10-06: hero-biography init sequence now byte-exact vs capture (91 packets incl. raw 19910/12106/12210/12220, zero-filled until re-captured with payload_hex).
