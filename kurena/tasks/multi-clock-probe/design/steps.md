# Implementation & verification steps

## Step 1 — Fixture

1. Create `tests/designs/sta_clocks/` per `fixture.md`:
   `two_clock.def`, the `t2_*.sdc` files, `probe.sh`.
2. `probe.sh` mirrors `tests/designs/sta_exceptions/generate_ref.sh`:
   container wrapper (docker via Colima, `mise exec ... -- docker`
   fallback), then per-scenario `openroad -no_init -no_splash` runs
   inside `ghcr.io/librelane/librelane:3.0.16` — one producing the
   endpoint JSON, one producing `probe_<name>.txt` (full-format
   `report_checks` + `-unconstrained`). Consolidate into
   `two_clock.probe.json`; delete `ref_*.json` temporaries.
3. `tests/designs/README.md` catalog entry (hand-written fixture,
   authored in-repo, `sky130_fd_sc_hd` cells only).

Commit 1: `test:` fixture + probe script + README entry.

## Step 2 — Our-side break probe

Run the `breakpoints.md` §3 table on HEAD and record actuals:

1. `cargo run --release -p grus-eda -- sta --lib <lib> --tlef <tlef> --lef <lef> --def tests/designs/sta_clocks/two_clock.def --sdc tests/designs/sta_clocks/t2_int.sdc` — record the exact stderr message and exit code (expected: `a second design clock is not supported`, from `sdc.rs:865`).
2. The `parse_sdc`-level rows can run as `cargo test`-style probes or
   a temporary unit check — record each message verbatim.
3. Scratch experiment (optional but recommended): comment out the
   `create_clock` bail, run the same command, record what domain A's
   endpoints do (absent/unreached/panic/wrong numbers) and whether
   `clk2` shows up as a data-launch source. **Revert the edit; it is
   never committed.** Record precisely what was run (AGENTS.md §7
   rule 9).
