# Anchor Panic Ps

A small Rust workspace for a local game-server emulator, an optional TCP packet-inspection proxy, HTTP/HTTPS API stubs, asset decryption, and protocol/crypto experiments.

## Build and test

Install a recent Rust toolchain (the workspace uses Rust 2024 edition for some crates), then run:

```sh
cargo build --workspace --release
cargo test --workspace
```

## Run the services

### TCP game server

```sh
cargo run -p tcpserver -- --bind 127.0.0.1 --port 8702
```

The bind address and port default to `127.0.0.1:8702`. To accept connections from another device on your LAN, explicitly bind to an appropriate interface, for example `--bind 0.0.0.0`. Set `GAME_SERVER_HOST` and `GAME_SERVER_PORT` on the HTTP service to advertise the reachable TCP address in the server list; keep its port in sync with the TCP server's `--port`.

The server loads response data from `DATA_DIR` (default: the repository's `data/` directory). Data files are intentionally not included in this repository; commands that depend on a missing JSON file return a descriptive error and do not terminate the process.

Beyond the captured init sequences, normal mode implements: public chat channels (`10054`), module reads (`10057`), mail read/attachment claims (`16005`, `16007`), hero formation/ready updates (`13044`, `13046`, `13061`), reward claims (`24022`, `24065`, `24098`, `24111`, `24113`, `24221`, `24270`) driven by `progression/` tables, and the scripted battle flow (`20100`, `20102`, `20104`, `20113`). Claims are once per id per connection and keep updating the local bag/attribute profile, so repeated requests behave like a stateful server instead of a recording.

### TCP proxy

```sh
cargo run -p tcpserver -- --proxy "gamehost:gameport" --bind 127.0.0.1 --port 8702
```

The proxy forwards packet bytes unchanged, decodes supported commands for inspection, and appends request/response groups to daily `requests_YYYYMMDD.jsonl` files in the current working directory. Newly written captures redact known login/device credentials, session and player identifiers, profile names/signatures, and public-chat text. Server messages with no known schema also get a `payload_hex` field (up to 1 KiB) so the message catalogue can be completed from a later capture; client request bytes are never stored this way. Capture files are ignored by Git; do not commit older, unredacted captures.

### Captured TCP replay

A decoded proxy capture can be used as a best-effort local server fixture:

```sh
cargo run -p tcpserver -- --replay-capture requests_20261005.jsonl
```

Replay groups are selected by client command and consumed in capture order independently for each connection. Known server message schemas are re-encoded from the decoded JSON; uncaptured commands fall back to the normal handlers, while undecoded responses are replayed byte-exact when the capture stored their `payload_hex` and skipped otherwise. Captured server timestamps are shifted to the current time. This reproduces the recorded snapshot, not full game logic: response grouping is approximate and values can be stale. The replay loader ignores client request bodies and substitutes fresh account/player/session identifiers. Captures made before the proxy stored `payload_hex` (like `requests_20261005_new.jsonl`) still lack the four schema-less messages `12106`, `12210`, `12220` and `19910`.

### HTTP API

```sh
cargo run -p Httpserver
```

HTTP listens on `127.0.0.1:10800` by default. Set `HTTP_BIND_HOST=0.0.0.0` to expose it to another device on a trusted LAN. HTTPS on port `10443` starts when `cert/localhost.crt` and `cert/localhost.key` are present. Since development certificates are excluded from version control, the server falls back to HTTP-only mode when they are absent; a present but invalid certificate is reported as an error.

Most endpoints currently emulate captured responses and are not a production authentication service. The `/v1/User/Login` route validates the repository's simplified request signature. Keep the server on a trusted local network and do not use real account credentials.

### Capture tooling

Two helper scripts under `tools/` work with decoded proxy captures:

```sh
# Coverage report: which captured commands are handled, which responses the
# replay/encoder can reproduce, and which data files a capture can fill.
python3 tools/capture_coverage.py requests_YYYYMMDD.jsonl \
    --markdown docs/capture_coverage.md --json docs/capture_coverage.json

# Write DATA_DIR JSON files from a capture so the normal-mode handlers work
# without --replay-capture.
python3 tools/import_capture_data.py requests_YYYYMMDD.jsonl --all
python3 tools/import_capture_data.py requests_YYYYMMDD.jsonl --fill-defaults
```

`import_capture_data.py` writes three kinds of files: the JSON files `data_loader.rs` reads, per-flow tables (`progression/`, `battle/`, `chat/`, `mail/`) extracted from the captured claim/battle flows, and with `--all` a dump of every captured response group under `capture/`. Existing files are kept unless `--force` is given; guessed payloads are only written by `--fill-defaults` (every field becomes an empty list/zero), and `--only <prefix>` limits a run to one data directory. A worked example for `requests_20261005_new.jsonl` is in [`docs/capture_implementation_plan.md`](docs/capture_implementation_plan.md).

### Asset unpacker

```sh
cargo run -p AssetUnpacker -- <encrypted_dir> --lua [output_dir]
```

When `output_dir` is omitted, decrypted files are written to a sibling `output2/` directory. The tool validates its arguments, avoids walking its own output, reports per-file failures, and exits non-zero if any file could not be processed.

## Current scope and limitations

- TCP packet framing supports fragmented and coalesced frames, with 16-bit protocol lengths.
- The protocol message catalogue is largely generated; unknown compound structures remain placeholders until their field layouts are known.
- Capture replay re-encodes every captured message that has a schema, sends schema-less messages byte-exact when the capture recorded `payload_hex`, and shifts captured server timestamps to the current time. Four messages (`12106`, `12210`, `12220`, `19910`) still need a new capture to recover their payloads.
- Normal mode emulates login, world entry, hero biography init, shop, mail, chat, module reads, all captured reward-claim flows (achievements, sign-ins, gifts, novice training/recruit), and a scripted battle flow; see `docs/capture_implementation_plan.md` for the per-command map and the data files each flow reads.
- Reward data lives in `DATA_DIR` (`progression/`, `battle/`, `chat/`, `mail/`, ...). Generate a full data set from a capture with `tools/import_capture_data.py`; commands whose JSON files are missing return a descriptive error and do not terminate the process.
- Several HTTP responses are static stubs. Signature formats and response fields should be verified against captures before extending them.
- Legacy crypto convenience functions are retained for compatibility. Prefer the `try_*` APIs when invalid keys or ciphertext must be handled explicitly.
