//! Effective capacitance of an RC load seen by a Liberty NLDM driver, after Dartu, Menezes and
//! Pileggi, "Performance Computation for Precharacterized CMOS Gates with RC Loads" (IEEE TCAD 1996).
//!
//! The driver is a saturated voltage ramp (start `t0`, duration `dt`) behind a resistance `rd`.
//! For a trial `ceff` the ramp is fitted so that a single capacitor `ceff` reproduces the table
//! delay (50%) and slew (20%-80%); `ceff` is then adjusted until the charge that this source
//! delivers into the pi-model load up to the 50% crossing equals the charge delivered into `ceff`.

/// Driving-point pi model: `c1` at the driver, `rpi` in series, `c2` at the far end.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PiModel {
    pub c1: f64,
    pub rpi: f64,
    pub c2: f64,
}

impl PiModel {
    pub fn total(&self) -> f64 {
        self.c1 + self.c2
    }

    /// O'Brien-Savarino reduction from the first three driving-point admittance moments
    /// `Y(s) = y1 s + y2 s^2 + y3 s^3` (`y2 < 0`).
    pub fn from_moments(y1: f64, y2: f64, y3: f64) -> Self {
        if y2.abs() < 1e-30 || y3.abs() < 1e-30 {
            return Self { c1: y1, rpi: 0.0, c2: 0.0 };
        }
        let c2 = y2 * y2 / y3;
        Self { c1: y1 - c2, rpi: -(y3 * y3) / (y2 * y2 * y2), c2 }
    }
}

/// Output of [`solve_dmp`]: the effective capacitance and the table delay and slew at it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DmpResult {
    pub ceff: f64,
    pub delay: f64,
    pub slew: f64,
}

/// Normalised (Vdd = 1) voltage of a capacitor charged through `tau = rd * c` by a ramp that
/// starts at 0 and lasts `dt`.
fn rc_ramp_voltage(u: f64, dt: f64, tau: f64) -> f64 {
    if u <= 0.0 {
        0.0
    } else if u <= dt {
        u / dt - (tau / dt) * (1.0 - (-u / tau).exp())
    } else {
        1.0 - (tau / dt) * (-u / tau).exp() * ((dt / tau).exp() - 1.0)
    }
}

/// Closed-form crossing of `level` after the ramp. `None` unless the crossing is after the ramp
/// (`v(dt) <= level`) and the formula gives a finite time at or after `dt`.
fn rc_crossing_closed(level: f64, dt: f64, tau: f64) -> Option<f64> {
    if rc_ramp_voltage(dt, dt, tau) > level {
        return None;
    }
    let u = -tau * ((1.0 - level) * dt / (tau * (dt / tau).exp_m1())).ln();
    (u.is_finite() && u >= dt).then_some(u)
}

