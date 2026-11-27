//! Per-call rayon pool for the drt stages. `RAYON_NUM_THREADS` is
//! read at call time rather than left to rayon's global pool, which
//! pins the env var on first use process-wide — inside `cargo test`
//! that would make the forced single-thread determinism arms
//! untestable. `num_threads(0)` keeps rayon's default (available
//! parallelism) when the var is unset or unparseable.

/// A private pool honoring `RAYON_NUM_THREADS` as of this call.
pub(crate) fn drt_pool() -> rayon::ThreadPool {
    let n = std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build()
        .expect("drt thread pool")
}