4. Optional regression test committing the documented break: a
   `sdc.rs` test asserting `parse_sdc` on a two-design-clock SDC
   fails with `a second design clock is not supported` (plus the
   `set_clock_latency takes one clock` and virtual-period bails if
   not already covered — check `sdc.rs`'s test module first). This is
   the only code-side change contemplated.

## Step 3 — OpenSTA probe

1. Start Colima (`mise exec docker-cli@latest colima@latest lima@latest -- colima start --memory 12 --cpu 8`) if needed.
2. `bash tests/designs/sta_clocks/probe.sh` from the repo root.
3. Inspect `two_clock.probe.json` + `probe_*.txt` against `probe.md`'s
   hypotheses *before* writing results:
   - `t2_int`/`t2_frac` edge times → Q1/Q2 pairing rule.
   - `t2_unc`/`t2_uncx` req decompositions → Q3.
   - `t2_lat` per-endpoint latency attribution → Q4.
   - `t2_md`/`t2_mdo` required-time anchors → Q5 (the §5 question).
   - `t2_prop*`/`t2_grp`/`t2_mcp` → Q6.
4. Re-run `probe.sh`; `git diff` the artifacts — they must be
   byte-identical (AGENTS.md §7 rule 1).

## Step 4 — `docs/results.md`

Dated entry `### <date> — multi-clock SDC probe (sta_clocks)`:

- commands run (our binary probe rows, scratch run if done,
  `probe.sh`);
- break-point table (command → observed behaviour, message verbatim);
- measured-rules table per Q1–Q6 (scenario → numbers → rule);
- which hypothesis won where several were listed;
- explicit note of anything *not* run.

Commit 2: `test:`/`docs:` probe artifacts + results entry. (Commit
the generated `two_clock.probe.json` and `probe_*.txt` with it —
small, and they are the evidence.)

## Step 5 — Sequenced plan → `docs/multi-clock.md` + `docs/roadmap.md`

Unless the probe shows a ticket-sized gap (it should not — the audit
is structural), write `docs/multi-clock.md` with two sections:

1. **Measured rules** — the OpenSTA semantics from `docs/results.md`
   restated as the target spec (pairing rule, horizon, per-clock
   uncertainty/latency/transition, propagation per clock, delay-check
   anchors, clock groups if measured).
2. **Ticket sequence** — one commit each, gated, in this order
   (adjust to what the probe found):
   - `mc-model`: `Sdc` keeps a clock table — per-domain
     name/port/period/latencies/uncertainty/transition/propagated;
     `ClockRef` becomes a domain index; `create_clock` accepts N
     design clocks; virtual clocks join the table. Eval still treats
     unseeded domains conservatively (loud skip, not silent wrongness).
     Parse + model tests.
   - `mc-eval`: domain tagging — per-port clock-net seeding
     (propagated + `ideal_clock_parity`), domain tags on launch seeds
     and clock pins, `clk2`-style clock ports excluded from data
     seeding; same-domain pairing with distinct periods; per-domain
     period in MCP/borrow windows.
   - `mc-xdomain`: cross-domain launch/capture edge search per the
     measured horizon rule; inter-clock uncertainty (`-from`/`-to`).
   - `mc-selectors`: clock-object selectors in exceptions
     (`-from [get_clocks cka]`), `set_clock_latency` multi-name,
     `set_clock_groups` if the probe warrants it.
   - `mc-golden`: `sta_clocks` becomes `two_clock.reference.json` +
     `test_sta_clocks_match_opensta`, closing `docs/path-delays.md` §5
     item 1.

`docs/roadmap.md`: one pointer line under the path-delay "Remaining"
item naming `docs/multi-clock.md`.

## Step 6 — `docs/path-delays.md` §5

Update item 1: record the measured capture-anchor answer (Q5) and
that the fixture exists as a committed probe pending the `mc-*`
sequence; keep it in "Still open" unless implementation landed.

## Step 7 — Version + full verify

- `Cargo.toml` `[workspace.package] version` patch bump in the same
  commit as the docs land (`Cargo.lock` follows via build). No
  functional change, but the spec calls for a patch bump when docs
  change.
- `cargo test --workspace` — must be green with **no** changes to
  existing goldens (any drift means the probe run touched committed
  numbers — stop and investigate).
- `cargo clippy --workspace --all-targets -- -D warnings`.
- The heavy `sta-regression` verify is not required: no timing-code
  change is expected. Run it anyway if the optional `sdc.rs` test or
  any production edit lands.

## Suggested commit split

1. `test:` `tests/designs/sta_clocks/` fixture + `probe.sh` + README entry
2. `test:` probe artifacts (`two_clock.probe.json`, `probe_*.txt`) + optional `sdc.rs` bail test
3. `docs:` results.md entry + `docs/multi-clock.md` + roadmap pointer + path-delays §5 + version bump

## Verify commands (workflow.toml)

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

## Risk checklist

- **Spec/code mismatch** — spec says "silent overwrite"; HEAD bails.
  Record the actual behaviour; do not write "overwrite" into
  results.md unless the scratch run is what produced it (label it).
- **Container/PDK** — docker via Colima; PDK from `~/.ciel`
  (`PDK_ROOT` override). `probe.sh` must fail loudly if the image or
  PDK is missing.
- **`report_checks -format full` flags** — exact flag spellings are
  resolved inside the image (`report_checks -help`); the `-unconstrained`
  listing is the fallback for "which paths are timed" evidence.
- **Artifact drift on re-run** — expected byte-identical; investigate
  any diff before committing.
- **Hand-derived numbers** — every number in results.md comes from
  the committed probe artifacts, not arithmetic on them; where a
  decomposition is arithmetic it is labelled as such (e.g.
  "req − arr = 0.20 ⇒ capture-side uncertainty").
- **Scope creep** — no production-code changes except the optional
  bail regression test; the scratch edit is reverted before any
  commit; a surprise "small gap" still needs golden coverage, not
  just a fixed parse.
