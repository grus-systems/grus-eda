# E2: Global Placement & Greedy Legalization — Implementation Spec

This document breaks E2 into four concrete implementation steps (`E2.1`–`E2.4`) with exact equations and Rust pseudocode for the implementer.

---

## 1. Done Criteria

| Sub-Step | What | Reference Command (`ghcr.io/librelane/librelane:3.0.16`) | Done When |
|---|---|---|---|
| **E2.1** | Weighted-Average (WA) wirelength & gradient (`placement/wirelength.rs`) | Finite-difference gradient unit test | Analytical gradient matches central finite differences within `< 1e-4` relative error |
| **E2.2** | 2D DCT/IDXST Poisson electrostatic solver (`placement/density.rs`) | Analytical 2D cosine charge test + finite-difference force check | Electric field $(E_x, E_y)$ matches $-\nabla \phi$ and total net force sums to $0$ |
| **E2.3** | Nesterov global placer (`placement/nesterov.rs`) | `openroad`: `global_placement` | Converges to overflow $\tau \le 0.10$ in $< 300$ iterations |
| **E2.4** | Greedy Abacus/Tetris legalizer (`placement/legalize.rs`) & DEF writer | `openroad`: `check_placement -verbose` & `report_design_area` | `check_placement` reports 0 violations, `detailed_placement` accepts output DEF, and legalized HPWL is within 5% of OpenROAD |

---

## 2. Step-by-Step Algorithms & Equations

### Step E2.1: Smooth Weighted-Average (WA) Wirelength (`wirelength.rs`)

> **Two Mandatory `f32` Numerical Safeguards**:
> 1. **Exponent Shift (Preventing `expf` Overflow)**: In `f32`, `exp(x)` overflows to `+inf` as soon as the argument exceeds `88.72` (`NaN` in division). Never evaluate `exp(x_p / gamma)` directly; always subtract the per-net maximum $x_{\max}$ (for positive exponentials) and minimum $x_{\min}$ (for negative exponentials) so every exponent argument is $\le 0$.
> 2. **`f64` Scalar Accumulators for Global Reductions (Preventing Mantissa Swamping)**: Keep all per-cell and per-pin arrays (`pos_x`, `grad_x`, `e_field_x`) as `Vec<f32>`, but accumulate global sums—total WA wirelength $\sum_e W(e)$, $L_1$ norm $\|\nabla W\|_1$, and Barzilai-Borwein $L_2$ dot products $\|v_k - v_{k-1}\|_2^2$, $\|g_k - g_{k-1}\|_2^2$—in scalar **`f64`** (and sum exact integer DBU HPWL in **`i64`**). Summing 100k `f32` values into an `f32` accumulator loses low-order bits and causes the Nesterov step size $\alpha_k$ to oscillate.

Let movable cell $c$ have center coordinate $(x_c, y_c)$ in microns (or normalized DBU `f32`). Each pin $p \in e$ on net $e$ has position:
$$x_p = x_{c(p)} + \Delta x_p, \quad y_p = y_{c(p)} + \Delta y_p$$

For a smoothing parameter $\gamma > 0$, define stabilized exponentials on net $e$ using $x_{\max} = \max_{p \in e} x_p$ and $x_{\min} = \min_{p \in e} x_p$:

$$a_p^+ = \exp\!\left(\frac{x_p - x_{\max}}{\gamma}\right) \in (0, 1], \quad a_p^- = \exp\!\left(\frac{x_{\min} - x_p}{\gamma}\right) \in (0, 1]$$

Define the four net accumulators:
$$A_e^+ = \sum_{p \in e} a_p^+, \quad B_e^+ = \sum_{p \in e} x_p \, a_p^+, \quad A_e^- = \sum_{p \in e} a_p^-, \quad B_e^- = \sum_{p \in e} x_p \, a_p^-$$

Then the WA wirelength along $x$ for net $e$ is:
$$W_x(e) = \frac{B_e^+}{A_e^+} - \frac{B_e^-}{A_e^-}$$

And the analytical partial derivative with respect to pin $p \in e$ is:
$$\frac{\partial W_x(e)}{\partial x_p} = \frac{a_p^+}{\gamma \, (A_e^+)^2} \Bigl(\bigl(\gamma + x_p\bigr) A_e^+ - B_e^+\Bigr) - \frac{a_p^-}{\gamma \, (A_e^-)^2} \Bigl(\bigl(\gamma - x_p\bigr) A_e^- + B_e^-\Bigr)$$

(Identical formulas apply independently to $y_p$.)
To accumulate cell gradients without atomic operations:
1. Compute pin gradients `pin_grad_x[p]` and `pin_grad_y[p]` in parallel across nets `e` (since each pin belongs to at most one net).
2. Gather into `cell_grad_x[c]` and `cell_grad_y[c]` in parallel across movable cells `c` by summing over `cell_pin_ids[cell_pin_offsets[c] .. cell_pin_offsets[c+1]]`.

