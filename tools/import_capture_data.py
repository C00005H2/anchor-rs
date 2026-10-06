#!/usr/bin/env python3
"""Materialize ``DATA_DIR`` JSON files from a decoded proxy capture.

The normal-mode handlers in ``tcpserver/src/data_loader.rs`` read one JSON file
per server message from ``DATA_DIR`` (default ``data/``).  A proxy capture
already contains the decoded payloads for those messages, so this tool writes
them out in the exact layout the loader expects.

What it does

* maps captured server messages onto ``load_packet!`` expectations by command
  id, and verifies that the message struct matches before writing;
* expands the ``1..=N`` loop used for bag-init files;
* routes shop responses to ``shop/shop_type_<n>.json`` using the captured
  request's ``shop_type`` field;
* with ``--fill-defaults`` writes empty placeholder panels for loader files the
  capture does not contain (all remaining fields become empty lists / zeroes);
* with ``--all`` also dumps every captured response group under
  ``<data-dir>/capture/`` for commands that have no loader slot yet.

Credentials and user-identifying values are redacted the same way the proxy
does, and the ``__ANCHOR_REPLAY_*__`` placeholders are replaced with plain
numeric ids.

Usage::

    python3 tools/import_capture_data.py requests_20261005_new.jsonl
    python3 tools/import_capture_data.py capture.jsonl --fill-defaults --all
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from capture_coverage import (  # noqa: E402  (same-directory helper module)
    MessageStruct,
    parse_capture,
    parse_data_loader,
    parse_messages,
)

MARKERS = {
    "__ANCHOR_REPLAY_ACCOUNT_ID__": "1180000000000000001",
    "__ANCHOR_REPLAY_PLAYER_ID__": "1180000000000000002",
    "__ANCHOR_REPLAY_SESSION__": "00000000000000000000000000000000",
}

REDACTED_KEYS = {
    "acc_name": "[redacted]",
    "login_token": "[redacted]",
    "dev_token": "[redacted]",
    "dev_code": "[redacted]",
    "dev_model": "[redacted]",
    "signature": "",
    "player_signature": "",
    "friend_remarks": "",
    "sender_name": "Replay User",
    "sender_id": "0",
    "show_id": "0",
    "player_name": "Replay Player",
}


# --------------------------------------------------------------------------
# Sanitization
# --------------------------------------------------------------------------


def sanitize(value: object) -> object:
    """Apply the proxy redaction rules and replace replay markers."""
    if isinstance(value, dict):
        result = {}
        for key, item in value.items():
            if key in REDACTED_KEYS and isinstance(item, str):
                result[key] = REDACTED_KEYS[key]
                continue
            result[key] = sanitize(item)
        return result
    if isinstance(value, list):
        return [sanitize(item) for item in value]
    if isinstance(value, str) and value in MARKERS:
        return MARKERS[value]
    return value


def default_value(rust_type: str, structs: dict[str, MessageStruct]) -> object:
    """Build an empty placeholder that satisfies the generated struct."""
    inner = rust_type.strip()
    if inner.startswith("Option<") and inner.endswith(">"):
        return None
    if inner.startswith("Vec<") or inner.startswith("HashMap<"):
        return [] if inner.startswith("Vec<") else {}
    if inner == "bool":
        return False
    if inner == "String":
        return ""
    if inner in {
        "i8", "i16", "i32", "i64", "i128", "isize",
        "u8", "u16", "u32", "u64", "u128", "usize",
        "f32", "f64",
    }:
        return 0
    struct = structs.get(inner)
    if struct is not None:
        return {
            message_field.json_name: default_value(message_field.rust_type, structs)
            for message_field in struct.fields.values()
        }
    return 0


def render_default_sequence(struct_name: str, structs: dict[str, MessageStruct]) -> object:
    struct = structs.get(struct_name)
    if struct is None:
        return {}
    return {
        message_field.json_name: default_value(message_field.rust_type, structs)
        for message_field in struct.fields.values()
    }



# --------------------------------------------------------------------------
# Flow tables (progression/, battle/, chat/, mail/)
# --------------------------------------------------------------------------

# Attribute keys observed in the capture: 611 = player exp, 612 = gold coin,
# 600 = level, 608 = max exp.  Only 611/612 seed the delta ledger from the
# login base data; others rely on captured updates.
ATTR_KEY_EXP = 611
ATTR_KEY_GOLD = 612

FLOW_CMD_REWARD = {24022, 24065, 24098, 24221, 24270, 16007}


class AttrLedger:
    """Tracks last-known attribute values so reward deltas can be extracted."""

    def __init__(self):
        self.values = {}

    def seed_from_base_data(self, decoded):
        pairs = {
            ATTR_KEY_EXP: decoded.get("exp"),
            ATTR_KEY_GOLD: decoded.get("gold_coin"),
        }
        for key, value in pairs.items():
            if value is not None and key not in self.values:
                try:
                    self.values[key] = int(value)
                except (TypeError, ValueError):
                    pass

    def absorb(self, decoded):
        for attr in (decoded or {}).get("attr_list", []):
            try:
                self.values[int(attr["key"])] = int(attr["value"])
            except (KeyError, TypeError, ValueError):
                continue

    def extract(self, decoded):
        """Return (after, delta) entries for one attr-update payload."""
        after, delta = [], []
        for attr in (decoded or {}).get("attr_list", []):
            try:
                key = int(attr["key"])
                new_value = int(attr["value"])
            except (KeyError, TypeError, ValueError):
                continue
            after.append({"key": key, "value": str(new_value)})
            before = self.values.get(key)
            if before is not None:
                delta.append({"key": key, "value": str(new_value - before)})
            self.values[key] = new_value
        return after, delta


def reward_from_responses(responses, ledger):
    """Collect bag items, awards, attr updates and unread notices of a chain."""
    reward = {
        "award_list": [],
        "bag_items": [],
        "attr_list": [],
        "attr_delta": [],
        "unread": [],
    }
    for response in responses:
        decoded = response.decoded
        if decoded is None:
            continue
        if response.cmd == 10059:
            reward["unread"].append(
                {"type": decoded.get("type"), "id_list": decoded.get("id_list", [])}
            )
        elif response.cmd == 17001:
            reward["bag_items"].extend(decoded.get("updateList", []))
        elif response.cmd == 17013:
            reward["award_list"].extend(decoded.get("award_list", []))
        elif response.cmd == 24099:
            reward["award_list"].extend(decoded.get("award_list", []))
        elif response.cmd == 12003:
            after, delta = ledger.extract(decoded)
            reward["attr_list"].extend(after)
            reward["attr_delta"].extend(delta)
    return reward


def template_payload(responses):
    """Capture-style response list for TemplateFile (battle scripts etc.)."""
    template = []
    for response in responses:
        entry = {"cmd": response.cmd}
        if response.decoded is not None:
            entry["decoded"] = sanitize(response.decoded)
        elif response.payload_hex:
            entry["payload_hex"] = response.payload_hex
        else:
            continue  # undecoded and no raw bytes: nothing to script
        template.append(entry)
    return template


def find_response(group, cmd):
    return next((r for r in group.responses if r.cmd == cmd), None)


def write_flow_tables(groups, data_dir, written, skipped_existing, force):
    """Extract per-flow data tables from the capture."""
    ledger = AttrLedger()
    chat_channels = {}
    enclosure_unread = None
    achievement_awards = []
    seven_day = None
    open_server_sign = None
    gift_goods = []
    novice_training = None
    recruit_rewards = []
    recruit_times = None
    battle_enter = None
    battle_auto = None
    battle_video_end = []
    novice_training_panel = None

    def emit(relative_path, payload):
        target = data_dir / relative_path
        if target.exists() and not force:
            skipped_existing.append(relative_path)
            return
        if relative_path in written:
            return
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
        written.append(relative_path)

    for group in groups:
        request = group.request_decoded or {}
        for response in group.responses:
            if response.cmd == 12001 and response.decoded:
                ledger.seed_from_base_data(response.decoded)
            elif response.cmd in (12002, 12003) and response.cmd not in FLOW_CMD_REWARD:
                if group.request_cmd not in FLOW_CMD_REWARD | {24111}:
                    ledger.absorb(response.decoded)

        cmd = group.request_cmd

        if cmd == 10054:
            response = find_response(group, 10055)
            if response and response.decoded is not None:
                channel = response.decoded.get("channel")
                chat_channels.setdefault(channel, sanitize(response.decoded))
        elif cmd == 16007:
            enclosure_unread = reward_from_responses(group.responses, ledger)["unread"]
        elif cmd == 24022:
            gain = find_response(group, 24023)
            update = find_response(group, 24024)
            complete = find_response(group, 24027)
            if gain and gain.decoded:
                entry = {
                    "achievement_id": gain.decoded.get("achievement_id"),
                    "stage": gain.decoded.get("stage"),
                    "point": gain.decoded.get("point"),
                }
                entry.update(reward_from_responses(group.responses, ledger))
                if update and update.decoded:
                    entry["next"] = update.decoded.get("achievement_info")
                if complete and complete.decoded:
                    entry["complete"] = complete.decoded.get("complete_achieve_info", [])
                achievement_awards.append(entry)
        elif cmd == 24065:
            panel = find_response(group, 24066)
            entry = {"day": request.get("day")}
            entry.update(reward_from_responses(group.responses, ledger))
            seven_day = {
                "login_day": (panel.decoded or {}).get("login_day", 1) if panel else 1,
                "days": [entry],
            }
        elif cmd == 24270:
            panel = find_response(group, 24271)
            entry = {"day": request.get("day")}
            entry.update(reward_from_responses(group.responses, ledger))
            decoded = (panel.decoded or {}) if panel else {}
            open_server_sign = {
                "open_day": decoded.get("open_day", 1),
                "end_time": decoded.get("end_time", 0),
                "days": [entry],
            }
        elif cmd == 24098:
            entry = {"goods_id": request.get("goods_id"), "num": request.get("num", 1)}
            entry.update(reward_from_responses(group.responses, ledger))
            gift_goods.append(entry)
        elif cmd == 24111:
            receive = find_response(group, 24114)
            panel = find_response(group, 24112)
            if panel and panel.decoded is not None and novice_training_panel is None:
                novice_training_panel = sanitize(panel.decoded)
            if receive and receive.decoded is not None:
                reward = reward_from_responses(group.responses, ledger)
                novice_training = {
                    "tasks": [
                        {"id": task_id, **reward}
                        for task_id in receive.decoded.get("task_id_list", [])
                    ],
                    "panel": novice_training_panel,
                }
        elif cmd == 24221:
            result = find_response(group, 24222)
            if result and result.decoded is not None:
                entry = {"id": result.decoded.get("id")}
                entry.update(reward_from_responses(group.responses, ledger))
                recruit_rewards.append(entry)
        elif cmd == 18006:
            response = find_response(group, 24220)
            if response and response.decoded is not None and recruit_times is None:
                recruit_times = response.decoded.get("recruit_times", 0)
        elif cmd == 20100 and battle_enter is None:
            battle_enter = {"groups": [template_payload(group.responses)]}
        elif cmd == 20113 and battle_auto is None:
            battle_auto = {"groups": [template_payload(group.responses)]}
        elif cmd == 20104:
            battle_video_end.append(template_payload(group.responses))

    if chat_channels:
        emit("chat/public_chat.json", {"channels": list(chat_channels.values())})
    if enclosure_unread is not None:
        emit("mail/enclosure_unread.json", enclosure_unread)
    if achievement_awards:
        emit("progression/achievement.json", {"awards": achievement_awards})
    if seven_day:
        emit("progression/seven_day.json", seven_day)
    if open_server_sign:
        emit("progression/open_server_sign.json", open_server_sign)
    if gift_goods:
        emit("progression/direct_gift.json", {"goods": gift_goods})
    if novice_training and novice_training.get("tasks"):
        novice_training.setdefault("recruit_times", recruit_times or 0)
        emit("progression/novice_training.json", novice_training)
    if recruit_rewards:
        emit(
            "progression/novice_recruit.json",
            {"rewards": recruit_rewards, "recruit_times": recruit_times or 0},
        )
    if battle_enter:
        emit("battle/enter.json", battle_enter)
    if battle_auto:
        emit("battle/auto.json", battle_auto)
    if battle_video_end:
        emit("battle/video_end.json", {"groups": battle_video_end})


# --------------------------------------------------------------------------
# Main
# --------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("capture", type=Path, help="proxy JSONL capture")
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--data-dir", type=Path, default=None,
                        help="output directory (default: <repo>/data)")
    parser.add_argument("--fill-defaults", action="store_true",
                        help="write empty placeholder panels for files the capture lacks")
    parser.add_argument("--all", action="store_true",
                        help="also dump every response group under <data-dir>/capture/")
    parser.add_argument("--only", action="append", default=None, metavar="PREFIX",
                        help="only write loader files under this path prefix "
                             "(repeatable, e.g. --only hero_biography/)")
    parser.add_argument("--force", action="store_true",
                        help="overwrite files that already exist")
    args = parser.parse_args(argv)

    repo: Path = args.repo
    data_dir: Path = args.data_dir or repo / "data"
    structs = parse_messages(repo / "tcpserver/src/messages.rs")
    expectations = parse_data_loader(repo / "tcpserver/src/data_loader.rs")
    groups = parse_capture(args.capture)

    if args.only:
        expectations = [
            expectation
            for expectation in expectations
            if any(expectation.path.startswith(prefix) for prefix in args.only)
        ]

    by_cmd: dict[int, list[object]] = {}
    for group in groups:
        for response in group.responses:
            by_cmd.setdefault(response.cmd, []).append(group)

    written: list[str] = []
    skipped_existing: list[str] = []
    mismatched: list[str] = []
    filled: list[str] = []
    dumps: list[str] = []
    unmapped_cmds: set[int] = set()

    def write_json(relative_path: str, payload: object, bucket: list[str]) -> None:
        if relative_path in written or relative_path in filled or relative_path in dumps:
            return  # already written by an equivalent loader entry
        target = data_dir / relative_path
        if target.exists() and not args.force:
            skipped_existing.append(relative_path)
            return
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
        bucket.append(relative_path)

    covered_cmds: set[int] = set()
    covered_paths: set[str] = set()

    for expectation in expectations:
        if expectation.raw:
            for group in by_cmd.get(expectation.cmd, []):
                response = next(
                    (r for r in group.responses if r.cmd == expectation.cmd), None
                )
                if response is None:
                    continue
                hex_payload = response.payload_hex or "00" * (response.payload_len or 0)
                write_json(expectation.path, {"payload_hex": hex_payload}, written)
                covered_cmds.add(expectation.cmd)
                covered_paths.add(expectation.path)
                break
            continue

        candidate_groups = []
        for group in by_cmd.get(expectation.cmd, []):
            matching_response = next(
                (
                    response
                    for response in group.responses
                    if response.cmd == expectation.cmd
                    and response.name == expectation.struct
                    and response.decoded is not None
                ),
                None,
            )
            if matching_response is None:
                continue
            if expectation.selector is not None:
                field_name, value = expectation.selector
                if value is None:
                    continue  # fallback file: only needed when no variant matches
                if str((group.request_decoded or {}).get(field_name)) != value:
                    continue
            candidate_groups.append((group, matching_response))

        if not candidate_groups:
            continue

        # Group responses are replayed in capture order, so the first matching
        # group is the one the client would use for this data file.
        _, response = candidate_groups[0]
        write_json(expectation.path, sanitize(response.decoded), written)
        covered_cmds.add(expectation.cmd)
        covered_paths.add(expectation.path)

    for expectation in expectations:
        captured_names = {
            response.name
            for group in by_cmd.get(expectation.cmd, [])
            for response in group.responses
        }
        if expectation.raw:
            continue
        if captured_names and expectation.struct not in captured_names:
            mismatched.append(
                f"{expectation.path} (loader expects {expectation.struct} for cmd "
                f"{expectation.cmd}, capture has {', '.join(sorted(captured_names))})"
            )

    flow_written_before = len(written)
    write_flow_tables(groups, data_dir, written, skipped_existing, args.force)
    flow_files = len(written) - flow_written_before

    if args.fill_defaults:
        for expectation in expectations:
            if expectation.path in covered_paths:
                continue
            payload = render_default_sequence(expectation.struct, structs)
            if not payload:
                continue
            write_json(expectation.path, payload, filled)

    for cmd, cmd_groups in by_cmd.items():
        if cmd not in covered_cmds:
            unmapped_cmds.add(cmd)
        if args.all:
            dump: list[object] = []
            for index, group in enumerate(cmd_groups, 1):
                dump.append(
                    {
                        "client_request": {
                            "cmd": group.request_cmd,
                            "name": group.request_name,
                            "decoded": sanitize(group.request_decoded),
                        },
                        "server_responses": [
                            {
                                "cmd": response.cmd,
                                "name": response.name,
                                "decoded": sanitize(response.decoded),
                            }
                            for response in group.responses
                        ],
                    }
                )
            write_json(f"capture/cmd_{cmd}.json", dump, dumps)

    print(f"data directory: {data_dir}")
    print(f"loader expectations: {len(expectations)} files")
    print(f"written from capture: {len(written) - flow_files}")
    print(f"flow tables (progression/battle/chat/mail): {flow_files}")
    if dumps:
        print(f"response-group dumps: {len(dumps)} (under capture/)")
    if filled:
        print(f"placeholder files:   {len(filled)}")
        for path in filled:
            print(f"  ~ {path}")
    if skipped_existing:
        print(f"kept existing (use --force to overwrite): {len(skipped_existing)}")
    if mismatched:
        print("loader/capture struct mismatches:")
        for line in mismatched:
            print(f"  ! {line}")
    if unmapped_cmds:
        print("captured commands not written to a loader file by this run "
              "(see --all for response dumps): "
              + ", ".join(str(cmd) for cmd in sorted(unmapped_cmds)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
