# async-latch-delay — Implementation & verification steps

Fresh run: nothing from the aborted pass survives except these design docs
— no scenario SDCs, no `analysis.rs` changes, no golden sections. Order is
evidence first (goal.md "Measure first"), implementation second.

## Step 0 — Fixtures & committed measurement

1. Create the 11 scenario SDCs per `fixture.md` §1 (each = base SDC
   verbatim + one delay command; verify with `diff` against the base).
2. Extend both `generate_ref.sh` per `fixture.md` §2 — scenario list +
   merge keys, in each script's own emit style (`latch_borrows` vs
   `hold_violations`).
3. Start Colima if needed (`AGENTS.md` §2); run each committed script
   **as written** from the repo root. `git diff` the reference JSONs:
   new `delay_*` sections only; `ideal`/`propagated` byte-identical —
   if a baseline drifts, stop: the committed script no longer reproduces
   its own golden (AGENTS.md §7.1), a separate bug.
4. Check each new section against `probe.md`'s recorded numbers —
   agreement upgrades the record to evidence; divergence means
   re-probe the pin in the container and re-derive the rule from the
   tool's actual output (AGENTS.md §7.5/§7.6).
5. In the same container session, run the residual ad-hoc probes
   (`probe.md` §Residual: `-from [get_ports d_in]`, `-from [get_pins
   u_ff0/CLK]`) and capture outputs verbatim for `results.md`.
6. Commit unit: `test:` — scenario SDCs + `generate_ref.sh` +
   `*.reference.json`.

## Step 1 — Implement per `api.md`

Only for (kind, side) pairs the goldens confirm as applied. `dly`
extremes, `disabled_borrow`, `edge0_pol`, widened `delay_supported`,
latch clamp + latch-min branches. For any pair that came back
ignored/errored instead: keep the counted skip, narrow the gate, record
the evidence — a `docs:`-level outcome, not more code.

## Step 2 — Tests

1. Add the 11 `compare_sta_edge` tests (`fixture.md` §3) at STRICT.
2. `cargo test -p grus-core sta_edge` — every field of every endpoint
   row. On a mismatch: decompose OpenSTA's actual `req`/`arr`
   (`report_checks`/field dump in the container), fix the code — never
   fit the golden. Scrutiny list: `api.md` §5 (`dly` merge semantics,
   `disabled_borrow` breadth, `edge0_pol`).
3. `cargo test --workspace` — `sta_exceptions` has no latches, so its
   `md_*`/`pm*`/`m2_*`/`pmp_*`/`pml_*`/`pmd_*` goldens must stay
   byte-identical; any diff is a bug in the `dly` plumbing, not a
   tolerance. Fail loudly on missing fixture inputs (AGENTS.md §2 — no
   silent skips).
4. `cargo clippy --workspace --all-targets -- -D warnings`.
5. Fail-proof (`api.md` §6): revert the gate → red on async/latch rows;
   `w.arr` for `w.dly` → red on `u_lp/D`. Restore after each; record
   both red runs in `results.md`.

## Step 3 — `docs/results.md` entry

One dated entry (`probe.md` §Recording format): reference command = the
committed `generate_ref.sh` scripts; verdict table per (kind, side);
numeric decomposition of each applied rule at its target pin (solve the
reported `req` into `capture/lat0/unc/t_check/N` terms, AGENTS.md §7.6);
negative-control outputs; OpenSTA warnings verbatim; ours-vs-golden
STRICT comparison; fail-proof record; explicit "not run" list (e.g.
`async_fall`-style negative-parity delay coverage, two-clock domain).

## Step 4 — Docs, version, commits

1. `docs/path-delays.md` §1: add the measured req rules — async
   (recovery/removal arcs play late/early) and latch-D (clamp + `dly`
   re-basing + `disabled_borrow` + `lat0` anchor). §3: move
   async-recovery/removal and latch-D from the diagnoses/out-of-scope
   lists into "v1 supports"; the
   `unsupported_path_delay_endpoints` bullet shrinks to `ClockGate`.
   §5 item 1: strike the async-reset and latch-borrow halves with the
   measured outcomes; keep "literal two-clock-domain `set_min_delay`
   anchoring" open with the multi-clock blocker restated.
2. `docs/timing-exceptions.md` only if the skip surface changed
   (expected: no).
3. `Cargo.toml` `0.1.94` → `0.1.95` + `Cargo.lock` in the same commit as
   the functional change.
4. Commits (AGENTS.md §5, Conventional Commits + agent trailer):
   (a) `test:` fixtures + goldens; (b) `feat:` analysis.rs + tests +
   docs + results entry + version bump. If any (kind, side) came back
   ignored/errored instead, that part is a `docs:`-level outcome with
   the counted skip kept.

## Risk checklist

- **Golden contradicts the record** → re-probe the exact pin inside the
  container; fix code or narrow `delay_supported` to the measured
  subset; record either way (`probe.md` §"If regen contradicts").
- **`disabled_borrow` over-silences** → `delay_allregs`/`delay_max`
  goldens show it on downstream `arr`; if OpenSTA silences only for the
  covered endpoint, re-scope to per-endpoint.
- **`dly` merge model wrong** (independent extreme vs
  of-the-arr-winner) → shows on multi-source delay endpoints; measure
  with a `-from`-restricted variant before refactoring.
- **Baseline sections drift on regen** → stop; that means the committed
  script no longer reproduces `ideal`/`propagated` — a separate bug,
  per AGENTS.md §7.1.
- **Container/PDK**: Colima running, PDK at `~/.ciel` (`PDK_ROOT`
  override supported).
