# F9: machine-independent tests and CI

Implements finding F9 of `docs/review-2026-11-11.md`. Surveyed state
(2026-11-14): `dirs_home()` is copied into 8 test modules and PDK path
chains are open-coded in about 14; `test_sky130_liberty_integration`
asserts a 10 ms warm-cache wall-clock bound (known flaky); the Liberty
cache's primary location is next to the `.lib` file, i.e. inside the PDK
installation; there is no CI. The org's self-hosted runner
(org Mac mini, PDK at `~/.ciel`, LibreLane image cached)
already serves other grus-systems repositories.

## Part A: test support, cache location, wall-clock (code)

1. **One `test_support` module** (`crates/grus-core/src/test_support.rs`,
   `#[cfg(test)]`), used by every test module:
   - `pdk_root()`: `$PDK_ROOT`, else `$HOME/.ciel`; panics with a clear
     message when neither is set (tests keep failing loudly on missing
     files — no silent skips).
   - `sky130_hd()`: `pdk_root()/sky130A/libs.ref/sky130_fd_sc_hd`, the
     base every current lookup builds on.
   - `design_dir(name)`: the repo's `tests/designs/<name>`, resolved
     from `CARGO_MANIFEST_DIR` so tests do not depend on the working
     directory.
   - `require(path)`: returns the path, panicking with the full path and
     what provides it when it does not exist.
   All local `dirs_home()` copies and open-coded PDK chains in test
   modules are replaced by these; no test's assertions change.
2. **Liberty cache relocation** (`crates/grus-core/src/readers/liberty.rs`,
   production code): the cache lives in one place — `$XDG_CACHE_HOME/
   grus-eda/`, else `$HOME/.cache/grus-eda/`, else
   `temp_dir()/grus-eda-cache/` — under the existing xxh3-based file
   name. The next-to-the-`.lib` primary location is removed entirely
   (neither read nor written): the cache never touches the PDK
   installation or any source directory again. The atomic tmp+rename
   write stays. Existing next-to-lib caches become orphans; the first
   load after the change reparses once.
3. **Wall-clock out of tests**: the timing assertion in
   `test_sky130_liberty_integration` is deleted (the value assertions
   stay). A std-only benchmark target
   `crates/grus-core/benches/liberty_cache.rs` (`harness = false`)
   prints cold-parse and warm-cache load times; it asserts nothing.

## Part B: CI (workflow, maintainer-verified)

`.github/workflows/ci.yml`:
- Triggers: push to `dev`, pull requests.
- `runs-on: [self-hosted, macOS, ARM64]` — the org runner on the Mac
  mini, which has the PDK and the cargo toolchain installed for the runner user.
- Steps: checkout; put `$HOME/.cargo/bin` and the mise shims on `PATH`;
  if the picorv32 generated inputs are missing, start Colima (the
  command in AGENTS.md Section 2) and run
  `tests/designs/picorv32/generate_ref.sh`; `cargo test --release`;
  `cargo clippy --all-targets -- -D warnings`. Timeout 60 minutes.
- The job is green only when the whole suite passes on the runner;
  the maintainer watches the first run to completion and records the
  result in `docs/results.md`.

## Acceptance

- Part A: full suite green on the mini with the PDK present; on this
  VM (no PDK) the same tests fail loudly on missing files as today,
  with messages naming the path; `grep` finds no `dirs_home` and no
  next-to-lib cache path construction; the liberty integration test
  contains no timing assertion; `cargo bench --bench liberty_cache`
  runs and prints both timings on the mini.
- Part B: a push to `dev` triggers the workflow and the run finishes
  green on the org runner; the run's log shows the picorv32 inputs
  being generated on a cold checkout.
- `docs/results.md` entry; version bump for Part A (production cache
  behaviour changes), none for Part B alone.

## Risks

- The runner is shared with other repositories' CI; queueing delays are
  acceptable, interference is not expected (separate `_work` checkouts).
- Colima inside CI: generation is needed only on a cold checkout (the
  files persist in the runner's `_work` between runs of the same job?
  No — actions/checkout cleans the workspace; generation runs every
  time unless cached. The step therefore runs `generate_ref.sh`
  whenever the files are missing, which on this runner is every run;
  measured cost goes into the results entry. If it is minutes-scale,
  a follow-up can cache the generated files with actions/cache.)
- `HOME` under the runner service is the user's home (the runner runs
  via launchd), so `~/.ciel` resolves; `test_support`
  panics clearly if that ever changes.

## Review resolutions (external review, 2026-11-14)

The full review text is not included in this repository. Verdict: adopt with
changes. Maintainer spot-checks: no code outside `readers/liberty.rs`
references the cache path (grep for `grus.cache`/`grus-cache` is empty);
there is no `crates/grus-core/tests/` directory, so all tests are unit
tests; every `sky130_fd_sc_*` reference in tests is `sky130_fd_sc_hd`;
every `.exists()` check in tests panics (no skip-tolerant test exists).
Adopted:

- **Unique temp file (S1)**: the cache write uses a per-writer unique
  temp name in the cache directory (process id + an atomic counter),
  then renames. (Load validation — magic, size, xxh3 — already bounds
  the damage of a torn entry to a reparse; this removes the race.)
- **Cache format version (S2)**: `CacheFile` gains a `version` field,
  checked on load; a mismatch is a cache miss.
- **design_dir (S1/S2)**: defined explicitly as
  `CARGO_MANIFEST_DIR/../../tests/designs/<name>` (the form the current
  tests already use), not a naive join.
- **cfg(test) scope (S2)**: with no integration tests, `#[cfg(test)]`
  covers every test module. The benchmark keeps its own small lookup,
  with a comment pointing at `test_support` as the canonical logic; no
  public feature is added for one bench.
- **Generic PDK helper (S2/S3)**: not needed — survey shows only
  `sky130_fd_sc_hd` is referenced. `sky130_hd()` only.
- **XDG edge (S3)**: an empty or relative `XDG_CACHE_HOME` is ignored
  (treated as unset).
- **Temp fallback (S2)**: the shared-temp fallback directory gains a
  `$USER` suffix when set; with unique temp names + rename this is the
  remaining mitigation, and the fallback only triggers when neither
  XDG nor HOME is set.
- **CI (Part B)**: PATH is extended via `$GITHUB_PATH`; the Colima
  step is guarded (`colima status` first, start only if not running);
  `runs-on` stays `[self-hosted, macOS, ARM64]` (the runner's default
  labels; the maintainer verifies via the API at first run). The repo
  is private and PRs are internal, so the fork-PR runner risk does not
  apply today; noted here so a future public flip reopens it.
