# mc-eval — Overview

Second ticket of the multi-clock sequence (`docs/multi-clock.md` §3).
`mc-model` landed the `Sdc::clocks` row table and parked eval behind the
`design_clock_count() > 1` bail in `run_sta_detail`. This ticket makes eval
domain-aware: every clock net is tagged with the `create_clock` row that
sources it, every launch carries a domain tag, and same-domain
launch/capture pairs time on their own domain's lattice (period, latency,
uncertainty, transition, propagated flag — all row reads, never the
domain-0 accessors). Cross-domain pairs are never folded onto a lattice:
they defer with a named, counted diagnostic (`mc-xdomain` owns the LCM
edge-pair search, `docs/multi-clock.md` §1.2).

## In scope

1. **Per-port clock-net seeding.** `propagate_clock` seeds every design
   (port-bearing) clock row's own port — each with that row's
   `source_latency_ns`, `0.5 * period_ns` half period, and per-port
   `set_input_transition` slew — and returns `(clk_state, parity)` as
   today. `ideal_clock_parity` becomes a `(parity, domain)` walk that
   seeds every design row's port and returns both arrays.
   `build_graph`'s clock-net marking seeds from every design row's port
   net, not just `clock_port()`'s.
2. **Domain tags.** A `clock_domain: Vec<u8>` map (row index into
   `Sdc::clocks`, `u8::MAX` = untagged) rides the ideal parity walk.
   `PinTimingState` gains `dom_max`/`dom_min` masks (bit = clock-row
   index) with the same winner-replace/tie-union discipline as
   `par_*`/`crpr_*`. `LaunchSeed`/`PortSeed` carry a `dom` tag; non-first
   clock ports stay excluded from data-input seeding (`is_clock_port`,
   already row-wise since `mc-model`).
3. **Same-domain pairing, distinct periods.** In `collect_endpoints`
   (and the partitioned skip pass) a check's capture domain is
   `clock_domain[check.clk]`; an arrival extreme is evaluated iff its
   domain mask is exactly the capture bit — anything else is a
   cross-domain contribution and the **whole endpoint** defers:
   `SkipReason::CrossDomain`, counted in a new
   `SkipReport::cross_domain_endpoints`, listed in `skipped_endpoints`
   with `related` naming the launch/capture clocks. Required times use
   the capture row's period/uncertainty; the launch side keeps its own
   domain's latency inside arrivals (measured §1.2/§1.4).
