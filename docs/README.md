# Documentation

Start here. Everything under `docs/` is written in English.

## Read in this order

1. [`../README.md`](../README.md): what grus-eda is, the implemented stage map, and how a stage counts as done.
2. [`architecture.md`](architecture.md): the shared data layout (SoA/CSR), the pipeline, and the numeric model.
3. One design document per consolidated stage: [`e0-readers.md`](e0-readers.md), [`e1-congestion.md`](e1-congestion.md),
   [`e2-placement.md`](e2-placement.md), [`e3-sta.md`](e3-sta.md), [`e4-guides.md`](e4-guides.md),
   [`e5-cts.md`](e5-cts.md), [`e6-drt.md`](e6-drt.md), [`e7-drc.md`](e7-drc.md),
   [`e9-reporting.md`](e9-reporting.md). E8 IR drop and N7 power estimation do not yet have
   consolidated stage documents; their current evidence is in `status.md`, `results.md`, and `roadmap.md`.
4. [`status.md`](status.md): the current state of each stage, the tolerances actually used, and the known gaps.
5. [`results.md`](results.md): the log of every comparison against OpenROAD, OpenSTA, Magic, and PDNSim,
   including the runs that failed and why.
6. [`roadmap.md`](roadmap.md): remaining correctness and performance work.

## History

[`history/`](history/README.md) holds the per-step design documents, the implementation tickets, and the
2026-11-11 project review. They record how the work was done; they are not current guidance. When a
history document and the documents above disagree, the documents above win.
