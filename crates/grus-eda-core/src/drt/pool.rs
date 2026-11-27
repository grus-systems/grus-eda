//! Per-call rayon pool for the drt stages. Each parallel surface
//! builds its own pool rather than sharing one: the pool size is a
//! call input, and tests force different counts in the same process.
//! `num_threads(0)` keeps rayon's default (available parallelism).

/// A private pool of `threads` workers; `0` keeps rayon's default
/// (available parallelism). The count is an explicit input — the
/// library never reads the environment; the CLI reads
/// `RAYON_NUM_THREADS` once at its boundary (`main.rs`).
pub(crate) fn drt_pool(threads: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("drt thread pool")
}
