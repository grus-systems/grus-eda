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

> **Three Mandatory `f32` Numerical Safeguards**:
> 1. **Exponent Shift (Preventing `expf` Overflow)**: In `f32`, `exp(x)` overflows to `+inf` as soon as the argument exceeds `88.72` (`NaN` in division). Never evaluate `exp(x_p / gamma)` directly; always use shifted coordinates $\tilde{x}_p^+ = x_p - x_{\max} \le 0$ and $\tilde{x}_p^- = x_p - x_{\min} \ge 0$ so every exponent argument is $\le 0$.
> 2. **Shifted Coordinates Inside $B_e^\pm$ and Gradient (Preventing Catastrophic Cancellation)**: If $B_e^+ = \sum x_p a_p^+$ is evaluated with large absolute coordinates (e.g., $x_p \approx 5000\,\mu\text{m}$ while net span is $2\,\mu\text{m}$), subtracting $x_p A_e^+ - B_e^+$ in `f32` cancels the leading 12+ bits of mantissa! Because shifting all $x_p$ on net $e$ by a constant shifts $W_x^+(e)$ by that constant and leaves the gradient $\frac{\partial W_x(e)}{\partial x_p}$ **mathematically invariant**, always compute $\tilde{B}_e^+$ and the gradient using $\tilde{x}_p^+ = x_p - x_{\max}$ and $\tilde{x}_p^- = x_p - x_{\min}$.
> 3. **`f64` Scalar Accumulators for Global Reductions (Preventing Mantissa Swamping)**: Keep all per-cell and per-pin arrays (`pos_x`, `grad_x`, `e_field_x`) as `Vec<f32>`, but accumulate global sums—total WA wirelength $\sum_e W(e)$, $L_1$ norm $\|\nabla W\|_1$, and Barzilai-Borwein $L_2$ dot products $\|v_k - v_{k-1}\|_2^2$, $\|g_k - g_{k-1}\|_2^2$—in scalar **`f64`** (and sum exact integer DBU HPWL in **`i64`**).

Let movable cell $c$ have center coordinate $(x_c, y_c)$ in microns (with die lower-left origin shifted to $(0, 0)$). Each pin $p \in e$ on net $e$ has position:
$$x_p = x_{c(p)} + \Delta x_p, \quad y_p = y_{c(p)} + \Delta y_p$$

For a smoothing parameter $\gamma > 0$, define shifted pin coordinates $\tilde{x}_p^+ = x_p - x_{\max} \le 0$ and $\tilde{x}_p^- = x_p - x_{\min} \ge 0$ (where $x_{\max} = \max_{p \in e} x_p$ and $x_{\min} = \min_{p \in e} x_p$):

$$a_p^+ = \exp\!\left(\frac{\tilde{x}_p^+}{\gamma}\right) \in (0, 1], \quad a_p^- = \exp\!\left(\frac{-\tilde{x}_p^-}{\gamma}\right) \in (0, 1]$$

Define the four shifted net accumulators:
$$A_e^+ = \sum_{p \in e} a_p^+, \quad \tilde{B}_e^+ = \sum_{p \in e} \tilde{x}_p^+ \, a_p^+, \quad A_e^- = \sum_{p \in e} a_p^-, \quad \tilde{B}_e^- = \sum_{p \in e} \tilde{x}_p^- \, a_p^-$$

Then the WA wirelength along $x$ for net $e$ is:
$$W_x(e) = (x_{\max} - x_{\min}) + \frac{\tilde{B}_e^+}{A_e^+} - \frac{\tilde{B}_e^-}{A_e^-}$$

And the analytical partial derivative with respect to pin $p \in e$ (free of catastrophic cancellation) is:
$$\frac{\partial W_x(e)}{\partial x_p} = \frac{a_p^+}{\gamma \, (A_e^+)^2} \Bigl(\bigl(\gamma + \tilde{x}_p^+\bigr) A_e^+ - \tilde{B}_e^+\Bigr) - \frac{a_p^-}{\gamma \, (A_e^-)^2} \Bigl(\bigl(\gamma - \tilde{x}_p^-\bigr) A_e^- + \tilde{B}_e^-\Bigr)$$

(Identical formulas apply independently to $y_p$.)
To accumulate cell gradients without atomic operations:
1. Compute pin gradients `pin_grad_x[p]` and `pin_grad_y[p]` in parallel across nets `e` (since each pin belongs to at most one net).
2. Gather into `cell_grad_x[c]` and `cell_grad_y[c]` in parallel across movable cells `c` by summing over `cell_pin_ids[cell_pin_offsets[c] .. cell_pin_offsets[c+1]]`.

---

### Step E2.2: Electrostatic Density & 2D Spectral Solver (`density.rs`)

#### 1. Physical-Only Cell Classification & Bin Grid Setup
- **Cell Filtering**:
  - `MacroClass::CoreSpacer` (`sky130_fd_sc_hd__fill_*`, post-placement decap fillers): excluded from E2 (if present in an input DEF, strip them on load).
  - `PlacementStatus::Fixed` cells (`tapvpwrvgnd`, endcap `decap`, block macros): stationary. Their area is added to the fixed background bin density $\rho_{\text{fixed}}(u, v)$ scaled by $\rho_{\text{target}}$, and they act as fixed obstacles splitting rows in Step E2.4.
  - `PlacementStatus::Placed` / `Unplaced` standard cells (`MacroClass::Core` / `CoreTie`): movable optimization variables.
