#!/usr/bin/env python3
"""Convert a VCD waveform dump to a SAIF 2.0 activity file.

Aggregates each scalar signal's history over [first_ts, last_ts] of the dump
into SAIF duration/count fields: T0/T1/TX/TZ (time per state), TC (clean
0<->1 toggle count, every in-window edge counted), TB/IG (emitted 0).
Vector signals and real-valued changes bail loudly: mb.vcd is all scalars.

The scope tree mirrors the VCD `$scope` hierarchy and every signal lands in
a `NET` group, the only form OpenSTA 3.0.16 annotates. An optional
`--ghost-scope <path>` adds an unmatched `(INSTANCE ghost ...)` to exercise
annotation-drop diagnostics; `--window-end <ts>` truncates the aggregation
window (and DURATION) before the dump's last timestamp.
"""

import argparse
import sys

UNITS = {"s": 1.0, "ms": 1e-3, "us": 1e-6, "ns": 1e-9, "ps": 1e-12, "fs": 1e-15}


def die(msg):
    sys.exit(f"vcd2saif: {msg}")


def parse_vcd(path):
    """Returns (scope_tree, ts_num, ts_unit, first_ts, last_ts).

    scope_tree: {name: {"signals": [decl], "children": {name: subtree}}}
    decl: {"name": str, "id": str, "events": [(ts, state)]}  state in {0,1,2,3}
    (ts_num, ts_unit): the $timescale emission pair ("1ns" -> ("1", "ns")).
    All timestamps/durations are in VCD-timestamp units, which equal SAIF
    TIMESCALE units by construction — no rescaling anywhere.
    """
    decls = {}  # id -> decl
    scopes = {}  # tuple path -> node
    root = {"signals": [], "children": {}}
    cur_path = []
    in_defs = True
    pending = ""
    ts = 0
    first_ts = None
    last_ts = None
    timescale = None

    def node_for(path):
        node = root
        for part in path:
            node = node["children"].setdefault(part, {"signals": [], "children": {}})
        return node

    def handle_item(item):
        nonlocal cur_path
        words = item.split()
        if not words:
            return
        if words[0] == "$scope":
            cur_path.append(words[2])
        elif words[0] == "$upscope":
            cur_path.pop()
        elif words[0] == "$timescale":
            nonlocal timescale
            spec = item.split("$timescale", 1)[1].split("$end", 1)[0].split("/")[0].strip()
            i = next((k for k, c in enumerate(spec) if c.isalpha()), len(spec))
            num, unit = spec[:i].strip(), spec[i:].strip().lower()
            if unit not in UNITS:
                die(f"unknown $timescale unit {unit!r}")
            float(num)  # numeric validation only; the pair is emitted verbatim
            timescale = (num, unit)
        elif words[0] == "$var":
            # $var <type> <size> <id> <name> [msb lsb] $end
            kind, size, ident, name = words[1], words[2], words[3], words[4]
            if int(size) != 1:
                die(f"vector signal {name} ({size} bits) unsupported")
            decl = {"name": name.removesuffix("$end"), "id": ident, "events": []}
            decls[ident] = decl
            node_for(cur_path)["signals"].append(decl)

    with open(path) as f:
        for raw in f:
            line = raw.strip()
            if not line:
                continue
            if pending:
                pending += " " + line
                if "$end" in line:
                    handle_item(pending)
                    pending = ""
                continue
            if in_defs:
                if line.startswith("$enddefinitions"):
                    in_defs = False
                elif line.startswith("$"):
                    if "$end" in line:
                        handle_item(line)
                    else:
                        pending = line
                continue
            if line.startswith("#"):
                ts = int(line[1:].strip())
                if first_ts is None:
                    first_ts = ts
                last_ts = ts
                continue
            if line.startswith("$"):
                continue  # $dumpvars/$dumpoff/$dumpon/$end/$finish
            # scalar change: <0|1|x|z><id> (vectors already rejected at $var)
            v, ident = line[0], line[1:]
            decl = decls.get(ident)
            if decl is None:
                continue
            state = {"0": 0, "1": 1}.get(v, 3 if v in "zZ" else 2)
            decl["events"].append((ts, state))

    if timescale is None:
        die("no $timescale directive")
    if first_ts is None:
        die("no timestamps")
    return root, timescale[0], timescale[1], first_ts, last_ts


