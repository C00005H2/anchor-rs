#!/usr/bin/env python3
"""Report how much of a decoded proxy capture the tcpserver emulator can serve.

The proxy writes request/response groups to ``requests_YYYYMMDD.jsonl`` (see
``tcpserver/src/proxy.rs``).  This script cross-references such a capture with
the Rust sources and answers, per captured command:

* does the client request have a normal-mode handler in ``handle.rs``?
* is the request payload decodable with the catalogued message structs?
* can each captured server response be re-encoded by ``capture_replay.rs``?
* do the recorded JSON fields match the Rust struct schema (i.e. would the
  replay deserialization succeed at runtime)?
* is there a ``data_loader.rs`` slot (JSON data file) that could serve the
  response in normal (non-replay) mode?

Usage::

    python3 tools/capture_coverage.py requests_20261005_new.jsonl \
        --markdown docs/capture_coverage.md --json coverage.json

Exit status is 0; findings are meant to be read, not to fail a build.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter, OrderedDict, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

# --------------------------------------------------------------------------
# Rust source parsing helpers
# --------------------------------------------------------------------------

STRUCT_RE = re.compile(r"^pub struct ([A-Za-z0-9_]+)\s*\{", re.M)
FIELD_RE = re.compile(r"^\s*pub ([a-z0-9_]+): (.+?),?\s*$", re.M)
IMPL_RE = re.compile(r"^impl ([A-Za-z0-9_]+)\s*\{", re.M)
FN_RE = re.compile(r"pub fn (decode|encode)\s*\(")
SERDE_RENAME_RE = re.compile(r'#\[serde\(rename\s*=\s*"([^"]+)"\)\]')
SERDE_DEFAULT_RE = re.compile(r"#\[serde\([^)]*default[^)]*\)\]")


@dataclass
class MessageField:
    name: str
    rust_type: str
    json_name: str
    required: bool = True


@dataclass
class MessageStruct:
    name: str
    fields: "OrderedDict[str, MessageField]" = field(default_factory=OrderedDict)
    has_decode: bool = False
    has_encode: bool = False
    line: int = 0


def parse_messages(path: Path) -> dict[str, MessageStruct]:
    """Parse ``messages.rs`` into struct name -> fields/methods.

    Honours ``#[serde(rename = "...")]`` and ``#[serde(default)]`` attributes so
    the JSON field check matches what serde actually accepts at runtime.
    """
    text = path.read_text(encoding="utf-8")
    structs: dict[str, MessageStruct] = {}

    matches = list(STRUCT_RE.finditer(text))
    for match in matches:
        name = match.group(1)
        body_start = match.end()
        # Struct bodies are flat: the first line at column 0 that is "}".
        body_end = text.find("\n}", body_start)
        if body_end == -1:
            body_end = len(text)
        body = text[body_start:body_end]
        fields: "OrderedDict[str, MessageField]" = OrderedDict()
        pending_rename: str | None = None
        pending_default = False
        for line in body.splitlines():
            stripped = line.strip()
            rename_match = SERDE_RENAME_RE.search(stripped)
            if rename_match:
                pending_rename = rename_match.group(1)
                continue
            if SERDE_DEFAULT_RE.search(stripped):
                pending_default = True
                continue
            field_match = re.match(r"pub ([a-z0-9_]+): (.+?),?\s*$", stripped)
            if not field_match:
                continue
            field_name = field_match.group(1)
            fields[field_name] = MessageField(
                name=field_name,
                rust_type=field_match.group(2).strip(),
                json_name=pending_rename or field_name,
                required=not pending_default,
            )
            pending_rename = None
            pending_default = False
        structs[name] = MessageStruct(
            name=name, fields=fields, line=text.count("\n", 0, match.start()) + 1
        )

    for impl_match in IMPL_RE.finditer(text):
        target = structs.get(impl_match.group(1))
        if target is None:
            continue
        body_start = impl_match.end()
        # Stop at the next top-level item after the impl block.
        next_item = min(
            (
                position
                for position in (
                    text.find("\nimpl ", body_start),
                    text.find("\n#[", body_start),
                    text.find("\npub struct ", body_start),
                )
                if position != -1
            ),
            default=len(text),
        )
        body = text[body_start:next_item]
        for fn_match in FN_RE.finditer(body):
            if fn_match.group(1) == "decode":
                target.has_decode = True
            else:
                target.has_encode = True

    return structs


@dataclass
class LoaderEntry:
    struct: str
    path: str
    cmd: int
    # Loader function that reads this file (e.g. ``load_hero_biography_sequence``).
    sequence: str = ""
    # (request field, expected value) when the data file depends on the request;
    # ``None`` as the value marks a fallback file (e.g. ``shop_type_default``).
    selector: tuple[str, str | None] | None = None


def parse_data_loader(path: Path) -> list[LoaderEntry]:
    """Collect every JSON file ``data_loader.rs`` expects.

    Covers literal ``load_packet!(Struct, "path", cmd)`` calls, ``for x in a..=b``
    loops, ``Self::build_packet::<Struct>("path", cmd)`` calls, and paths chosen
    by a ``let path = match request_field { .. }`` table (shop/dialogue files).
    Each ``pub fn`` body is analysed separately so identically named locals in
    different functions do not leak into each other.
    """
    text = path.read_text(encoding="utf-8")
    functions = list(re.finditer(r"\n    pub fn (\w+)", text))
    starts = [match.start() for match in functions] + [len(text)]
    entries: list[LoaderEntry] = []

    for index, function in enumerate(functions):
        sequence = function.group(1)
        chunk = text[starts[index]:starts[index + 1]]
        loop_spans: list[tuple[int, int]] = []

        for match in re.finditer(
            r"for\s+(\w+)\s+in\s+(\d+)\.\.=(\d+)\s*\{(.*?)\n\s*\}", chunk, re.S
        ):
            variable, start, end, body = (
                match.group(1),
                int(match.group(2)),
                int(match.group(3)),
                match.group(4),
            )
            loop_spans.append((match.start(), match.end()))
            for entry in re.finditer(
                r'load_packet!\(\s*([A-Za-z0-9_]+)\s*,\s*&format!\("([^"]+)"\s*,\s*(\w+)\s*\)\s*,\s*(\d+)',
                body,
            ):
                if entry.group(3) != variable:
                    continue
                for value in range(start, end + 1):
                    entries.append(
                        LoaderEntry(
                            struct=entry.group(1),
                            path=entry.group(2).replace("{}", str(value)),
                            cmd=int(entry.group(4)),
                            sequence=sequence,
                        )
                    )

        stripped = chunk
        for span_start, span_end in reversed(loop_spans):
            stripped = stripped[:span_start] + stripped[span_end:]

        for match in re.finditer(
            r'load_packet!\(\s*([A-Za-z0-9_]+)\s*,\s*"([^"]+)"\s*,\s*(\d+)', stripped
        ):
            entries.append(
                LoaderEntry(
                    struct=match.group(1),
                    path=match.group(2),
                    cmd=int(match.group(3)),
                    sequence=sequence,
                )
            )

        for match in re.finditer(
            r'build_packet::<([A-Za-z0-9_]+)>\(\s*"([^"]+)"\s*,\s*(\d+)\s*\)', stripped
        ):
            entries.append(
                LoaderEntry(
                    struct=match.group(1),
                    path=match.group(2),
                    cmd=int(match.group(3)),
                    sequence=sequence,
                )
            )

        # let <var> = match <field or tuple> { <pattern> => "path", .., _ => "path" };
        for match in re.finditer(
            r"let\s+(\w+)\s*=\s*match\s+([^\{]+?)\s*\{(.*?)\n\s*\};", stripped, re.S
        ):
            variable, scrutinee, arms = match.group(1), match.group(2), match.group(3)
            field_match = re.search(r"[a-z_][a-z0-9_]*", scrutinee)
            if not field_match:
                continue
            field_name = field_match.group(0)
            arm_paths: list[tuple[str | None, str]] = []
            for arm in re.findall(r"([^\n=]+?)\s*=>\s*\"([^\"]+)\"", arms):
                pattern, arm_path = arm[0].strip(), arm[1]
                if pattern.startswith("_"):
                    arm_paths.append((None, arm_path))
                    continue
                literal = re.search(r"[A-Za-z0-9_]+", pattern)
                if literal:
                    arm_paths.append((literal.group(0), arm_path))
            if not arm_paths:
                continue
            for usage in re.finditer(
                r"load_packet!\(\s*([A-Za-z0-9_]+)\s*,\s*" + re.escape(variable) + r"\s*,\s*(\d+)",
                stripped,
            ):
                for value, arm_path in arm_paths:
                    entries.append(
                        LoaderEntry(
                            struct=usage.group(1),
                            path=arm_path,
                            cmd=int(usage.group(2)),
                            selector=(field_name, value),
                            sequence=sequence,
                        )
                    )

    # Drop duplicates produced by the same file being loaded from two sequences.
    unique: dict[tuple, LoaderEntry] = {}
    for entry in entries:
        unique.setdefault((entry.path, entry.cmd, entry.selector), entry)
    return list(unique.values())


def parse_handler_loader_calls(handle_path: Path, cmd_dir: Path) -> dict[int, set[str]]:
    """Map handled command ids to the ``GameDataLoader`` functions they reach.

    ``handle.rs`` arms call into ``cmd::<module>::<handler>``, and those handlers
    call ``GameDataLoader::<loader>``.  Both hops are resolved here so the report
    knows which data-file sequences a capture actually activates.
    """
    handler_loaders: dict[str, set[str]] = {}
    for source in sorted(cmd_dir.glob("*.rs")):
        text = source.read_text(encoding="utf-8")
        functions = list(re.finditer(r"pub async fn (\w+)", text))
        starts = [match.start() for match in functions] + [len(text)]
        for index, function in enumerate(functions):
            body = text[starts[index]:starts[index + 1]]
            names = set(re.findall(r"GameDataLoader::(\w+)", body))
            if names:
                handler_loaders.setdefault(function.group(1), set()).update(names)

    text = handle_path.read_text(encoding="utf-8")
    arms = list(re.finditer(r"^\s*(\d{4,6}) => \{", text, re.M))
    calls: dict[int, set[str]] = {}
    for index, arm in enumerate(arms):
        end = arms[index + 1].start() if index + 1 < len(arms) else len(text)
        body = text[arm.end():end]
        names: set[str] = set()
        for module_name, function_name in re.findall(r"(\w+)::(\w+)\s*\(", body):
            if function_name in handler_loaders:
                names.update(handler_loaders[function_name])
        if names:
            calls.setdefault(int(arm.group(1)), set()).update(names)
    return calls


@dataclass
class HandlerEntry:
    cmd: int
    line: int
    kind: str  # "direct" | "replay-aware"


def parse_handlers(path: Path) -> dict[int, HandlerEntry]:
    """Find ``NUM => {`` arms in handle.rs' dispatch match statements."""
    text = path.read_text(encoding="utf-8")
    handlers: dict[int, HandlerEntry] = {}
    for match in re.finditer(r"^(\s*)(\d{4,6}) => \{", text, re.M):
        cmd = int(match.group(2))
        line = text.count("\n", 0, match.start()) + 1
        handlers.setdefault(cmd, HandlerEntry(cmd=cmd, line=line, kind="direct"))
    return handlers


def parse_encode_schemas(path: Path) -> dict[int, str]:
    """Collect ``NUM => encode_as!(Struct)`` arms from capture_replay.rs."""
    text = path.read_text(encoding="utf-8")
    return {
        int(match.group(1)): match.group(2)
        for match in re.finditer(r"^\s*(\d+) => encode_as!\(([A-Za-z0-9_]+)\)", text, re.M)
    }


def parse_msgid_names(path: Path) -> dict[int, str]:
    """Map numeric command ids to their ``MsgId`` enum names."""
    text = path.read_text(encoding="utf-8")
    names: dict[int, str] = {}
    for match in re.finditer(r"^\s*([A-Z][A-Z0-9_]*)\s*=\s*(\d+)\s*,", text, re.M):
        names[int(match.group(2))] = match.group(1)
    return names


@dataclass
class CommandReport:
    cmd: int
    name: str
    calls: int
    request_decoded: bool
    handler: HandlerEntry | None
    responses: list[ResponseReport]
    tier: str


@dataclass
class ResponseReport:
    cmd: int
    name: str
    count: int
    status: str
    schema_struct: str | None
    problems: list[str]
    loader_paths: list[str]

    @property
    def replayable(self) -> bool:
        return self.status == "ok"


# --------------------------------------------------------------------------
# Schema compatibility checks
# --------------------------------------------------------------------------

NUMBER_TYPES = {
    "i8", "i16", "i32", "i64", "i128", "isize",
    "u8", "u16", "u32", "u64", "u128", "usize",
    "f32", "f64",
}


def json_kind(value: object) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "bool"
    if isinstance(value, (int, float)):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    return "object"


def match_type(rust_type: str, value: object, structs: dict[str, MessageStruct]) -> str | None:
    """Return None when the JSON value fits the Rust type, else a human hint.

    This is a best-effort static check for the generated message structs: it
    catches the common failure mode where a captured field is an object/array
    but the struct expects a scalar (or vice versa), which would make replay
    deserialization fail at runtime.
    """
    kind = json_kind(value)

    if rust_type.startswith("Option<"):
        if value is None:
            return None
        return match_type(rust_type[7:-1].strip(), value, structs)

    if rust_type.startswith("Vec<") and rust_type.endswith(">"):
        if kind != "array":
            return f"expected array, captured {kind}"
        inner = rust_type[4:-1].strip()
        for item in value:
            problem = match_type(inner, item, structs)
            if problem:
                return f"array item: {problem}"
        return None

    if rust_type.startswith("HashMap<") and rust_type.endswith(">"):
        return None if kind in ("object", "null") else f"expected object map, captured {kind}"

    if rust_type in NUMBER_TYPES:
        return None if kind == "number" else f"expected number, captured {kind}"
    if rust_type == "bool":
        return None if kind == "bool" else f"expected bool, captured {kind}"
    if rust_type == "String":
        return None if kind == "string" else f"expected string, captured {kind}"

    struct = structs.get(rust_type)
    if struct is not None:
        if kind != "object":
            return f"expected object ({rust_type}), captured {kind}"
        for message_field in struct.fields.values():
            if message_field.required and message_field.json_name not in value:
                return f"missing field {message_field.json_name} of {rust_type}"
        return None

    return f"unrecognized type {rust_type} (captured {kind})"


def check_response_schema(
    cmd: int,
    decoded: object,
    encode_schemas: dict[int, str],
    structs: dict[str, MessageStruct],
) -> tuple[str, list[str]]:
    """Classify a captured response payload."""
    if decoded is None:
        return "undecoded", []
    struct_name = encode_schemas.get(cmd)
    if not struct_name:
        return "no_encode_schema", []
    struct = structs.get(struct_name)
    if struct is None:
        return "missing_struct", [f"{struct_name} not found in messages.rs"]
    if not struct.has_encode:
        return "missing_encoder", [f"{struct_name} has no encode() method"]
    if not isinstance(decoded, dict):
        return "bad_shape", [f"captured payload is {json_kind(decoded)}, expected object"]

    problems: list[str] = []
    for message_field in struct.fields.values():
        if message_field.json_name not in decoded:
            if message_field.required:
                problems.append(
                    f"missing captured field {message_field.json_name}: {message_field.rust_type}"
                )
            continue
        problem = match_type(
            message_field.rust_type, decoded[message_field.json_name], structs
        )
        if problem:
            problems.append(f"{message_field.json_name}: {problem}")
    return ("schema_mismatch" if problems else "ok"), problems


def classify_tier(
    cmd: int, name: str, handler: HandlerEntry | None, responses: list[ResponseReport],
    replay_enabled_tier: str,
) -> str:
    if handler is not None:
        return "implemented"
    if any(response.status == "undecoded" for response in responses):
        return "blocked-undecoded-response"
    if not responses:
        return "state-only (no captured response)"
    if any(not response.replayable for response in responses):
        return "needs-replay-encoder"
    return replay_enabled_tier


@dataclass
class CapturedResponse:
    cmd: int
    name: str | None
    payload_len: int | None
    decoded: object
    payload_hex: str | None = None


@dataclass
class CapturedGroup:
    request_cmd: int
    request_name: str | None
    request_decoded: object
    request_payload_len: int | None
    responses: list[CapturedResponse]


def parse_capture(path: Path) -> list[CapturedGroup]:
    groups: list[CapturedGroup] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as error:
            raise SystemExit(f"{path}:{line_number}: invalid JSON: {error}") from error

        request = row.get("client_request", {})
        responses = [
            CapturedResponse(
                cmd=response.get("cmd"),
                name=response.get("name"),
                payload_len=response.get("payload_len"),
                decoded=response.get("decoded"),
                payload_hex=response.get("payload_hex"),
            )
            for response in row.get("server_responses", [])
            if isinstance(response.get("cmd"), int)
        ]
        groups.append(
            CapturedGroup(
                request_cmd=request.get("cmd"),
                request_name=request.get("name"),
                request_decoded=request.get("decoded"),
                request_payload_len=request.get("payload_len"),
                responses=responses,
            )
        )
    return groups


def build_report(repo: Path, capture_path: Path) -> dict:
    messages_path = repo / "tcpserver/src/messages.rs"
    structs = parse_messages(messages_path)
    loader_entries = parse_data_loader(repo / "tcpserver/src/data_loader.rs")
    handlers = parse_handlers(repo / "tcpserver/src/handle.rs")
    encode_schemas = parse_encode_schemas(repo / "tcpserver/src/capture_replay.rs")
    msgid_names = parse_msgid_names(repo / "tcpserver/src/msgid.rs")
    groups = parse_capture(capture_path)

    loader_by_cmd: dict[int, list[LoaderEntry]] = defaultdict(list)
    for entry in loader_entries:
        loader_by_cmd[entry.cmd].append(entry)

    # Aggregate captured responses per command.
    response_stats: dict[int, dict] = {}
    for group in groups:
        for response in group.responses:
            stats = response_stats.setdefault(
                response.cmd,
                {
                    "name": response.name,
                    "count": 0,
                    "decoded": response.decoded,
                    "payload_len": response.payload_len,
                    "has_raw": False,
                },
            )
            stats["count"] += 1
            if stats["decoded"] is None and response.decoded is not None:
                stats["decoded"] = response.decoded
            if response.payload_hex:
                stats["has_raw"] = True

    response_reports: dict[int, ResponseReport] = {}
    for cmd, stats in response_stats.items():
        status, problems = check_response_schema(
            cmd, stats["decoded"], encode_schemas, structs
        )
        if status == "undecoded" and stats["has_raw"]:
            status = "undecoded_raw"
        response_reports[cmd] = ResponseReport(
            cmd=cmd,
            name=stats["name"] or msgid_names.get(cmd) or f"UNKNOWN({cmd})",
            count=stats["count"],
            status=status,
            schema_struct=encode_schemas.get(cmd),
            problems=problems,
            loader_paths=[entry.path for entry in loader_by_cmd.get(cmd, [])],
        )

    command_reports: list[CommandReport] = []
    for request_cmd, request_name in sorted(
        {
            (group.request_cmd, group.request_name)
            for group in groups
            if isinstance(group.request_cmd, int)
        }
    ):
        calls = sum(1 for group in groups if group.request_cmd == request_cmd)
        response_cmds = {
            response.cmd
            for group in groups
            if group.request_cmd == request_cmd
            for response in group.responses
            if response.cmd in response_reports
        }
        responses = [response_reports[cmd] for cmd in sorted(response_cmds)]
        handler = handlers.get(request_cmd)
        command_reports.append(
            CommandReport(
                cmd=request_cmd,
                name=request_name or msgid_names.get(request_cmd) or f"UNKNOWN({request_cmd})",
                calls=calls,
                request_decoded=any(
                    group.request_decoded is not None
                    for group in groups
                    if group.request_cmd == request_cmd
                ),
                handler=handler,
                responses=responses,
                tier=classify_tier(
                    request_cmd, request_name or "", handler, responses, "replay-only"
                ),
            )
        )

    # Request fields that select a data file (e.g. shop_type => shop_type_1.json).
    selector_values: dict[int, set[tuple[str, str]]] = defaultdict(set)
    for group in groups:
        if not isinstance(group.request_decoded, dict):
            continue
        for response in group.responses:
            for field_name, value in group.request_decoded.items():
                selector_values[response.cmd].add((field_name, str(value)))

    def loader_entry_covered(entry: LoaderEntry) -> bool:
        if entry.selector is None:
            return entry.cmd in response_stats
        field_name, value = entry.selector
        if value is None:  # fallback file, only needed when no variant matches
            return False
        return (field_name, value) in selector_values.get(entry.cmd, set())

    # A loader function matters when a captured command reaches it, or when it
    # reads a message id that appears in the capture.
    loader_calls = parse_handler_loader_calls(
        repo / "tcpserver/src/handle.rs", repo / "tcpserver/src/cmd"
    )
    captured_cmds = {group.request_cmd for group in groups}
    active_sequences = {
        sequence
        for cmd in captured_cmds
        for sequence in loader_calls.get(cmd, set())
    }
    required_loader = [
        entry
        for entry in loader_entries
        if entry.cmd in response_stats or entry.sequence in active_sequences
    ]
    covered_loader = [entry for entry in required_loader if loader_entry_covered(entry)]
    missing_loader = [
        entry
        for entry in required_loader
        if not loader_entry_covered(entry)
        and not (entry.selector is not None and entry.selector[1] is None)
    ]

    return {
        "capture": str(capture_path),
        "repo": str(repo),
        "totals": {
            "request_groups": len(groups),
            "distinct_client_commands": len(command_reports),
            "distinct_server_commands": len(response_stats),
            "captured_server_packets": sum(stats["count"] for stats in response_stats.values()),
            "message_structs": len(structs),
        },
        "commands": [
            {
                "cmd": report.cmd,
                "name": report.name,
                "calls": report.calls,
                "request_decoded": report.request_decoded,
                "handler": report.handler is not None,
                "handler_line": report.handler.line if report.handler else None,
                "tier": report.tier,
                "responses": [
                    {
                        "cmd": response.cmd,
                        "name": response.name,
                        "count": response.count,
                        "status": response.status,
                        "schema_struct": response.schema_struct,
                        "problems": response.problems,
                        "loader_paths": response.loader_paths,
                    }
                    for response in report.responses
                ],
            }
            for report in command_reports
        ],
        "loader": {
            "expected_files": len(loader_entries),
            "required_for_capture": len(required_loader),
            "covered_by_capture": len(covered_loader),
            "missing_from_capture": [
                {"cmd": entry.cmd, "struct": entry.struct, "path": entry.path}
                for entry in missing_loader
            ],
        },
    }


# --------------------------------------------------------------------------
# Rendering
# --------------------------------------------------------------------------

STATUS_LABEL = {
    "ok": "re-encodable",
    "schema_mismatch": "SCHEMA MISMATCH",
    "no_encode_schema": "no encoder",
    "undecoded": "payload not decoded",
    "missing_struct": "struct missing",
    "missing_encoder": "encode() missing",
    "bad_shape": "unexpected JSON shape",
    "undecoded_raw": "raw bytes stored (schema unknown)",
}

TIER_LABEL = {
    "implemented": "already handled",
    "replay-only": "data-only handler feasible",
    "state-only (no captured response)": "needs game-state logic",
    "needs-replay-encoder": "blocked by missing replay encoder",
    "blocked-undecoded-response": "blocked by undecoded response",
}


def render_markdown(report: dict) -> str:
    totals = report["totals"]
    lines: list[str] = []
    lines.append("# Capture coverage\n")
    lines.append(
        f"Capture: `{report['capture']}` — {totals['request_groups']} request groups, "
        f"{totals['distinct_client_commands']} client commands, "
        f"{totals['distinct_server_commands']} server message types "
        f"({totals['captured_server_packets']} packets).\n"
    )
    lines.append("## Client commands\n")
    lines.append("| cmd | command | calls | handler | tier | response chain |")
    lines.append("| --- | --- | ---: | --- | --- | --- |")
    for command in report["commands"]:
        chain = ", ".join(
            f"`{response['cmd']}`{'' if response['status'] == 'ok' else ' ⚠'}"
            for response in command["responses"]
        ) or "—"
        lines.append(
            "| {cmd} | {name} | {calls} | {handler} | {tier} | {chain} |".format(
                cmd=command["cmd"],
                name=command["name"],
                calls=command["calls"],
                handler="yes" if command["handler"] else "no",
                tier=TIER_LABEL.get(command["tier"], command["tier"]),
                chain=chain,
            )
        )

    lines.append("\n## Server responses\n")
    lines.append("| cmd | message | packets | replay encode | schema problems | data file |")
    lines.append("| --- | --- | ---: | --- | --- | --- |")
    for command in report["commands"]:
        for response in command["responses"]:
            lines.append(
                "| {cmd} | {name} | {count} | {status} | {problems} | {paths} |".format(
                    cmd=response["cmd"],
                    name=response["name"],
                    count=response["count"],
                    status=STATUS_LABEL.get(response["status"], response["status"]),
                    problems="; ".join(response["problems"]) or "—",
                    paths=", ".join(f"`{path}`" for path in response["loader_paths"]) or "—",
                )
            )

    loader = report["loader"]
    lines.append("\n## Data-loader coverage\n")
    lines.append(
        f"{loader['covered_by_capture']} of {loader['required_for_capture']} JSON data files "
        f"for commands present in this capture can be produced from it "
        f"({loader['expected_files']} files are declared by `data_loader.rs` in total)."
    )
    if loader["missing_from_capture"]:
        lines.append("\nNot present in the capture:\n")
        for entry in loader["missing_from_capture"]:
            lines.append(f"- `{entry['path']}` ({entry['struct']}, cmd {entry['cmd']})")

    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("capture", type=Path, help="proxy JSONL capture")
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--json", type=Path, help="write the machine-readable report here")
    parser.add_argument("--markdown", type=Path, help="write a markdown report here")
    args = parser.parse_args(argv)

    report = build_report(args.repo, args.capture)

    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(report, indent=2), encoding="utf-8")
    if args.markdown:
        args.markdown.parent.mkdir(parents=True, exist_ok=True)
        args.markdown.write_text(render_markdown(report), encoding="utf-8")

    totals = report["totals"]
    print(
        f"{totals['request_groups']} request groups | "
        f"{totals['distinct_client_commands']} client commands | "
        f"{totals['distinct_server_commands']} server message types"
    )
    tiers = Counter(command["tier"] for command in report["commands"])
    for tier, count in tiers.most_common():
        print(f"  {count:>2}  {TIER_LABEL.get(tier, tier)}")
    statuses = Counter(
        response["status"]
        for command in report["commands"]
        for response in command["responses"]
    )
    print("response payloads:")
    for status, count in statuses.most_common():
        print(f"  {count:>2}  {STATUS_LABEL.get(status, status)}")
    loader = report["loader"]
    print(
        f"data files: {loader['covered_by_capture']}/{loader['required_for_capture']} files for "
        f"captured commands ({loader['expected_files']} declared in total)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
