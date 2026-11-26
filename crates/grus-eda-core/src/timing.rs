//! Env-gated phase timers for the heavy commands (`sta`, `power`, `place`,
//! `congestion`), off by default and stderr-only when on. Scopes sit at
//! whole-pass boundaries only — never inside a `par_iter` body or a
//! per-iteration loop — so all drops land on the thread that opened them and
//! the accumulator needs no lock. The gate lives here (not in the CLI) so
//! `run_sta`, `estimate_power` and `legalize_placement` keep their
//! signatures: `GRUS_PHASE_TIMING` set to anything but `0`/`""` enables
//! timing under `cargo test`, benches, and the binary alike.

use std::cell::RefCell;
use std::sync::OnceLock;
use std::time::Instant;

static ENABLED: OnceLock<bool> = OnceLock::new();

/// `GRUS_PHASE_TIMING` gates every scope; read once per process.
pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| {
        std::env::var("GRUS_PHASE_TIMING").is_ok_and(|v| !v.is_empty() && v != "0")
    })
}

// First-touch-ordered `(name, total_ns, count)` per calling thread.
thread_local! {
    static PHASES: RefCell<Vec<(&'static str, u64, u32)>> = const { RefCell::new(Vec::new()) };
}

/// Records its lifetime into the calling thread's accumulator on drop.
pub struct PhaseScope {
    name: &'static str,
    start: Instant,
}

impl Drop for PhaseScope {
    fn drop(&mut self) {
        let ns = self.start.elapsed().as_nanos() as u64;
        PHASES.with(|p| {
            let mut p = p.borrow_mut();
            // ~40 phase names: a linear scan beats a HashMap here.
            match p.iter_mut().find(|e| e.0 == self.name) {
                Some(e) => {
                    e.1 += ns;
                    e.2 += 1;
                }
                None => p.push((self.name, ns, 1)),
            }
        });
    }
}

/// Opens a phase scope when timing is enabled; `None` (one relaxed read)
/// when disabled. Usage: `let _p = timing::phase("sta.eval.walk");`.
pub fn phase(name: &'static str) -> Option<PhaseScope> {
    enabled().then(|| PhaseScope {
        name,
        start: Instant::now(),
    })
}

/// Drains the calling thread's accumulator: `(name, total_ms, count)` in
/// first-touch order.
pub fn take_report() -> Vec<(&'static str, f64, u32)> {
    PHASES.with(|p| {
        p.take()
            .into_iter()
            .map(|(name, ns, count)| (name, ns as f64 / 1e6, count))
            .collect()
    })
}

/// Drains and writes one stderr line per phase, in first-touch order.
pub fn print_report() {
    for (name, ms, count) in take_report() {
        eprintln!("phase {name:<28} {ms:>10.2} ms   x{count}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulator_records_and_drains() {
        {
            let _a = PhaseScope {
                name: "t.a",
                start: Instant::now(),
            };
            let _b = PhaseScope {
                name: "t.b",
                start: Instant::now(),
            };
            drop(_a);
            let _a2 = PhaseScope {
                name: "t.a",
                start: Instant::now(),
            };
        }
        let report = take_report();
        assert_eq!(report.len(), 2, "expected two distinct phase names");
        let (a_name, a_ms, a_count) = report[0];
        let (b_name, b_ms, b_count) = report[1];
        assert_eq!(a_name, "t.a");
        assert_eq!(a_count, 2, "repeated scopes accumulate");
        assert_eq!(b_name, "t.b");
        assert_eq!(b_count, 1);
        assert!(a_ms >= 0.0 && b_ms >= 0.0);
        assert!(
            take_report().is_empty(),
            "take_report drains the accumulator"
        );
    }
}