/// Time (from the ramp start) at which the RC response first reaches `level`, by bisection
/// until the bracket is at most 1e-6 ns wide.
fn rc_crossing_bisect(level: f64, dt: f64, tau: f64) -> f64 {
    let g = |u: f64| rc_ramp_voltage(u, dt, tau) - level;
    if let Some(root) = brent_checked(&g, 0.0, dt + tau * 40.0, 1e-6) {
        return root;
    }
    let (mut lo, mut hi) = (0.0, dt + tau * 40.0);
    while hi - lo > 1e-6 {
        let mid = 0.5 * (lo + hi);
        if rc_ramp_voltage(mid, dt, tau) < level {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Brent's method (zbrent) at absolute tolerance `tol` on `[a, b]`, given
/// `f(a)` and `f(b)` of opposite signs (or a zero endpoint, returned at
/// once). Returns the midpoint of the final bracket, whose half-width is
/// below `tol` — the same error bound the bisections this replaces had.
/// Superlinear where bisection is linear, with bisection steps whenever
/// interpolation stalls.
fn brent_root(f: &dyn Fn(f64) -> f64, a: f64, b: f64, fa: f64, fb: f64, tol: f64) -> f64 {
    let (mut a, mut b) = (a, b);
    let mut c = a;
    let mut fa = fa;
    let mut fb = fb;
    let mut fc = fa;
    let (mut d, mut e) = (b - a, b - a);
    for _ in 0..200 {
        if (fb > 0.0 && fc > 0.0) || (fb < 0.0 && fc < 0.0) {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            a = b;
            b = c;
            c = a;
            fa = fb;
            fb = fc;
            fc = fa;
        }
        let tol1 = 2.0 * f64::EPSILON * b.abs() + 0.5 * tol;
        let xm = 0.5 * (c - b);
        if fb == 0.0 {
            return b;
        }
        if xm.abs() <= tol1 {
            // The root lies inside the final [b, c] bracket; its midpoint has
            // half the bracket width as its error bound, matching what the
            // bisections this replaces returned.
            return 0.5 * (b + c);
        }
        if e.abs() >= tol1 && fa.abs() > fb.abs() {
            let s = fb / fa;
            let (mut p, mut q);
            if a == c {
                p = 2.0 * xm * s;
                q = 1.0 - s;
            } else {
                q = fa / fc;
                let r = fb / fc;
                p = s * (2.0 * xm * q * (q - r) - (b - a) * (r - 1.0));
                q = (q - 1.0) * (r - 1.0) * (s - 1.0);
            }
            if p > 0.0 {
                q = -q;
            }
            p = p.abs();
            if 2.0 * p < (3.0 * xm * q - (tol1 * q).abs()).min((e * q).abs()) {
                e = d;
                d = p / q;
            } else {
                d = xm;
                e = d;
            }
        } else {
            d = xm;
            e = d;
        }
        a = b;
        fa = fb;
        b += if d.abs() > tol1 { d } else { tol1.copysign(xm) };
        fb = f(b);
    }
    0.5 * (b + c)
}

/// `brent_root` at `tol` when `f(lo)` and `f(hi)` are finite with opposite
/// signs (or a zero endpoint); otherwise `None`, so the caller can keep its
/// original bisection as the fallback — that fallback is also the NaN guard
/// (`dt == 0` makes `rc_ramp_voltage` return NaN, which bisection absorbs via
/// `NaN < level == false`).
fn brent_checked(f: &dyn Fn(f64) -> f64, lo: f64, hi: f64, tol: f64) -> Option<f64> {
    let (flo, fhi) = (f(lo), f(hi));
    if flo == 0.0 {
        return Some(lo);
    }
    if fhi == 0.0 {
        return Some(hi);
    }
    (flo.is_finite() && fhi.is_finite() && flo * fhi < 0.0).then(|| brent_root(f, lo, hi, flo, fhi, tol))
}

/// Time (from the ramp start) at which the RC response first reaches `level`.
fn rc_crossing(level: f64, dt: f64, tau: f64) -> f64 {
    rc_crossing_closed(level, dt, tau).unwrap_or_else(|| rc_crossing_bisect(level, dt, tau))
}

/// 20%-80% transition time of the RC response, as a function of the ramp duration.
fn rc_slew(dt: f64, tau: f64) -> f64 {
    rc_crossing(0.8, dt, tau) - rc_crossing(0.2, dt, tau)
}

/// True when no ramp can reproduce the table slew with this `rd`: even a step through `rd` is slower than
/// the table slew, so [`fit_ramp`] returns its floor `dt`. The ramp model is outside its domain there.
pub fn ramp_fit_degenerate(rd: f64, c: f64, slew: f64) -> bool {
    fit_ramp_duration(rd, c, slew).1
}

/// Fits the saturated-ramp duration `dt` so that driving capacitor `c` through `rd` produces
/// 20%-80% output slew `slew`. Returns `(dt, degenerate)` where `degenerate` is true when even a
/// step input through `rd` (`dt -> 0`) is at least as slow as `slew`.
pub fn fit_ramp_duration(rd: f64, c: f64, slew: f64) -> (f64, bool) {
    let tau = rd * c;
    // The slew grows monotonically with the ramp duration; at dt -> 0 it is the step response.
    let mut lo = 1e-9 * tau.max(1e-12);
    if rc_slew(lo, tau) >= slew {
        return (lo, true);
    }
    let mut hi = (slew * 4.0).max(lo * 2.0) + tau * 8.0;
    let g = |x: f64| rc_slew(x, tau) - slew;
    if let Some(root) = brent_checked(&g, lo, hi, 1e-6) {
        return (root, false);
    }
    while hi - lo > 1e-6 {
        let mid = 0.5 * (lo + hi);
        if rc_slew(mid, tau) < slew {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (0.5 * (lo + hi), false)
}

/// Ramp parameters `(t0, dt)` such that a capacitor `c` behind `rd` has its 50% point at `delay`
/// and a 20%-80% slew of `slew`. The input 50% point is time 0.
fn fit_ramp(rd: f64, c: f64, delay: f64, slew: f64) -> (f64, f64) {
    let (dt, _) = fit_ramp_duration(rd, c, slew);
    (delay - rc_crossing(0.5, dt, rd * c), dt)
}

/// 20%-80% transition time of the driving-point voltage `V1(t)` when a saturated ramp of duration
/// `dt` behind `rd` drives the pi model `(c1, rpi, c2)` (`c1` near, `c2` far).
pub fn pi_waveform_slew(pi: &PiModel, rd: f64, dt: f64) -> f64 {
    let total = pi.total();
    if rd * total <= 1e-15 {
        return 0.6 * dt;
    }
    if pi.c2 <= 1e-12 || pi.rpi <= 1e-12 {
        return rc_slew(dt, rd * total);
    }
    let c1 = pi.c1.max(1e-6 * total);
    let (c2, rpi) = (pi.c2, pi.rpi);
    let b1 = rd * (c1 + c2) + rpi * c2;
    let b2 = rd * rpi * c1 * c2;
    let disc = (b1 * b1 - 4.0 * b2).max(0.0).sqrt();
    let tau1 = 0.5 * (b1 + disc);
    let tau2 = (b2 / tau1).max(1e-18);
    let tz = rpi * c2;
    let denom = (tau1 - tau2).max(1e-18);
    let k1 = (tau1 - tz) / denom;
    let k2 = (tz - tau2) / denom;
    let v1 = |u: f64| -> f64 {
        if u <= 0.0 {
            return 0.0;
        }
        if dt <= 1e-12 * b1 {
            return 1.0 - k1 * (-u / tau1).exp() - k2 * (-u / tau2).exp();
        }
        if u <= dt {
            return u / dt + (k1 * tau1 * (-u / tau1).exp_m1() + k2 * tau2 * (-u / tau2).exp_m1()) / dt;
        }
        let post = |k: f64, tau: f64| -> f64 {
            if dt <= tau {
                k * (tau / dt) * (-u / tau).exp() * (dt / tau).exp_m1()
            } else {
                k * (tau / dt) * ((-(u - dt) / tau).exp() - (-u / tau).exp())
            }
        };
        1.0 - post(k1, tau1) - post(k2, tau2)
    };
    let crossing = |level: f64| -> f64 {
        let g = |u: f64| v1(u) - level;
        if let Some(root) = brent_checked(&g, 0.0, dt + 40.0 * b1, 1e-7) {
            return root;
        }
        let (mut lo, mut hi) = (0.0, dt + 40.0 * b1);
        for _ in 0..80 {
            if hi - lo <= 1e-7 {
                break;
            }
            let mid = 0.5 * (lo + hi);
            if v1(mid) < level { lo = mid } else { hi = mid }
        }
        0.5 * (lo + hi)
    };
    crossing(0.8) - crossing(0.2)
}

/// Trapezoid steps of the charge integrator: [`pi_charge_numeric`] runs this many steps, and
/// [`pi_charge_discrete`] sums the same recurrence in closed form.
const TRAPEZOID_STEPS: usize = 200;

/// Charge delivered by the ramp source through `rd` into the pi load between `t0` and `tx`:
/// closed form of the discrete recurrence, with the numeric integrator as fallback.
fn pi_charge(pi: &PiModel, rd: f64, t0: f64, dt: f64, tx: f64) -> f64 {
    pi_charge_discrete(pi, rd, t0, dt, tx).unwrap_or_else(|| pi_charge_numeric(pi, rd, t0, dt, tx))
}

/// One eigenmode of the trapezoid step, with `y = h lam / 2`. The step ratio is
/// `rho = (1 + y) / (1 - y)`, so `rho - 1 = 2 y / (1 - y)` exactly and, for `rho > 0`,
/// `ln rho = 2 atanh(y)`. Powers `rho^k - 1` are formed through `expm1`, because the modal sums
/// below subtract nearly equal quantities when `rho` is close to 1.
struct ModalRatio {
    y: f64,
    rho: f64,
    inv: f64,
    log: f64,
}

impl ModalRatio {
    fn new(y: f64) -> Self {
        let inv = 1.0 / (1.0 - y);
        Self { y, rho: (1.0 + y) * inv, inv, log: 2.0 * y.atanh() }
    }

    /// `rho^k - 1`.
    fn pm1(&self, k: usize) -> f64 {
        if self.rho > 0.0 {
            (k as f64 * self.log).exp_m1()
        } else {
            self.rho.powi(k as i32) - 1.0
        }
    }

    /// Charge weight of an input with `m` steps still to run before the end:
    /// `(1 - rho^m) / (1 - rho) + rho^m / 2 = 1/2 + (rho^m - 1) / (2 y)`.
    fn phi(&self, m: usize) -> f64 {
        0.5 + self.pm1(m) / (2.0 * self.y)
    }

    /// `(sum_{m<=n} phi(m), sum_{m<=n} m phi(m))`; zero for `n < 0`. With `E0 = sum (rho^m - 1)`
    /// and `E1 = sum m (rho^m - 1)`, the closed forms are `E0 = P/Q - (n+1)` and
    /// `E1 = [(n+1) Q - P + n P Q - n(n+1)/2 Q^2] / Q^2`, where `P = rho^(n+1) - 1` and
    /// `Q = rho - 1`.
    fn prefix(&self, n: isize) -> (f64, f64) {
        if n < 0 {
            return (0.0, 0.0);
        }
        let nf = n as f64;
        let q = 2.0 * self.y * self.inv;
        let p = self.pm1(n as usize + 1);
        let e0 = p / q - (nf + 1.0);
        let e1 = ((nf + 1.0) * q - p + nf * p * q - 0.5 * nf * (nf + 1.0) * q * q) / (q * q);
        (0.5 * (nf + 1.0) + e0 / (2.0 * self.y), 0.25 * nf * (nf + 1.0) + e1 / (2.0 * self.y))
    }
}

/// `sum_j h phi(N-1-j) u_j` over the `N` trapezoid inputs of one mode, with `u_j = vs_j + vs_{j+1}`
/// and `k = ceil(dt / h)` the first saturated grid index. Inputs `j <= k - 2` are affine
/// (`u_j = (2j + 1) h / dt`), input `j = k - 1` is the straddle step (`u = (k - 1) h / dt + 1`),
/// and inputs `j >= k` are 2. The straddle step is not part of the affine run.
fn modal_input_sum(ratio: &ModalRatio, n: usize, h: f64, dt: f64, k: usize) -> f64 {
    let last = n as isize - 1;
    // Affine inputs are the steps m = N-1-j with m in [N - affine, N - 1].
    let affine = (k - 1).min(n) as isize;
    let (p0_hi, p1_hi) = ratio.prefix(last);
    let (p0_lo, p1_lo) = ratio.prefix(last - affine);
    let affine_sum = (2.0 * n as f64 - 1.0) * (p0_hi - p0_lo) - 2.0 * (p1_hi - p1_lo);
    let mut sum = h / dt * affine_sum;
    if k <= n {
        sum += ratio.phi(n - k) * ((k - 1) as f64 * h / dt + 1.0);
    }
    // Saturated inputs j in [k, N-1] are the steps m in [0, N-1-k].
    sum += 2.0 * ratio.prefix(last - k as isize).0;
    h * sum
}

/// Closed form of the trapezoid recurrence of [`pi_charge_numeric`] (design Section 7.1).
///
/// Each step is `v_{j+1} = P v_j + G u_j`, with `P = (I - hA/2)^-1 (I + hA/2)`, `G` the first
/// column of `(I - hA/2)^-1` times `h g / (2 c1)`, and `u_j = vs_j + vs_{j+1}`. With the spectral
/// projectors `R_i` of `A`, `P = sum_i rho_i R_i`, so `v1` is a sum over the two modes of
/// `alpha_i` times the modal input sums of [`modal_input_sum`]. The charge keeps the loop's
/// endpoint half-weights. `None` if the eigenbasis is ill-conditioned, a mode has `|rho| >= 1`,
/// or the result is not finite.
fn pi_charge_discrete(pi: &PiModel, rd: f64, t0: f64, dt: f64, tx: f64) -> Option<f64> {
    let span = tx - t0;
    if span <= 0.0 {
        return Some(0.0);
    }
    let n = TRAPEZOID_STEPS;
    let h = span / n as f64;
    let c1 = pi.c1.max(1e-4 * pi.total());
    let (c2, rpi) = (pi.c2, pi.rpi);
    let (g, gp) = (1.0 / rd, 1.0 / rpi);
    // A = [[a, b], [d, e]], the system matrix of the pi model.
    let (a, b, d, e) = (-(g + gp) / c1, gp / c1, gp / c2, -gp / c2);
    let tr = a + e;
    // tr^2 - 4 det, written as a sum of squares so that it does not cancel.
    let sq = ((a - e) * (a - e) + 4.0 * b * d).sqrt();
    let lam_a = 0.5 * (tr - sq);
    let lam_b = g * gp / (c1 * c2) / lam_a;
    // Eigenvectors (b, lam - a) normalised; for unit columns ||V||_F ||V^-1||_F = 2 / |det V|.
    let unit = |lam: f64| {
        let (x, y) = (b, lam - a);
        let norm = x.hypot(y);
        (x / norm, y / norm)
    };
    let ((va0, va1), (vb0, vb1)) = (unit(lam_a), unit(lam_b));
    let det_v = va0 * vb1 - vb0 * va1;
    let cond = 2.0 / det_v.abs();
    if cond.is_nan() || cond > 1e12 {
        return None;
    }
    let modes = [ModalRatio::new(0.5 * h * lam_a), ModalRatio::new(0.5 * h * lam_b)];
    if !modes.iter().all(|m| m.rho.abs() < 1.0) {
        return None;
    }
    // (R_i)_11 for the projectors R_a = (A - lam_b) / (lam_a - lam_b) and R_b = (A - lam_a) / (lam_b - lam_a).
    let gap = lam_a - lam_b;
    let r11 = [(a - lam_b) / gap, (a - lam_a) / -gap];
    let kappa = 0.5 * h * g / c1;
    // First saturated grid index, clamped to N+1: beyond that every input is affine.
    let k = (dt / h).ceil().max(1.0).min((n + 1) as f64) as usize;
    let mut iv1 = 0.0;
    for (mode, w) in modes.iter().zip(r11) {
        iv1 += kappa * w * mode.inv * modal_input_sum(mode, n, h, dt, k);
    }
    // sum_{s=1}^{N-1} vs_s is linear up to grid index k - 1 and 1 from k on; vs_N = min(span/dt, 1).
    let linear = (k - 1).min(n - 1);
    let vs_sum = (h / dt) * (linear * (linear + 1)) as f64 / 2.0 + n.saturating_sub(k) as f64;
    let ivs = h * vs_sum + 0.5 * h * (span / dt).min(1.0);
    let q = g * (ivs - iv1);
    q.is_finite().then_some(q)
}

/// Charge delivered by the ramp source through `rd` into the pi load between `t0` and `tx`, by
/// the implicit trapezoid rule with `TRAPEZOID_STEPS` steps. Fallback for [`pi_charge_discrete`], and the
/// reference it is tested against.
fn pi_charge_numeric(pi: &PiModel, rd: f64, t0: f64, dt: f64, tx: f64) -> f64 {
    let c1 = pi.c1.max(1e-4 * pi.total());
    let (c2, rpi) = (pi.c2, pi.rpi);
    let steps = TRAPEZOID_STEPS;
    let h = (tx - t0) / steps as f64;
    if h <= 0.0 {
        return 0.0;
    }
    let vs = |t: f64| ((t - t0) / dt).clamp(0.0, 1.0);
    // Implicit trapezoid on C1 V1' = (Vs - V1)/rd - (V1 - V2)/rpi, C2 V2' = (V1 - V2)/rpi.
    let (g, gp) = (1.0 / rd, 1.0 / rpi);
    let (a11, a12) = (-(g + gp) / c1, gp / c1);
    let (a21, a22) = (gp / c2, -gp / c2);
    let (m11, m12, m21, m22) = (1.0 - 0.5 * h * a11, -0.5 * h * a12, -0.5 * h * a21, 1.0 - 0.5 * h * a22);
    let det = m11 * m22 - m12 * m21;
    let (mut v1, mut v2, mut q) = (0.0, 0.0, 0.0);
    let mut i_prev = (vs(t0) - v1) * g;
    for k in 1..=steps {
        let t = t0 + h * k as f64;
        let rhs1 = v1 + 0.5 * h * (a11 * v1 + a12 * v2) + 0.5 * h * (vs(t - h) + vs(t)) * g / c1;
        let rhs2 = v2 + 0.5 * h * (a21 * v1 + a22 * v2);
        let (n1, n2) = ((rhs1 * m22 - m12 * rhs2) / det, (m11 * rhs2 - m21 * rhs1) / det);
        v1 = n1;
        v2 = n2;
        let i_now = (vs(t) - v1) * g;
        q += 0.5 * h * (i_prev + i_now);
        i_prev = i_now;
    }
    q
}

/// Table of a driver arc: `table(c)` returns the table `(delay, slew)` at load `c`.
pub type DriverTable<'a> = &'a dyn Fn(f64) -> (f64, f64);

fn delay_slope(table: DriverTable, c: f64) -> f64 {
    let h = 0.05 * c;
    ((table(c + h).0 - table(c - h).0) / (2.0 * h)).max(1e-6)
}

/// True when `pi` has no effective shielding or `table(total).1 < 2 ln(5/4) * dslope(total) * total`,
/// where OpenSTA falls back to the unshielded lumped load rather than solving the pi ramp equation.
fn is_lumped_bailout(pi: &PiModel, table: DriverTable) -> bool {
    if pi.c2 <= 0.0 || pi.rpi <= 1e-9 {
        return true;
    }
    let total = pi.total();
    table(total).1 < 2.0 * 1.25f64.ln() * delay_slope(table, total) * total
}

/// Solves for the effective capacitance. `rd_scale` converts the local delay-vs-capacitance slope of the
/// table into the driver resistance.
pub fn solve_dmp(pi: &PiModel, rd_scale: f64, table: DriverTable) -> DmpResult {
    solve_dmp_with(pi, &|table, c| (rd_scale * delay_slope(table, c)).max(1e-6), table)
}

/// [`solve_dmp`] with the driver-resistance rule as a parameter: `rd_of(table, c)` is the resistance
/// used when the load is the capacitor `c`. Lets the rule be measured against the reference tool.
pub fn solve_dmp_with(
    pi: &PiModel,
    rd_of: &dyn Fn(DriverTable, f64) -> f64,
    table: DriverTable,
) -> DmpResult {
    let total = pi.total();
    let lumped = |c: f64| {
        let (delay, slew) = table(c);
        DmpResult { ceff: c, delay, slew }
    };
    if is_lumped_bailout(pi, table) {
        return lumped(total);
    }
    // f(c) > 0 when the pi load takes more charge than a capacitor `c` would: c is too small.
    let residual = |c: f64| {
        let (delay, slew) = table(c);
        let rd = rd_of(table, c);
        let (t0, dt) = fit_ramp(rd, c, delay, slew);
        let tx = delay;
        pi_charge(pi, rd, t0, dt, tx) - 0.5 * c
    };
    let (lo, hi) = (pi.c1.max(1e-4 * total), total);
    let r_hi = residual(hi);
    if r_hi >= 0.0 {
        return lumped(total);
    }
    let r_lo = residual(lo);
    if r_lo <= 0.0 {
        return lumped(lo);
    }
    // `residual` decreases in `c`, so `-residual` has the sign-change bracket
    // [lo, hi] with g(lo) < 0 < g(hi); endpoint values are reused from the
    // early-return checks above. Non-finite endpoints keep the old bisection.
    if r_lo.is_finite() && r_hi.is_finite() {
        let g = |c: f64| -residual(c);
        return lumped(brent_root(&g, lo, hi, -r_lo, -r_hi, 1e-6 * total));
    }
    let (mut lo, mut hi) = (lo, hi);
    while hi - lo > 1e-6 * total {
        let mid = 0.5 * (lo + hi);
        if residual(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lumped(0.5 * (lo + hi))
}

/// Driving-point 20%-80% waveform slew (`V1(t)`) for a solved DMP point `r`.
///
/// OpenSTA distinguishes the table slew at `Ceff` (`r.slew`, reported in `report_dcalc`)
/// from the driving-point waveform slew propagated to downstream stages (`OpenSTA#227`).
/// Uses `rd_w = ln(2) * d(delay)/dC(Ceff)` and falls back to `r.slew` on lumped or
/// extreme-load bailout points (`table(total).1 < 2 ln(5/4) * dslope(total) * total`).
pub fn driver_waveform_slew(pi: &PiModel, r: &DmpResult, table: DriverTable) -> f64 {
    if is_lumped_bailout(pi, table) {
        return r.slew;
    }
    let total = pi.total();
    let c = r.ceff.max(1e-6 * total);
    let rd_w = std::f64::consts::LN_2 * delay_slope(table, c);
    let (dt, _) = fit_ramp_duration(rd_w, c, r.slew);
    pi_waveform_slew(pi, rd_w, dt)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A linear driver: delay = 0.7 * rd * c + 0.05, slew = 2.2 * rd * c + 0.02 (ns, pF, kohm).
    fn linear_table(rd: f64) -> impl Fn(f64) -> (f64, f64) {
        move |c| (0.7 * rd * c + 0.05, 2.2 * rd * c + 0.02)
    }

    #[test]
    fn moments_of_a_single_rc_give_that_rc() {
        let (r, c) = (0.3, 0.02);
        let pi = PiModel::from_moments(c, -r * c * c, r * r * c * c * c);
        assert!((pi.c2 - c).abs() < 1e-12 && (pi.rpi - r).abs() < 1e-12 && pi.c1.abs() < 1e-12);
    }

    #[test]
    fn no_resistance_means_no_shielding() {
        let pi = PiModel { c1: 0.01, rpi: 0.0, c2: 0.02 };
        let r = solve_dmp(&pi, 1.0 / std::f64::consts::LN_2, &linear_table(2.0));
        assert!((r.ceff - 0.03).abs() < 1e-12);
    }

    #[test]
    fn resistive_shielding_lowers_ceff_between_c1_and_total() {
        let pi = PiModel { c1: 0.01, rpi: 5.0, c2: 0.04 };
        let r = solve_dmp(&pi, 1.0 / std::f64::consts::LN_2, &linear_table(1.0));
        assert!(r.ceff > 0.01 && r.ceff < 0.05, "ceff = {}", r.ceff);
        // A stronger driver makes the same load look more shielded.
        let strong = solve_dmp(&pi, 1.0 / std::f64::consts::LN_2, &linear_table(0.2));
        assert!(strong.ceff < r.ceff);
    }

    /// Golden regression: `(table, pi, ceff, delay, slew)` recorded from the solver before the
    /// E3.6 speed-up (2026-11-13). Recorded from the pre-E3.6 implementation; the tolerance covers the stop-rule noise
    /// (~1e-5) only. The charge evaluation must not move the model: see design Section 7.
    #[test]
    fn golden_regression_values_are_reproduced() {
        type TableFn = dyn Fn(f64) -> (f64, f64);
        let tables: [(&str, &TableFn); 3] = [
            ("lin1", &|c| (0.7 * c + 0.05, 2.2 * c + 0.02)),
            ("lin2", &|c| (1.4 * c + 0.05, 4.4 * c + 0.02)),
            ("nl", &|c| (0.05 + 0.9 * c.powf(0.8), 0.02 + 1.4 * c.powf(0.9))),
        ];
        let pis = [
            PiModel { c1: 0.01, rpi: 5.0, c2: 0.04 },
            PiModel { c1: 0.001, rpi: 2.0, c2: 0.2 },
            PiModel { c1: 0.005, rpi: 0.05, c2: 0.03 },
            PiModel { c1: 0.02, rpi: 50.0, c2: 0.01 },
            PiModel { c1: 0.002, rpi: 0.5, c2: 0.002 },
        ];
        #[rustfmt::skip]
        let golden: [(&str, usize, f64, f64, f64); 15] = [
            ("lin1", 0, 1.355989813805e-02, 5.949192869663e-02, 4.983177590370e-02),
            ("lin1", 1, 9.480459451675e-03, 5.663632161617e-02, 4.085701079369e-02),
            ("lin1", 2, 3.421102732420e-02, 7.394771912694e-02, 9.526426011324e-02),
            ("lin1", 3, 2.045850068331e-02, 6.432095047832e-02, 6.500870150328e-02),
            ("lin1", 4, 3.899842679501e-03, 5.272988987565e-02, 2.857965389490e-02),
            ("lin2", 0, 1.634207129478e-02, 7.287889981270e-02, 9.190511369705e-02),
            ("lin2", 1, 3.239255642891e-02, 9.534957900047e-02, 1.625272482872e-01),
            ("lin2", 2, 3.454947739840e-02, 9.836926835775e-02, 1.720177005529e-01),
            ("lin2", 3, 2.076532274485e-02, 7.907145184278e-02, 1.113674200773e-01),
            ("lin2", 4, 3.917413175106e-03, 5.548437844515e-02, 3.723661797047e-02),
            ("nl", 0, 1.250149369240e-02, 7.702788325274e-02, 4.712649929345e-02),
            ("nl", 1, 8.382279634476e-03, 6.963053904824e-02, 3.893014611227e-02),
            ("nl", 2, 3.337034612894e-02, 1.092831236217e-01, 8.563759741969e-02),
            ("nl", 3, 2.011763006449e-02, 8.954612763281e-02, 6.162432191976e-02),
            ("nl", 4, 3.884719073772e-03, 6.061036206277e-02, 2.947439946701e-02),
        ];
        let rel_ok = |got: f64, want: f64| (got - want).abs() <= 5e-5 * want.abs();
        let mut failures = Vec::new();
        for (name, pi_index, ceff, delay, slew) in golden {
            let table = tables.iter().find(|t| t.0 == name).expect("golden table").1;
            let r = solve_dmp(&pis[pi_index], 1.0 / std::f64::consts::LN_2, table);
            if !(rel_ok(r.ceff, ceff) && rel_ok(r.delay, delay) && rel_ok(r.slew, slew)) {
                failures.push(format!("{name} pi{pi_index}: got {r:?}, want ({ceff}, {delay}, {slew})"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Ramp-charge grid: the pi models of the golden set plus a stiff, strongly shielded one.
    fn charge_grid_models() -> Vec<PiModel> {
        vec![
            PiModel { c1: 0.01, rpi: 5.0, c2: 0.04 },
            PiModel { c1: 0.001, rpi: 2.0, c2: 0.2 },
            PiModel { c1: 0.005, rpi: 0.05, c2: 0.03 },
            PiModel { c1: 0.02, rpi: 50.0, c2: 0.01 },
            PiModel { c1: 0.002, rpi: 0.5, c2: 0.002 },
            PiModel { c1: 0.001, rpi: 1e-3, c2: 0.2 },
        ]
    }

    /// Discrete closed form against the 200-step integrator, at 1e-9 relative (1e-18 absolute floor).
    fn assert_charge_agrees(pi: &PiModel, rd: f64, dt: f64, span: f64) {
        let t0 = 0.05;
        let closed = pi_charge_discrete(pi, rd, t0, dt, t0 + span)
            .expect("discrete form must not fall back on the test grid");
        let numeric = pi_charge_numeric(pi, rd, t0, dt, t0 + span);
        assert!(
            (closed - numeric).abs() <= 1e-9 * numeric.abs().max(1e-18) + 1e-18,
            "pi {pi:?} rd {rd} dt {dt} tx-t0 {span}: closed {closed} numeric {numeric}"
        );
    }

    #[test]
    fn discrete_charge_matches_the_integrator() {
        for pi in charge_grid_models() {
            for rd in [0.2, 1.0, 5.0] {
                for dt in [0.02, 0.1, 0.4] {
                    // Both regimes, the regime boundary, and points just either side of it.
                    for span in [0.5 * dt, dt - 1e-9, dt, dt + 1e-9, 1.5 * dt, 4.0 * dt] {
                        assert_charge_agrees(&pi, rd, dt, span);
                    }
                }
            }
            // The ramp saturates inside the interval at a step that is not a grid multiple.
            for (rd, dt, span) in [(1.0, 0.03, 0.07), (0.2, 0.03, 0.07), (5.0, 0.03, 0.0437)] {
                assert_charge_agrees(&pi, rd, dt, span);
            }
            // The whole charge is delivered inside the ramp (tx - t0 < dt).
            for (rd, dt, span) in [(1.0, 0.4, 0.1234), (5.0, 0.4, 0.0009), (0.2, 0.1, 0.0731)] {
                assert_charge_agrees(&pi, rd, dt, span);
            }
        }
    }

    #[test]
    fn charge_is_exactly_zero_at_the_ramp_start_and_continuous_across_its_end() {
        for pi in charge_grid_models() {
            for (rd, dt) in [(0.2, 0.02), (1.0, 0.1), (5.0, 0.4)] {
                let t0 = 0.05;
                assert_eq!(pi_charge_discrete(&pi, rd, t0, dt, t0), Some(0.0));
                let before = pi_charge_discrete(&pi, rd, t0, dt, t0 + dt - 1e-9).unwrap();
                let after = pi_charge_discrete(&pi, rd, t0, dt, t0 + dt + 1e-9).unwrap();
                assert!((before - after).abs() <= 1e-6 * after.abs(), "{before} vs {after}");
            }
        }
    }

    /// The 40-iteration bisection that `rc_crossing` replaced.
    fn rc_crossing_reference(level: f64, dt: f64, tau: f64) -> f64 {
        let mut hi = dt + tau * 40.0;
        let mut lo = 0.0;
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            if rc_ramp_voltage(mid, dt, tau) < level {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// `dt / tau` at which the response reaches `level` exactly at the end of the ramp.
    fn ramp_end_ratio(level: f64) -> f64 {
        let (mut lo, mut hi) = (1e-6, 100.0);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if rc_ramp_voltage(mid, mid, 1.0) > level {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        0.5 * (lo + hi)
    }

    #[test]
    fn closed_form_crossing_matches_full_bisection_where_it_applies() {
        for level in [0.2, 0.5, 0.8] {
            let x_end = ramp_end_ratio(level);
            for tau in [0.003, 0.05, 0.4] {
                for x in [0.05 * x_end, 0.5 * x_end, 0.99 * x_end] {
                    let dt = x * tau;
                    assert!(rc_ramp_voltage(dt, dt, tau) <= level);
                    let closed = rc_crossing_closed(level, dt, tau)
                        .expect("closed form must apply when v(dt) <= level");
                    let reference = rc_crossing_reference(level, dt, tau);
                    assert!(
                        (closed - reference).abs() <= 1e-6,
                        "level {level} dt {dt} tau {tau}: closed {closed} reference {reference}"
                    );
                }
            }
        }
    }

    #[test]
    fn crossing_at_the_ramp_end_and_before_it_is_handled() {
        for level in [0.2, 0.5, 0.8] {
            let x_end = ramp_end_ratio(level);
            for tau in [0.003, 0.05, 0.4] {
                // Boundary: the level is exactly the response at the end of the ramp.
                let dt = x_end * tau;
                let boundary = rc_ramp_voltage(dt, dt, tau);
                let reference = rc_crossing_reference(boundary, dt, tau);
                assert!((rc_crossing(boundary, dt, tau) - reference).abs() <= 1e-6);
                // Not triggered: the crossing happens during the ramp.
                let dt = 1.5 * x_end * tau;
                assert!(rc_crossing_closed(level, dt, tau).is_none());
                let reference = rc_crossing_reference(level, dt, tau);
                assert!((rc_crossing(level, dt, tau) - reference).abs() <= 1e-6);
            }
        }
    }

    #[test]
    fn pi_waveform_slew_matches_limits_and_ode_simulation() {
        // Zero rpi reduces to the single-pole lumped rc_slew.
        let pi_lumped = PiModel { c1: 0.2, rpi: 0.0, c2: 0.8 };
        for (rd, dt) in [(0.5, 0.0), (0.5, 0.3), (2.0, 1.5)] {
            let got = pi_waveform_slew(&pi_lumped, rd, dt);
            let want = rc_slew(dt, rd * 1.0);
            assert!((got - want).abs() <= 1e-6, "lumped rd {rd} dt {dt}: got {got} want {want}");
        }

        // General pi model matches a fine-step backward-Euler/trapezoid ODE trajectory of V1(t).
        for pi in [
            PiModel { c1: 0.114533, rpi: 0.497066, c2: 2.118645 },
            PiModel { c1: 0.01, rpi: 5.0, c2: 0.04 },
            PiModel { c1: 0.2152, rpi: 0.5469, c2: 1.8 },
        ] {
            for (rd, dt) in [(1.5, 0.0), (1.3, 3.6), (0.8, 0.05), (9.9, 0.0)] {
                let got = pi_waveform_slew(&pi, rd, dt);
                let total = pi.total();
                let c1 = pi.c1.max(1e-6 * total);
                let (c2, rpi) = (pi.c2, pi.rpi);
                let (g, gp) = (1.0 / rd, 1.0 / rpi);
                let (a11, a12) = (-(g + gp) / c1, gp / c1);
                let (a21, a22) = (gp / c2, -gp / c2);
                let t_end = dt + 10.0 * (rd * total + rpi * c2);
                let steps = 100_000usize;
                let h = t_end / steps as f64;
                let vs = |t: f64| if dt <= 1e-12 { 1.0 } else { (t / dt).clamp(0.0, 1.0) };
                let (m11, m12, m21, m22) = (1.0 - 0.5 * h * a11, -0.5 * h * a12, -0.5 * h * a21, 1.0 - 0.5 * h * a22);
                let det = m11 * m22 - m12 * m21;
                let (mut v1, mut v2) = (0.0, 0.0);
                let (mut t20, mut t80) = (None, None);
                for k in 1..=steps {
                    let t_prev = h * (k - 1) as f64;
                    let t = h * k as f64;
                    let rhs1 = v1 + 0.5 * h * (a11 * v1 + a12 * v2) + 0.5 * h * (vs(t_prev) + vs(t)) * g / c1;
                    let rhs2 = v2 + 0.5 * h * (a21 * v1 + a22 * v2);
                    let n1 = (rhs1 * m22 - m12 * rhs2) / det;
                    let n2 = (m11 * rhs2 - m21 * rhs1) / det;
                    if t20.is_none() && n1 >= 0.2 {
                        let frac = (0.2 - v1) / (n1 - v1);
                        t20 = Some(t_prev + frac * h);
                    }
                    if t80.is_none() && n1 >= 0.8 {
                        let frac = (0.8 - v1) / (n1 - v1);
                        t80 = Some(t_prev + frac * h);
                        break;
                    }
                    v1 = n1;
                    v2 = n2;
                }
                let sim = t80.unwrap() - t20.unwrap();
                assert!(
                    (got - sim).abs() <= 1e-3 * sim.max(1e-3),
                    "pi {pi:?} rd {rd} dt {dt}: closed {got} sim {sim}"
                );
            }
        }
    }
}
