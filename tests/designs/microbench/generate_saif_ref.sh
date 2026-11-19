#!/usr/bin/env bash
# Reference generation for the microbench SAIF power fixture. Converts mb.vcd to
# mb.saif via vcd2saif.py over the truncated window [10, 1990] (DURATION 1980,
# --window-end 1990: dropping clk's final #2000 fall makes its TC odd, so the
# golden exercises the odd-TC rise/fall split), then runs two OpenROAD sessions
# on identical design inputs: `read_vcd -scope tb/dut` + `report_power -digits 6`
# (checked against the committed mb.openroad.txt table) and `read_saif -scope
# tb/dut` + `report_power -digits 6` (filtered stdout committed as
# mb.saif.openroad.txt). Both tables are parsed into mb.saif.reference.json.
# A final section re-measures the SAIF field semantics (TC scaling, T0/T1 duty,
# IG/TX, PORT groups, ghost scopes); its output is recorded in docs/results.md,
# not committed.
# Runs inside ghcr.io/librelane/librelane:3.0.16.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
CONTAINER_IMAGE="ghcr.io/librelane/librelane:3.0.16"
PDK_HOST_ROOT="${PDK_ROOT:-${HOME}/.ciel}"

if [ ! -f /.dockerenv ] && [ ! -f /nix/var/nix/profiles/default/bin/openroad ]; then
    if docker info >/dev/null 2>&1; then
        docker_cmd="docker"
    else
        docker_cmd="mise exec docker-cli@latest colima@latest lima@latest -- docker"
    fi
    ${docker_cmd} run --rm \
        -u "$(id -u):$(id -g)" \
        -v "${REPO_ROOT}:/work" \
        -v "${PDK_HOST_ROOT}:/pdk" \
        -e PDK_ROOT=/pdk \
        -w "/work/tests/designs/microbench" \
        "${CONTAINER_IMAGE}" \
        bash /work/tests/designs/microbench/generate_saif_ref.sh
    exit 0
fi

cd /work/tests/designs/microbench

# 1. Convert the VCD window [10, 1990] to SAIF aggregates (DURATION 1980);
#    --window-end drops clk's last #2000 fall so its TC is odd, exercising
#    the rise/fall split. --ghost-scope adds an unmatched (INSTANCE ghost ...)
#    under dut to exercise drop diagnostics.
python3 vcd2saif.py --ghost-scope tb/dut --window-end 1990 mb.vcd mb.saif

cat > /tmp/mb_reads.tcl <<'TCL'
read_liberty /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lib/sky130_fd_sc_hd__tt_025C_1v80.lib
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/techlef/sky130_fd_sc_hd__nom.tlef
read_lef /pdk/sky130A/libs.ref/sky130_fd_sc_hd/lef/sky130_fd_sc_hd.lef
read_def /work/tests/designs/microbench/mb.def
read_sdc /work/tests/designs/microbench/mb.sdc
TCL

# 2. Session A: VCD-driven report_power (same inputs as committed mb.openroad.txt).
{
    cat /tmp/mb_reads.tcl
    echo "read_vcd -scope tb/dut /work/tests/designs/microbench/mb.vcd"
    echo "report_power -digits 6"
} > /tmp/session_vcd.tcl
openroad -no_init -no_splash -exit /tmp/session_vcd.tcl > /tmp/mb_vcd_power.txt

# 3. Session B: SAIF-driven report_power. The committed mb.saif.openroad.txt is
#    FILTERED stdout — grep -v drops INFO/WARNING/banner/blank lines so the file
#    keeps the annotation count, report table, and annotation split without
#    tool-version noise. It is not verbatim stdout.
{
    cat /tmp/mb_reads.tcl
    echo "read_saif -scope tb/dut /work/tests/designs/microbench/mb.saif"
    echo "report_power -digits 6"
    echo "report_activity_annotation"
} > /tmp/session_saif.tcl
openroad -no_init -no_splash -exit /tmp/session_saif.tcl \
    | grep -v '^\[INFO\|^\[WARNING\|^The NOWIREEXTENSIONATPIN\|^$' \
    > /work/tests/designs/microbench/mb.saif.openroad.txt

# 4. Parse both report tables into mb.saif.reference.json and cross-check the
#    session-A table against the committed mb.openroad.txt (print-rounded).
python3 - <<'PY'
import json
import re

ROW = re.compile(
    r"^\s*(Sequential|Combinational|Clock|Macro|Pad|Total)\s+"
    r"(\S+)\s+(\S+)\s+(\S+)\s+(\S+)"
)