4. **Per-domain rows in anchors.** `set_max_delay`/`set_min_delay`
   required times read the capture row (`§1.5` anchor: capture domain's
   edge-0 arrival + capture row's latency/uncertainty); MCP multipliers
   read the capture row's period (`§1.6`); latch-borrow windows read the
   latch enable's own domain row; output-port checks read the
   `set_output_delay -clock` anchor row (virtual rows included); pulse
   widths read the check pin's domain row period.
5. **The gate moves.** `run_sta_detail` drops the
   `>1 design clock` bail. Multi-clock SDCs evaluate; the residual bail
   is a named `prepare_sta` bail when the clock table exceeds the
   domain-mask width (> 8 rows). The single-clock-SDC-on-multi-clock-DEF
   fold is fixed structurally: a clock consumer pin whose domain tag is
   `u8::MAX` is never seeded, so its checks surface as `NoClock` instead
   of silently taking domain 0's lattice.
6. **Golden.** `tests/designs/sta_clocks` becomes a regression target:
   a test compares our per-endpoint numbers against the committed
   `two_clock.probe.json` (and a new `two_clock_latch.probe.json`) at
   `Tol::STRICT` for the evaluable endpoint set, and pins the deferred
   cross-domain set. Three new scenarios enter the probe (`t2_mcps`,
   `t2_mds`, `t2_latch`) so MCP, delay-check anchors, and the borrow
   window are discriminating *same-domain* measurements — the committed
   fixture's only MCP/delay-check lines cover cross-domain endpoints,
   which defer before those code paths run.
7. **Recording.** `docs/results.md` entry: per-scenario same-domain
   matches, deferral counts verbatim (CLI `--json` skip summary),
   still-bailing forms (`t2_uncx`, `t2_grp`) verbatim, the fail-proof
   mutation run. `docs/multi-clock.md`: status line + §3 tick, §2
   rewrite (still-open: xdomain pairing, `-waveform`,
   `set_clock_groups`, inter-clock `-from`/`-to` uncertainty).
   `docs/roadmap.md` sequence note; `tests/designs/README.md` fixture
   entry updated (now a regression target + new fixture files).

## Out of scope (later tickets)

- `mc-xdomain`: cross-domain launch/capture edge-pair search (LCM
  window, min `capture − launch`), inter-clock
  `set_clock_uncertainty -from/-to` (still a parse bail, `t2_uncx`
  untestable until then), cross-domain hold-pair selection beyond
  coincident edges.
- `mc-selectors`: `get_clocks` object selectors in `-from`/`-to`,
  `set_clock_groups` (still `unsupported command`, `t2_grp` untestable),
  `create_clock -waveform`, `set_clock_latency -early`/`-late`,
  generated clocks.
- `mc-golden`: promote to `two_clock.reference.json` +
  `test_sta_clocks_match_opensta`, closing `docs/path-delays.md` §5
  item 1. The delay-check capture anchor is exercised same-domain by
  `t2_mds`; the literal cross-domain delay-check endpoint stays
  deferred until `mc-xdomain`.
- Real pairing for virtual-clock I/O anchors whose period matches no
  design row (see api.md §2 — they defer loudly, not fold).

## Golden-identity invariants

On a single-design-clock SDC every domain read resolves to the design
row, so all existing goldens must be byte-identical. Specifically:

- `clock_domain` has exactly one valid tag (0); every `dom` mask is
  `{0}`; `same_domain` is always true; every `cap` read is the design
  row — identical arithmetic to today on every site.
- `propagate_clock` seeds the same single port with the same values;
  `ideal_clock_parity` returns the same parity array (domain map is
  additive). `clock_parity` merge prefers the propagated result where
  it reached — identical wherever consumers read it.
- `cell_driven_clock_nets` moves from `build_graph` into `prepare_sta`
  and keeps the same totals (see api.md §5).
- `merge()` gains two arguments; all call sites updated — no semantic
  change on single-domain inputs.
- ibex's `vclk_core_clock` I/O anchors fold onto `core_clock`'s row
  (equal period): input/output clock latencies, periods, and
  uncertainties read the same numbers — the fold is arithmetically
  identical, not a behaviour change.

Two intended, non-golden-visible changes (record in `docs/results.md`):

- A clock pin no declared domain reaches now stays unseeded → its
  checks skip `NoClock` instead of evaluating on domain 0's lattice
  (the recorded hazard). No committed golden has such a pin — a
  regression here turns `cargo test --workspace` red and is the
  fail-proof check that the fold is gone.
- `sta` on a >1-design-clock SDC produces a report instead of the
  `multi-clock analysis is not implemented yet` error.

## Risks

- `merge()`/`LaunchSeed` plumbing touches the hot eval loop; keep the
  changes mechanical (extra mask args, `src.dom_*` propagation).
- `u8` domain masks cap the clock table at 8 rows — bail loudly in
  `prepare_sta` beyond that rather than truncate.
- A reached extreme with an empty `dom` mask (only unmatched-virtual or
  untagged launches) defers as cross-domain — conservative, never
  silently evaluated.
- Tied arrivals from different domains on one extreme: the mask unions
  and the endpoint defers — conservative and honest (pairing identity
  per tying launch is not recoverable from merged masks).
- `two_clock.probe.json` regenerates with two added scenarios — the
  pre-existing blocks must stay byte-identical; `git diff` after
  re-running `probe.sh` (AGENTS.md §7 rule 1).

## Files

- `crates/grus-core/src/sta/analysis.rs`
- `crates/grus-core/src/sta/sdc.rs`
- `tests/designs/sta_clocks/probe.sh`
- `tests/designs/sta_clocks/t2_mcps.sdc`
- `tests/designs/sta_clocks/t2_mds.sdc`
- `tests/designs/sta_clocks/t2_latch.sdc`
- `tests/designs/sta_clocks/two_clock_latch.def`
- `tests/designs/sta_clocks/two_clock.probe.json`
- `tests/designs/sta_clocks/two_clock_latch.probe.json`
- `tests/designs/sta_clocks/probe_t2_mcps.txt`
- `tests/designs/sta_clocks/probe_t2_mds.txt`
- `tests/designs/sta_clocks/probe_t2_latch.txt`
- `tests/designs/README.md`
- `docs/multi-clock.md`
- `docs/results.md`
- `docs/roadmap.md`
- `Cargo.toml`
- `Cargo.lock`
