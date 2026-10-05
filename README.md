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

### TCP proxy

```sh
cargo run -p tcpserver -- --proxy "gamehost:gameport" --bind 127.0.0.1 --port 8702
```

The proxy forwards packet bytes unchanged, decodes supported commands for inspection, and appends request/response groups to daily `requests_YYYYMMDD.jsonl` files in the current working directory. Newly written captures redact known login/device credentials, session and player identifiers, profile names/signatures, and public-chat text. Capture files are ignored by Git; do not commit older, unredacted captures.

### Captured TCP replay

A decoded proxy capture can be used as a best-effort local server fixture:

```sh
cargo run -p tcpserver -- --replay-capture requests_20261005.jsonl
```

Replay groups are selected by client command and consumed in capture order independently for each connection. Known server message schemas are re-encoded from the decoded JSON; uncaptured commands fall back to the normal handlers, while unknown or undecoded captured responses are skipped. This reproduces the recorded snapshot, not full game logic: response grouping is approximate, values can be stale, and messages without a known schema cannot be reconstructed because the JSONL file does not retain their raw bytes. The replay loader ignores client request bodies and substitutes fresh account/player/session identifiers.

### HTTP API

```sh
cargo run -p Httpserver
```

HTTP listens on `127.0.0.1:10800` by default. Set `HTTP_BIND_HOST=0.0.0.0` to expose it to another device on a trusted LAN. HTTPS on port `10443` starts when `cert/localhost.crt` and `cert/localhost.key` are present. Since development certificates are excluded from version control, the server falls back to HTTP-only mode when they are absent; a present but invalid certificate is reported as an error.

Most endpoints currently emulate captured responses and are not a production authentication service. The `/v1/User/Login` route validates the repository's simplified request signature. Keep the server on a trusted local network and do not use real account credentials.

### Asset unpacker

```sh
cargo run -p AssetUnpacker -- <encrypted_dir> --lua [output_dir]
```

When `output_dir` is omitted, decrypted files are written to a sibling `output2/` directory. The tool validates its arguments, avoids walking its own output, reports per-file failures, and exits non-zero if any file could not be processed.

## Current scope and limitations

- TCP packet framing supports fragmented and coalesced frames, with 16-bit protocol lengths.
- The protocol message catalogue is largely generated; unknown compound structures remain placeholders until their field layouts are known.
- The emulator currently implements a small set of game commands. Additional response sequences require the matching JSON files under `DATA_DIR`.
- Several HTTP responses are static stubs. Signature formats and response fields should be verified against captures before extending them.
- Legacy crypto convenience functions are retained for compatibility. Prefer the `try_*` APIs when invalid keys or ciphertext must be handled explicitly.