- Choose grid dimension $M = 2^k \in \{64, 128, 256, 512\}$ such that bin width $w_b = W_{\text{core}} / M$ and bin height $h_b = H_{\text{core}} / M$ are approximately $1\text{--}4\times$ standard cell height.
- Local density stretching (ePlace rule): if movable cell $c$ has width $w_c < \sqrt{2} w_b$, stretch its density footprint width to $w'_c = \sqrt{2} w_b$ and scale its density weight by $s_c = \frac{w_c h_c}{w'_c h'_c}$ so total cell area (charge) is conserved while making the overlap function piecewise smooth across bins.
- Accumulate cell area overlap with each bin $(u, v) \in [0, M)^2$ into $\rho(u, v)$, and subtract the DC mean $\bar{\rho} = \frac{1}{M^2}\sum_{u, v} \rho(u, v)$ so net total charge is zero.

#### 2. Exact 1D Forward DCT-II, Inverse IDCT (DCT-III), and Inverse IDXST via `rustfft`
Let $M$ be a power of 2:

- **1D Forward DCT-II (`dct1d`, unnormalized $\sum_{n=0}^{M-1} x_n \cos(\frac{\pi u (2n+1)}{2M})$)**:
  1. Reorder $x[0..M]$ into complex buffer $v[0..M]$:
     $$v[k] = x[2k], \quad v[M - 1 - k] = x[2k + 1] \quad \text{for } k = 0, \dots, \frac{M}{2} - 1$$
  2. Run 1D forward complex FFT on $v \to V$.
  3. Multiply by twiddle factors:
     $$C[u] = \text{Re}\!\left(V[u] \cdot e^{-i \frac{\pi u}{2M}}\right) \quad \text{for } u = 0, \dots, M - 1$$

- **1D Inverse DCT (`idct1d`, satisfying $\text{idct1d}(\text{dct1d}(x)) = x$)**:
  1. Given spectral coefficients $C[0..M]$, form complex frequency vector $W[0..M]$:
     $$W[0] = \frac{1}{M} C[0], \quad W[u] = \frac{2}{M}\bigl(C[u] - i \, C[M - u]\bigr)\,e^{i \frac{\pi u}{2M}} \quad \text{for } u = 1, \dots, M - 1$$
     (or equivalently run inverse complex IFFT on $V[u] = \frac{2}{M} C[u] e^{i \frac{\pi u}{2M}}$ with $V[0] = \frac{1}{M} C[0]$ and take the real part).
  2. Un-permute the real part $v[0..M]$ back to spatial order:
     $$x[2k] = \text{Re}(v[k]), \quad x[2k + 1] = \text{Re}(v[M - 1 - k]) \quad \text{for } k = 0, \dots, \frac{M}{2} - 1$$

- **1D Inverse Shifted Sine Transform (`idxst1d`, computing $\sum_{u=1}^{M-1} S[u] \sin(\frac{\pi u (2n+1)}{2M})$)**:
  Using the identity $\sin\!\bigl(\frac{\pi u(2n+1)}{2M}\bigr) = (-1)^n \cos\!\bigl(\frac{\pi (M - u)(2n+1)}{2M}\bigr)$:
  1. Define reversed cosine coefficients $\tilde{C}[0] = 0$ and $\tilde{C}[u] = S[M - u]$ for $u = 1, \dots, M - 1$.
  2. Compute $y = \text{idct1d}(\tilde{C})$.
  3. Flip signs on odd spatial indices:
     $$\text{idxst1d}(S)[n] = (-1)^n \, y[n] \quad \text{for } n = 0, \dots, M - 1$$

- **2D Spectral Poisson Solve**:
  1. Apply `dct1d` to all rows of $\rho(u, v)$, then `dct1d` to all columns to obtain $a_{u, v}$.
  2. Define wavenumbers $\omega_u = \frac{\pi u}{M w_b}$ and $\omega_v = \frac{\pi v}{M h_b}$:
     $$\hat{\phi}_{u, v} = \begin{cases} 0 & \text{if } u = 0 \text{ and } v = 0 \\ \dfrac{a_{u, v}}{\omega_u^2 + \omega_v^2} & \text{otherwise} \end{cases}, \quad \hat{E}^x_{u, v} = \omega_u \, \hat{\phi}_{u, v}, \quad \hat{E}^y_{u, v} = \omega_v \, \hat{\phi}_{u, v}$$
  3. Recover $\phi(x, y)$ by applying `idct1d` along rows and `idct1d` along columns of $\hat{\phi}$.
  4. Recover $E_x(x, y)$ by applying `idxst1d` along $u$ (horizontal) and `idct1d` along $v$ (vertical) of $\hat{E}^x$.
  5. Recover $E_y(x, y)$ by applying `idct1d` along $u$ (horizontal) and `idxst1d` along $v$ (vertical) of $\hat{E}^y$.

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
