//! `GRUS_DRT_MEM`-gated resident-footprint instrumentation: a
//! capacity walk (`Vec::capacity() × size_of::<T>()`) over the
//! structures the rip-up loop holds for the whole run, plus
//! `getrusage` high-water marks at named checkpoints — the
//! per-structure split behind `drt-scale-memory` (`docs/results.md`).
//! Emission is `drt: mem <point> key=val …` on stderr, matching the
//! `GRUS_DRT_TIMING` style; zero output when the var is unset.

use std::mem::size_of;

/// `BTreeMap` exposes no capacity API, so `NetTopo.cover`'s per-entry
/// node overhead reports as `entries × BTREE_NODE_EST` beside the
/// exact inner-vec bytes — labeled `~` in the emitted line and never
/// folded into the exact totals.
pub(crate) const BTREE_NODE_EST: usize = 80;

/// Whether the census is live — read once per checkpoint site, so an
/// unset var costs one env lookup per checkpoint, not per structure.
pub fn enabled() -> bool {
    std::env::var_os("GRUS_DRT_MEM").is_some()
}

/// `ru_maxrss` high-water mark in bytes — monotonic, so each
/// checkpoint reads the worst-so-far peak and the leg that set it is
/// attributable. Units differ by platform (macOS bytes, Linux KiB).
pub fn peak_rss_bytes() -> u64 {
    let mut ru = std::mem::MaybeUninit::<libc::rusage>::uninit();
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, ru.as_mut_ptr()) } != 0 {
        return 0;
    }
    let rss = unsafe { ru.assume_init() }.ru_maxrss;
    #[cfg(target_os = "macos")]
    {
        rss as u64
    }
    #[cfg(not(target_os = "macos"))]
    {
        rss as u64 * 1024
    }
}

/// `v`'s heap bytes under the capacity-walk accounting: every
/// `Vec` contributes `capacity() × size_of::<T>()` — exact for the
/// Vec-dominated structures measured here.
pub(crate) fn vec_bytes<T>(v: &Vec<T>) -> usize {
    v.capacity() * size_of::<T>()
}