---

### Step E2.2: Electrostatic Density & 2D Spectral Solver (`density.rs`)

#### 1. Bin Grid Setup
- Choose grid dimension $M = 2^k \in \{64, 128, 256, 512\}$ such that bin width $w_b = W_{\text{core}} / M$ and bin height $h_b = H_{\text{core}} / M$ are approximately $1\text{--}4\times$ standard cell height.
- Local density stretching (ePlace rule): if movable cell $c$ has width $w_c < \sqrt{2} w_b$, stretch its density footprint width to $w'_c = \sqrt{2} w_b$ and scale its density weight by $s_c = \frac{w_c h_c}{w'_c h'_c}$ so total cell area (charge) is conserved while making the overlap function piecewise smooth across bins.
- Accumulate cell area overlap with each bin $(u, v) \in [0, M)^2$ into $\rho(u, v)$, and subtract the DC mean $\bar{\rho} = \frac{1}{M^2}\sum_{u, v} \rho(u, v)$ so net total charge is zero.

#### 2. 1D DCT-II / IDCT / IDXST via `rustfft` (Makhoul's Algorithm)
To compute a 1D DCT-II of length $M$ in $O(M \log M)$ using a standard complex FFT of length $M$:
1. Reorder input $x[0..M]$ into buffer $v[0..M]$:
   $$v[k] = x[2k], \quad v[M - 1 - k] = x[2k + 1] \quad \text{for } k = 0, \dots, \frac{M}{2} - 1$$
2. Run 1D forward complex FFT on $v$ to obtain $V[0..M]$.
3. Multiply by twiddle factors:
   $$\text{DCT}(x)[u] = \text{Re}\!\left(V[u] \cdot e^{-i \frac{\pi u}{2M}}\right) \quad \text{for } u = 0, \dots, M - 1$$

For the inverse transforms needed for potential $\phi$ (IDCT) and electric field $E_x, E_y$ (IDXST):
- Define frequency wavenumbers $\omega_u = \frac{\pi u}{M}$ and $\omega_v = \frac{\pi v}{M}$ (with $\omega_0 = 0$).
- Spectral coefficients for potential $\hat{\phi}_{u, v}$ and electric field $(\hat{E}^x_{u, v}, \hat{E}^y_{u, v})$:
  $$\hat{\phi}_{u, v} = \begin{cases} 0 & \text{if } u = 0 \text{ and } v = 0 \\ \dfrac{a_{u, v}}{\omega_u^2 + \omega_v^2} & \text{otherwise} \end{cases}$$
  $$\hat{E}^x_{u, v} = \omega_u \, \hat{\phi}_{u, v}, \quad \hat{E}^y_{u, v} = \omega_v \, \hat{\phi}_{u, v}$$
- Apply 2D IDCT to $\hat{\phi}_{u,v}$ to get $\phi(u, v)$, 1D IDXST along $u$ + 1D IDCT along $v$ to get $E_x(u, v)$, and 1D IDCT along $u$ + 1D IDXST along $v$ to get $E_y(u, v)$.

---

### Step E2.3: Nesterov Accelerated Gradient Loop (`nesterov.rs`)

Use `f64` scalar accumulators for all vector norms and dot products over `f32` slices:

```rust
#[inline]
pub fn l1_norm_f64(xs: &[f32], ys: &[f32]) -> f64 {
    xs.iter().chain(ys.iter()).map(|&v| (v as f64).abs()).sum()
}

#[inline]
pub fn l2_dist_f64(ax: &[f32], ay: &[f32], bx: &[f32], by: &[f32]) -> f32 {
    let sum_sq: f64 = ax.iter().zip(bx.iter())
        .chain(ay.iter().zip(by.iter()))
        .map(|(&a, &b)| {
            let d = (a as f64) - (b as f64);
            d * d
        })
        .sum();
    sum_sq.sqrt() as f32
}
```

1. **Initialization ($k = 0$)**:
   - Place movable cells near the core center (or via 5–10 iterations of bound-to-bound quadratic CG pulling toward fixed I/O pins) with small deterministic perturbation ($\pm 0.01\,w_b$) so cells do not start on identical coordinates.
   - Initialize wirelength smoothing $\gamma_0 = 4.0 \cdot (w_b + h_b)$.
   - Evaluate initial wirelength gradient $\nabla W(x_0)$ and electric field force $\xi(x_0) = -E(x_0)$, and set initial penalty multiplier:
     $$\lambda_0 = \frac{\|\nabla W(x_0)\|_1}{\|\xi(x_0)\|_1}$$
   - Set Nesterov parameter $t_0 = 1.0$ and reference point $v_0 = x_0$.
