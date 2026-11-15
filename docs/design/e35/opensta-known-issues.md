# E3.5: known upstream reports for three OpenSTA behaviours

Date of search: 2026-11-15. Scope: public issues, PRs, discussions, papers only.
Licence policy (`AGENTS.md` Section 3b): OpenSTA source was **not** read. Two web searches returned
snippets of a third-party mirror of `DmpCeff.cc`; they were not used as evidence anywhere below.
Searches used the GitHub search API (via WebFetch) and WebSearch. Nothing was run in the pinned image.

Repos searched: `parallaxsw/OpenSTA` (the maintainer's repo, issues + PRs), `The-OpenROAD-Project/OpenSTA`
(PRs; its issue tracker is not where bugs are filed), `The-OpenROAD-Project/OpenROAD`.
Keywords: Ceff, dmp, report_dcalc, slew+waveform, elmore+wire+delay, input transition+port,
shielding/extrapolat/large. The three searches for Ceff/dmp lists returned the same ~11 issues/PRs,
so coverage of the DMP topic looks complete for those keywords. GitHub *discussions* were not
searchable through the API used; OpenROAD discussion pages were only seen as search hits, not read.

## Summary

| # | Behaviour | Conclusion |
|---|---|---|
| 1 | Ceff 54% of total at 9.1 ns input slew, 2.23 pF load (jpeg `_43767_`) | **No prior report found; filed as [parallaxsw/OpenSTA#582](https://github.com/parallaxsw/OpenSTA/issues/582)** (`tests/designs/repro_extreme_ceff/`, `7bfa26b`) |
| 2 | Driver waveform slew > table slew | **Spec (documented by the maintainer)**; the size of the gap is not explained |
| 3 | Port-driven net wire delay = ln2 x Elmore (ideal) / 0.40-0.45 x Elmore (with `set_input_transition`) | **No report found** |

## 1. Ceff at extreme load and long input slew

| Item | State | Relevance |
|---|---|---|
| [parallaxsw/OpenSTA#405](https://github.com/parallaxsw/OpenSTA/issues/405) "DMP divergence with 0 input slew" (2026-03-23) | closed 2026-03-25 | DMP Newton-Raphson gave NaN/garbage in `repair_timing`. Thread concluded it was a **multithreading race** on a shared DMP object (commit named in the thread: "GraphDelayCalc::findInputArcDelay thread issue"). Not a physics/accuracy issue. A participant notes negative delays can be legitimate NLDM extrapolation at large input slew. |
| [The-OpenROAD-Project/OpenSTA#468](https://github.com/The-OpenROAD-Project/OpenSTA/pull/468) "add standalone DMP accuracy and invariant unit tests" (2026-11-11) | merged 2026-11-13 | 34 tests of `dmp_ceff_elmore` / `two_pole` against closed-form RC references. The description says "existing defects on master are guarded by documented `kBuggyMasterTol` bounds" - so the authors know of DMP defects on master, but the description does not say which. The test file itself was not read (it is GPL source). Worth reading the PR's test names/comments only if the licence review allows. |
| [parallaxsw/OpenSTA#450](https://github.com/parallaxsw/OpenSTA/pull/450) Eigen solver (2026-06-16), [#283](https://github.com/parallaxsw/OpenSTA/pull/283) prealloc arrays | merged | Refactors; #450 adds a determinant guard that falls back to lumped capacitance. Not about extreme loads. |
| [parallaxsw/OpenSTA#507](https://github.com/parallaxsw/OpenSTA/pull/507) two-pole zero wire delay (2026-08-31) | merged | `dmp_ceff_two_pole` + `read_spef -reduce`; unrelated to Elmore DMP Ceff. |

- Status: **no report found** for "Ceff much smaller than total at a very long edge / far outside the Liberty table".
- Searched: `Ceff`, `dmp`, `shielding`, `extrapolat`, `large` in both OpenSTA repos; `dmp ceff` and
  `report_dcalc` in OpenROAD. No hit mentions a load or slew outside the table range.
- Published method: Dartu, Menezes, Pileggi, *IEEE TCAD* 15(5):544-553, May 1996 (DOI 10.1109/43.506141) -
  RC load to pi model, then Ceff from a Thevenin (ramp + resistor) driver model solved iteratively.
  The paper's text was not fetched, so I cannot quote what it says at extreme `Rd*C`. Our own Section 8 result
  (ramp fit degenerate in 99% of rows) is a statement about *our* implementation, not a citation.
- Not found: any statement that OpenSTA changes the method outside the table range.

## 2. Driver waveform slew above the table slew

| Item | State | Relevance |
|---|---|---|
| [parallaxsw/OpenSTA#227](https://github.com/parallaxsw/OpenSTA/issues/227) "Inconsistency values of report_checks and report_dcalc" (2025-03-30) | closed same day | sky130 clkbuf: fall slew 0.171071 (report_checks) vs 0.172683 (report_dcalc), delay equal. Maintainer comment (paraphrase): `report_dcalc` shows the **table (LUT) slew**, the algorithm uses the **waveform slew**; the command was changed to also print the waveform slew. A follow-up asking *why* they differ was answered only with a pointer to the Dartu-Menezes-Pileggi paper. |

- Status: **spec**. Table slew vs waveform slew being different quantities is intended and documented by the
  maintainer (2025-03-30/31, parallaxsw). That matches our E3.5 observation (report_dcalc table slew, report_checks propagated slew).
- Not explained upstream: why the gap reaches +40% (ibex `_10092_` 2.34 vs 1.66 ns) or +33% (aes `_08249_`) at mild
  shielding with long input slews. #227's gap is 0.9% at a short edge. No issue found on the magnitude.
- Searched: `slew waveform` (OpenSTA), `report_dcalc` (OpenROAD, OpenSTA), web search for "waveform slew"/"table slew".

## 3. Port-driven net wire delay

- Status: **no report found.**
- Searched: `elmore wire delay`, `input transition port` (OpenSTA), `wire delay port input transition elmore`
  (OpenROAD), org-wide `ideal port driver resistance`, and WebSearch for `ln2 Elmore set_input_transition`.
  Hits were unrelated (set_input_delay reference-pin bugs #452/#528, write_timing_model #518/#535).
- Nearby but not the same: [#405](https://github.com/parallaxsw/OpenSTA/issues/405) involved `set_driving_cell` with
  zero input transition on a Pi model (a different mechanism: a driving cell, not a bare port).
- General background only (not OpenSTA-specific, from a search snippet): wire 50% delay -> ln2 x Elmore for a
  step input is a standard result; that explains the ideal-port ratio 1.4427 but does not by itself explain why
  a 0.15 ns input transition gives 0.40-0.45 x Elmore. I could not confirm any OpenSTA documentation of this.

## Related items seen, not relevant to the three behaviours

[#565](https://github.com/parallaxsw/OpenSTA/issues/565) (prima near-zero slew, closed 2026-11-05),
[#557](https://github.com/parallaxsw/OpenSTA/pull/557) (prima -INF slew), [#578](https://github.com/parallaxsw/OpenSTA/issues/578) (prima slowdown, open),
[#73](https://github.com/parallaxsw/OpenSTA/issues/73) / [#65](https://github.com/parallaxsw/OpenSTA/issues/65) (CCS Ceff display). All about prima/CCS, not the Elmore DMP.

## Opinion on reporting upstream

Behaviour 2 is documented, so a bug report is not warranted; a short question asking how the waveform slew is
defined when it exceeds the table slew (with the ibex/aes numbers) would be reasonable but low value to us, since we
must not read the source for the answer. Behaviours 1 and 3 have no existing report. Behaviour 1 is the strongest
candidate: it is reproducible from one arc and is physically suspicious (a ~1 ns RC shielding a 10-30 ns edge),
but the load is 7x and the input slew 6x beyond the Liberty table, so maintainers may call it extrapolation outside
the characterised range; the report should say so and ask whether it is intended. Behaviour 3 may be an intended
ideal-driver convention, so ask rather than assert a bug. Note that #468 shows upstream knows of unspecified DMP
defects; check that PR's tests and the maintainers' answer before filing. Note also that a reproducer is required
("Issues without test cases are unlikely to get a response", OpenSTA README): the placed jpeg DEF/SPEF and a one-arc
Tcl script would be needed, and building it needs the pinned image, which this note did not run.

### Issue posted: [parallaxsw/OpenSTA#582](https://github.com/parallaxsw/OpenSTA/issues/582)

Posted on 2026-11-15 with a standalone reproducer (`tests/designs/repro_extreme_ceff/`, commit `7bfa26b`): one `sky130_fd_sc_hd__nor2_2`, an explicit pi-model SPEF (`C2=0.114533`, `Rpi=0.497066`, `C1=2.118645`), and an input-slew sweep from `0.05` to `9.142787 ns` in OpenSTA 2.7.0 (`Ceff` drops from 74% to 54% of total load as input slew grows), asking whether this behaviour outside the Liberty table range (`0.303 pF` / `1.5 ns`) is intended and which reported quantities are meaningful there.

## Not confirmed

- Contents of the Dartu et al. paper, the DMP source and `kBuggyMasterTol` details, OpenROAD GitHub discussions,
  and OpenSTA `doc/` (the ChangeLog URL I tried returned 404; the PDF manual was not checked).
