# async-latch-delay — Implementation & verification steps

## Step 0 — Probe (acceptance #1)

1. Write the scenario SDCs per `fixture.md` §1 (they double as the
   committed probe inputs — writing them first means the evidence is
   reproducible from the start).
2. Start Colima if needed (`AGENTS.md` §2), run each scenario inside
   `ghcr.io/librelane/librelane:3.0.16` with the fixtures'
   `collect_endpoints_json` procs (ad-hoc heredoc on `openroad -no_init
   -no_splash`, or the extended `generate_ref.sh` once written). Record
   stderr — warnings/errors are part of the verdict.
3. For each target endpoint compare reported `arr`/`req`/`slack`
   against the baseline golden section; classify per `probe.md`
   (applied/ignored/errored) and, where applied, decompose `req` into
   the listed candidate rules — name the matching candidate or solve
   for the term it actually used.
4. Run the ad-hoc negative controls (`-from [get_ports d_in]`, `-from
   [get_pins u_ff0/CLK]` on the async pin).

## Step 1 — Golden regen + results entry (committable unit)

1. Extend both `generate_ref.sh` scripts per `fixture.md` §2 (scenario
   list → new top-level keys; `{"error": …}` sections for errored
   scenarios).
2. Run each committed script as written from the repo root; `git diff`
   the reference JSONs — all-new sections only, baseline `ideal` /
   `propagated` byte-identical (AGENTS.md §7.1).
3. Write the dated `docs/results.md` entry: verdict table per
   (kind, side), numeric decomposition of any applied req rule,
   OpenSTA warnings/errors verbatim, negative-control outputs, and
   exactly what ran.
4. Commit: `test:`/`docs:`-style — scenario SDCs + `generate_ref.sh` +
   `*.reference.json` + the results entry (AGENTS.md §5, trailer).

## Step 2 — Implement applied kinds (skip if none applied)

Per `api.md`: widen `delay_supported`, add any measured-rule branch on
the latch path, update the skip-counter doc comments. Add the
`fixture.md` §3 test functions at `Tol::STRICT` — all of them,
including ignored-verdict scenarios (they pin the evidence).

## Step 3 — Verify

- `cargo test -p grus-core sta_edge` then `cargo test --workspace` —
  STRICT throughout, never loosened (AGENTS.md §7.5).
- Regression watch: existing goldens (`ideal`/`propagated` of both
  fixtures, all `sta_exceptions` scenarios) byte-identical — the change
  is additive per endpoint kind. `unsupported_path_delay_endpoints`
  must drop by exactly the applied kinds' counts on the new scenarios.
- Fail-proof per `api.md` (revert gate / wrong req candidate → red;
  ignored verdicts → scenario byte-identical to baseline). Record the
  red run.
- `cargo clippy --workspace --all-targets -- -D warnings`.

## Step 4 — Docs, version, commits

1. `docs/path-delays.md`: §1 measured-rule paragraph per applied kind;
   §3 scope lists; §5 item 1 split — strike "async-reset endpoint
   probe" and "latch-borrow delay fixture" with the measured outcomes,
   keep "literal two-clock-domain `set_min_delay` anchoring" open with
   the multi-clock blocker restated.
2. `docs/results.md`: complete the entry (our vs golden numbers,
   fail-proof result, what ran).
3. `docs/timing-exceptions.md` only if the skip surface changed
   (expected: no).
4. `Cargo.toml` `0.1.94` → `0.1.95` + `Cargo.lock` in the same commit
   as the functional change (or the docs commit if nothing applied).
5. Commits per AGENTS.md §5: (a) fixture + probe results; (b)
   implementation + tests + docs + version — or a single docs commit
   if every verdict was ignored/errored. Conventional Commits, agent
   trailer.

## Risk checklist

- **OpenSTA applies with a rule outside the candidate list** → solve
  the reported `req` for its terms before implementing (AGENTS.md
  §7.6); if it resists decomposition, record the observation and keep
  the kind counted rather than approximate.
- **`delay_allregs` verdict differs from `-to [get_pins …]`** →
  selector-dependent coverage is a real possibility (launch-object
  semantics were selector-dependent on exceptions); record per-
  selector verdicts, implement only what was measured.
- **Async verdict differs between `async_reset` (FF async pin) and
  `delay_allregs`'s latch `RESET_B` pins** → same endpoint kind by our
  classification but possibly different in OpenSTA; record both, cover
  the narrower measured set.
- **A scenario OpenSTA errors on** → `{"error": …}` golden section per
  `fixture.md` §2; evidence in `results.md`.
- **Container/PDK**: Colima running, PDK at `~/.ciel` (`PDK_ROOT`
  override supported).