2. **Iteration Step ($k \ge 1$)**:
   - Evaluate total gradient at reference point $v_k$:
     $$g_k = \nabla W(v_k) - \lambda_k \, E(v_k)$$
   - Compute Barzilai-Borwein step size $\alpha_k$:
     $$\alpha_k = \frac{\|v_k - v_{k-1}\|_2}{\|g_k - g_{k-1}\|_2}$$
     (With 1–2 backtracking checks if $\alpha_k > \frac{\|v_k - u_k\|_2}{\|g(v_k) - g(u_k)\|_2}$).
   - Update solution and Nesterov lookahead:
     $$u_{k+1} = \text{clamp}_{\text{core}}\bigl(v_k - \alpha_k \, g_k\bigr)$$
     $$t_{k+1} = \frac{1 + \sqrt{1 + 4 t_k^2}}{2}, \quad v_{k+1} = u_{k+1} + \frac{t_k - 1}{t_{k+1}}\bigl(u_{k+1} - u_k\bigr)$$
   - Update $\lambda_{k+1}$ using HPWL ratio $\Delta \text{HPWL} = \text{HPWL}(u_{k+1}) - \text{HPWL}(u_k)$:
     $$\mu_k = \text{clamp}\!\left(1.1^{-\Delta\text{HPWL} / \text{HPWL}_{\text{ref}}},\; 0.95,\; 1.05\right), \quad \lambda_{k+1} = \mu_k \, \lambda_k$$
   - Update smoothing $\gamma_{k+1}$ as overflow $\tau_k$ decreases:
     $$\gamma_{k+1} = \text{base\_gamma} \cdot 10^{2.0 \cdot (\tau_k - 0.1) + 0.0}$$
   - Stop when overflow $\tau_k \le 0.10$ (or at `max_iters = 300`).

---

### Step E2.4: Greedy Tetris / Abacus Legalizer (`legalize.rs`)

After Nesterov finishes, snap every movable standard cell to a valid, non-overlapping `PlacementRow` site in `i32` DBU:

```rust
#[derive(Clone, Debug)]
struct AbacusCluster {
    x_dbu: i32,         // left edge of cluster (snapped to site grid)
    width_dbu: i32,     // sum of cell widths in cluster
    weight_sum: f32,    // e_c = sum of weights (1.0 or pin count)
    q_sum: f32,         // q_c = sum of w_i * (x_i_gp - offset_i)
    cells: Vec<CellId>, // cells in left-to-right order
}
```

#### Algorithm (Abacus Single-Row Dynamic Clustering)
1. **Row Sub-Segments**:
   - For each `PlacementRow`, create sub-row segments `[x_min_dbu, x_max_dbu]` by splitting around any `Fixed` macros or placement blockages overlapping that row's $y$-interval $[y_{\text{row}}, y_{\text{row}} + h_{\text{site}})$.
   - Track remaining capacity `free_width = (x_max_dbu - x_min_dbu) - used_width` per sub-row.
2. **Sort Movable Cells**:
   - Sort all movable cells by their global-placed left-edge coordinate $x_i^{\text{gp}}$ ascending.
3. **Trial Row Placement**:
   - For cell $i$, search sub-rows within $\pm 10$ row heights of $y_i^{\text{gp}}$ that have `free_width >= cell_width[i]`.
   - For each candidate sub-row, simulate appending cell $i$ as a singleton cluster `(x = x_i_gp, width = w_i, e = 1.0, q = x_i_gp)`:
     - **Collapse Loop**: While the last cluster `c_last` overlaps the preceding cluster `c_prev` (`c_prev.x_dbu + c_prev.width_dbu > c_last.x_dbu`):
       - Merge `c_last` into `c_prev`:
         ```rust
         c_prev.q_sum += c_last.q_sum - c_last.weight_sum * (c_prev.width_dbu as f32);
         c_prev.weight_sum += c_last.weight_sum;
         c_prev.width_dbu += c_last.width_dbu;
         c_prev.x_dbu = snap_to_site(
             (c_prev.q_sum / c_prev.weight_sum).round() as i32,
             sub_row.orig_x_dbu,
             sub_row.step_x_dbu,
             sub_row.x_min_dbu,
             sub_row.x_max_dbu - c_prev.width_dbu,
         );
         ```
   - Commit cell $i$ to the candidate sub-row that yields the minimum total displacement cost $\Delta = (x_i^{\text{legal}} - x_i^{\text{gp}})^2 + (y_{\text{row}} - y_i^{\text{gp}})^2$.
4. **Final Commit & Orientation**:
   - Unpack clusters in each sub-row left-to-right, setting `cell_x_dbu[c]`, `cell_y_dbu[c] = row.orig_y_dbu`, `cell_orient[c] = row.orient`, and `cell_status[c] = PlacementStatus::Placed`.