def aggregate(events, t0, t1):
    """SAIF fields over [t0, t1]: (T0, T1, TX, TZ, TC) in timestamp units.

    Edges at the window bounds count toward TC (the VCD reader counts every
    dumped edge). A signal's first event only sets its initial level, matching
    the VCD reader's x-initialised last state.
    """
    dur = {0: 0.0, 1: 0.0, 2: 0.0, 3: 0.0}
    tc = 0
    state = None
    prev_t = t0
    for ts, new in events:
        if ts > t1:
            break
        if ts < t0:
            state = new
            continue
        if state is not None:
            dur[state] += ts - prev_t
            if state in (0, 1) and new in (0, 1) and state != new:
                tc += 1
        state = new
        prev_t = ts
    if state is not None:
        dur[state] += t1 - prev_t
    return dur[0], dur[1], dur[2], dur[3], tc


def fmt(v):
    return str(int(round(v)))


def emit_scope(node, name, indent, ghost_names):
    pad = "  " * indent
    out = [f"{pad}(INSTANCE {name}"]
    if node["signals"]:
        out.append(f"{pad}  (NET")
        for d in node["signals"]:
            t0, t1, tx, tz, tc = d["agg"]
            out.append(
                f'{pad}    ({d["name"]} (T0 {fmt(t0)}) (T1 {fmt(t1)})'
                f" (TX {fmt(tx)}) (TZ {fmt(tz)}) (TB 0) (TC {tc}) (IG 0))"
            )
        out.append(f"{pad}  )")
    for child_name, child in node["children"].items():
        out.extend(emit_scope(child, child_name, indent + 1, ghost_names))
    if name in ghost_names:
        out.append(f"{pad}  (INSTANCE ghost")
        out.append(f"{pad}    (NET")
        out.append(f"{pad}      (ghost_sig (T0 0) (T1 0) (TX 0) (TZ 0) (TB 0) (TC 0) (IG 0))")
        out.append(f"{pad}    )")
        out.append(f"{pad}  )")
    out.append(f"{pad})")
    return out


def main():
    ap = argparse.ArgumentParser(description="convert VCD to SAIF")
    ap.add_argument("vcd")
    ap.add_argument("saif")
    ap.add_argument(
        "--ghost-scope",
        default=None,
        help="instance path (e.g. tb/dut) under which to add an unmatched "
        "(INSTANCE ghost ...) scope for diagnostics testing",
    )
    ap.add_argument(
        "--window-end",
        type=int,
        default=None,
        metavar="TS",
        help="VCD timestamp overriding the aggregation window's last_ts "
        "(and DURATION); later events are ignored",
    )
    args = ap.parse_args()

    root, ts_num, ts_unit, first_ts, last_ts = parse_vcd(args.vcd)
    if args.window_end is not None:
        last_ts = args.window_end
    duration = last_ts - first_ts
    if duration <= 0:
        die(f"window [{first_ts}, {last_ts}] is not positive")

    def walk(node):
        for d in node["signals"]:
            d["agg"] = aggregate(d["events"], first_ts, last_ts)
        for child in node["children"].values():
            walk(child)

    walk(root)

    ghost_names = set()
    if args.ghost_scope:
        ghost_names.add(args.ghost_scope.split("/")[-1])

    lines = [
        "(SAIFILE",
        '  (SAIFVERSION "2.0")',
        '  (DIRECTION "backward")',
        "  (DESIGN)",
        '  (DATE "vcd2saif")',
        '  (VENDOR "grus-eda")',
        '  (PROGRAM_NAME "vcd2saif.py")',
        '  (VERSION "vcd2saif")',
        "  (DIVIDER / )",
        f"  (TIMESCALE {ts_num} {ts_unit})",
        f"  (DURATION {fmt(duration)})",
    ]
    for name, node in root["children"].items():
        lines.extend(emit_scope(node, name, 1, ghost_names))
    lines.append(")")

    with open(args.saif, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"wrote {args.saif}: window [{first_ts}, {last_ts}] = {fmt(duration)} timescale units")


if __name__ == "__main__":
    main()