def parse_table(path):
    groups = {}
    for line in open(path):
        m = ROW.match(line)
        if not m:
            continue
        name = "total" if m.group(1) == "Total" else m.group(1)
        groups[name] = {
            "internal_w": float(m.group(2)),
            "switching_w": float(m.group(3)),
            "leakage_w": float(m.group(4)),
            "total_w": float(m.group(5)),
        }
    return groups


power_vcd = parse_table("/tmp/mb_vcd_power.txt")
power_saif = parse_table("/work/tests/designs/microbench/mb.saif.openroad.txt")
ref_vcd = parse_table("/work/tests/designs/microbench/mb.openroad.txt")

mismatch = []
for name, vals in power_vcd.items():
    ref = ref_vcd.get(name)
    if ref is None:
        continue
    for k, v in vals.items():
        r = ref[k]
        # mb.openroad.txt prints 3 significant digits; agreement must be at
        # least at that granularity.
        if r != 0 and abs(v - r) / abs(r) > 5e-3:
            mismatch.append(f"{name}.{k}: session {v} vs committed {r}")
if mismatch:
    print("WARNING: session-A table diverges from committed mb.openroad.txt:")
    for m in mismatch:
        print(" ", m)
else:
    print("session-A table agrees with committed mb.openroad.txt (print rounding)")

out = {"power_vcd": power_vcd, "power_saif": power_saif}
with open("/work/tests/designs/microbench/mb.saif.reference.json", "w") as f:
    json.dump(out, f, indent=2)
    f.write("\n")
print("wrote mb.saif.reference.json")
PY

# 5. Field-semantics probes (recorded in docs/results.md; no committed output).
#    Every variant mutates fields via a regex anchored on the signal NAME —
#    positional replaces hit the first matching field values in file order,
#    which are shared across the net and pin views of clk/q/y.
python3 - <<'PY'
import re

text = open("/work/tests/designs/microbench/mb.saif").read()


def sig_fields(text, name):
    """Return the re.Match of the `(<name> (T0 ..) ..)` line."""
    m = re.search(rf"\({re.escape(name)} \(T0 (\d+)\) \(T1 (\d+)\) "
                  rf"\(TX (\d+)\) \(TZ (\d+)\) \(TB (\d+)\) \(TC (\d+)\) "
                  rf"\(IG (\d+)\)\)", text)
    assert m, f"signal {name} not found"
    return m


# Duty swap anchored on the signal name. `a` and `y` carry asymmetric duty;
# clk's is degenerate (T0 == T1) under the truncated window, so swapping it
# would measure nothing.
def duty_swap(text, name):
    m = sig_fields(text, name)
    new = m.group(0).replace(f"(T0 {m.group(1)}) (T1 {m.group(2)})",
                             f"(T0 {m.group(2)}) (T1 {m.group(1)})")
    return text[:m.start()] + new + text[m.end():]


def igtx(text, name):
    m = sig_fields(text, name)
    new = m.group(0).replace("(TX 0)", "(TX 40)").replace("(IG 0)", "(IG 40)")
    return text[:m.start()] + new + text[m.end():]


variants = {
    "tc10": re.sub(r"\(TC (\d+)\)", lambda m: f"(TC {int(m.group(1)) * 10})", text),
    "duty_a": duty_swap(text, "a"),
    "duty_y": duty_swap(text, "y"),
    "igtx": igtx(text, "y"),
    "port": text.replace(
        "(INSTANCE _d_\n        (NET", "(INSTANCE _d_\n        (PORT"
    ).replace("(INSTANCE _i_\n        (NET", "(INSTANCE _i_\n        (PORT"),
    "noghost": text.replace(
        """      (INSTANCE ghost
        (NET
          (ghost_sig (T0 0) (T1 0) (TX 0) (TZ 0) (TB 0) (TC 0) (IG 0))
        )
      )
""",
        "",
    ),
}
for name, body in variants.items():
    with open(f"/tmp/mb_{name}.saif", "w") as f:
        f.write(body)
print("wrote probe variants:", ", ".join(variants))
PY

for v in tc10 duty_a duty_y igtx port noghost; do
    echo "===== probe: $v ====="
    {
        cat /tmp/mb_reads.tcl
        echo "read_saif -scope tb/dut /tmp/mb_$v.saif"
        echo "report_power -digits 6"
        echo "report_activity_annotation"
    } > /tmp/probe_$v.tcl
    openroad -no_init -no_splash -exit /tmp/probe_$v.tcl
done
