# e5-insert-api — Steps

Two commits: API + tests, then the docs record. Version bump rides with
the functional commit (AGENTS.md §5).

## Commit 1 — `feat:` mutation API + tests

Order inside `crates/grus-core/src/design_db.rs`:

1. `insert_cell` + `create_net` (+ T0's `create_net` cases), each with
   the rustdoc invariants from `api.md`.
2. `connect_pin`/`disconnect_pin` + the private offsets rebuild —
   implement T0 first and get it green before touching `insert_buffer_*`;
   this is where the I3/I4 bugs would live.
3. `insert_buffer_on_net` → T1.
4. T2 (legalize → `write_def_full` → `parse_def`) and T3 (byte-identity)
   on the gcd fixture.
5. `Cargo.toml`: `[workspace.package] version` `0.3.13 → 0.4.0` (minor —
   acceptance says so; name the §5-patch-vs-minor discrepancy in the
   commit body).

Gates before commit: `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`.

Message shape: `feat: Add DesignDb netlist-mutation API for CTS` —
bullets naming the five methods, the invariants, the O(P+N)-per-call
CSR policy, and the test list; trailer per AGENTS.md §5.

## Commit 2 — `docs:` results + status

1. Run the T4 corruptions, capture both red outputs.
2. `docs/results.md`: dated entry — what ran (`cargo test --workspace`
   count, the four test names), T4 red-run record, the guide-parasitics
   interpretation from `tests.md` T3 §4, the DEF-source-only coverage
   caveat, and the minor-vs-patch version note.
3. `docs/status.md`: new `**E5**` row after E4 — designs `gcd`;
   tolerance "exact structural equality (counts, membership,
   driver-first, byte-identical untouched lines)"; status "API landed —
   in progress (CTS tree next)"; known gap = no batch move API,
   DEF-source-only test coverage.

Same gates; commit `docs:` + trailer.

## Risks / notes for the implementer

- **Offset ordering pitfall**: in `connect_pin`, compute the insert
  index from the *post-removal* offsets, not the pre-call table — a
  removal in a lower-numbered net shifts every later segment (`api.md`).
- **`pin_io_rects` is a per-pin `Vec` column** — easy to drop from
  `insert_cell`; I1 (SoA lengths) catches it only if the checker runs.
- **`is_power` is load-bearing** in buffer-pin classification: `clkbuf_*`
  power pins are `DIRECTION INOUT`, not `Internal`, so a dir-only filter
  misfires.
- **`db.strings` is `tech_db.strings.clone()`** in all three readers —
  `mpin.name` pushes stay valid; document, don't re-intern.
- **Legalizer mutates all movable cells** — seed the inserted cell near
  its net's driver, and keep T3's byte-identity check on the
  pre-legalization emit. `legalize_placement` takes `gp_*` for *all*
  `num_cells` as `f32` µm.
- **Escapes**: `escape_def_name` lives in `readers::def` (`pub fn`) —
  import it for `create_net`'s `net_def_spellings` push; test names and
  net names containing `[`/`]`/`\\` already round-trip through it.
- **`+ SOURCE TIMING`**: OpenROAD marks CTS cells with it; our parser
  skips `SOURCE` and the writer doesn't emit it — unmodeled metadata,
  noted in `results.md`, not a gap for this ticket.
- **No silent skips**: T1–T3 use `require()`/explicit `is_file` panics
  for `gcd.placed.def`, tlef/lef, and the Liberty — the `def.rs` test
  pattern, verbatim.
- **Do not touch `readers/def.rs`**: verified `write_def_full` emits
  everything generically. If implementation finds otherwise, stop and
  re-plan rather than patch the writer blind.
