# steps — order, commits, verification (revision 2)

The routed-output question is already settled (byte-identical on both
designs, digest pinned). Order here is about producing the record
honestly: extract retained numbers first, land the census repairs,
then the cheap gcd re-runs, then the docs commit.

## Step 1 — extract the retained data (no code change)

Transcribe every `docs.md` §2 row from the retained logs:

- `/tmp/gcd.mem.base.{1,2,3}.log`, `/tmp/gcd.mem.inst.log`,
  `/tmp/gcd.mem.new.log`
- `/tmp/picorv32.mem.{base,inst,new}.log`

Any log that has been rotated away → re-run that leg; never
synthesize a number (E0 rules 1–2). The §2 verified figures are the
spot-check: a transcription that disagrees with them means the wrong
log was read.

## Commit 1 — `fix:` census conformance repairs

`code-fixes.md` §1–3 in one commit (all three are review repairs to
the same `GRUS_DRT_MEM` instrumentation), workspace 0.15.0 → 0.15.1.

Verify (workflow.toml pair):

- `cargo test --workspace --profile verify` — all green; the census
  change cannot move `test_maze_census`'s digest or the DEF golden.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo build --release -p grus-eda`.

## Step 2 — fresh gcd runs (~30 s total)

Same invocation as `measurement.md` §2, `target/release/grus-eda`:

1. One `GRUS_DRT_MEM=1` run → record the `iter=8` `attempt_hi` (now a
   real number, not 0) and confirm the `outcomes=0` note; emitted DEF
   sha256 must equal
   `1422d02765f6aba99e25f1792436607c788ca31397c0bf843340bd65fb2f9482`.
2. Two more **ungated** reps → with `/tmp/gcd.mem.new.log`'s
   82,575,360 B that is ≥3 after-reps; median peak RSS and median
   `route_ms` are the tabled after figures (`docs.md` §3–§4).

No picorv32 re-run — the retained three-leg logs carry every needed
row, and `docs.md` §6 names why the serial-arm figure there is a
bound, not a measurement.

## Commit 2 — `docs:` the corrected record

Rewrite the `### 2026-11-27` entry per `docs.md` §1–§7, the
`docs/e6-drt.md` clause, the `docs/status.md` version token. Land
with the fresh run numbers quoted in the checks-run section.
