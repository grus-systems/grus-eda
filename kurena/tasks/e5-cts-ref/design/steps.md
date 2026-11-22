# e5-cts-ref — Steps

Two commits: generated reference set + docs record. No version bump
(reference-only; say so in the commit body per the acceptance
checklist).

## Step 0 — probe the report surface (uncommitted)

Inside the pinned image (Colima:
`mise exec docker-cli@latest colima@latest lima@latest -- colima start
--memory 12 --cpu 8`), run the step-A Tcl on `gcd.placed.def` in a
scratch dir and record:

- exact `report_cts`, `report_clock_skew -setup/-hold`,
  `worst_clock_skew`, `report_cts_config` output formats — which lines
  carry buffer/dummy/sink/depth/wirelength/latency/skew;
- whether `worst_clock_skew -setup` returns a value;
- whether the DEF census's output-net-fanout rule reproduces the 3
  dummy loads the log claims.

This is the same shape as the P-CTS probe; its transcript lives in
`runs/cts/` (gitignored) and the findings land in `docs/results.md`.

## Step 1 — script + artifacts (commit 1, `test:`)

1. `tests/designs/generate_cts_ref.sh` per `script.md`;
   `tests/designs/gcd/gcd.cts.sdc` + `picorv32.cts.sdc` per
   `sta-golden.md`.
2. `generate_cts_ref.sh gcd` — produces the three committed artifacts.
   Rerun once: `git diff` must be empty (byte-reproduction gate).
3. `generate_cts_ref.sh picorv32` — decision point:
   - lands cleanly → include all three artifacts (`.cts.def` and
     `.cts.sta.reference.json` gitignored >1 MB per convention;
     `.cts.reference.json` committed);
   - anything surprising (multi-clock, hours-long run, nondeterminism)
     → defer to ticket 2 and write the reason in `docs/results.md`.
     The script itself stays design-parameterized either way.
4. `.gitignore` lines for the two gitignored picorv32 files.
5. `tests/designs/README.md`: gcd artifact-table rows
   (`gcd.cts.sdc`/`gcd.cts.def`/`gcd.cts.reference.json`/
   `gcd.cts.sta.reference.json`/`generate_cts_ref.sh`) and the
   picorv32 paragraph addition (gitignored pair + SHA-256 prefixes).
6. Gates: `cargo test --workspace`, `cargo clippy --workspace
   --all-targets -- -D warnings` (unchanged code — must stay green).
7. Commit (`test:` + trailer), explicit file paths, no `git add -A`.

## Step 2 — comparison record (commit 2, `docs:`)

`docs/results.md` entry per `metrics.md`: script steps, measured vs
P-CTS table (confirmed or corrected with the cause named), artifact
sizes + SHA-256 prefixes, field-source notes, host/invocation/rerun
record. Commit (`docs:` + trailer).

## Risks / notes for the implementer

- **Report formats are the only unknown.** `report_cts`'s table shape
  is not recorded anywhere committed — step 0 exists because the
  parser must match the real output, not a memory of it. If a number
  the schema wants has no surface (e.g. total clock-net wirelength),
  compute it in census Tcl and set the `*_basis` field — record which.
- **Dummy-load identification.** The DEF-side rule (inserted clkbuf
  with output-net fanout ≤ 1 ITerm) is plausible, not proven —
  `report_cts`/the TritonCTS log is primary; the census is the
  cross-check. If they disagree, the log wins and the discrepancy is a
  results.md finding.
- **`repair_clock_nets` may insert extra buffers.** P-CTS showed none
  on gcd; picorv32's longer nets may differ — the census reports what
  is actually there, not the scoping's count.
- **`inserted_instances ≠ tree_buffers + dummy_loads` is a red flag** —
  repair buffers or unexpected cells; abort loudly, don't absorb.
- **Tcl-on-stdin rules** (AGENTS.md §7.3 + P-GR): `!` is history
  expansion — write `== 0`; `\` does not continue long lines — the
  `clock_tree_synthesis` call is one line in the committed script.
- **`estimate_parasitics` needs `set_wire_rc -layer met2` first**
  (EST-0018), in both steps A and C.
- **Container access**: the run needs Docker (Colima on macOS). The
  worktree seed supplies `picorv32.placed.def`; the script still fails
  loudly if it is absent.
- **No consumer test yet.** Nothing reads these goldens in this ticket
  — that is by design (the e2e/assertion ticket comes next), so the
  reproducibility gate and `git diff` cleanliness are the proof of
  correctness.
