# Impedance quick guide

Always compute with `calc_impedance` for the actual stackup; these notes explain the choices.

- **Microstrip**: outer-layer trace over a ground plane. Simple; on a JLCPCB 4-layer 7628
  stackup a 50 Ω trace is roughly 0.35–0.37 mm wide (solve it, do not copy this).
- **CPWG (grounded coplanar waveguide)**: microstrip with ground pour beside it at a set gap,
  stitched to the plane below. Narrower traces for the same impedance and better isolation —
  the default for RF feeds on 2-layer and 4-layer boards.
- **Stripline**: inner-layer trace between two planes. Best shielding; needs 6+ layers in practice.
- **Edge-coupled differential pair**: two traces with a fixed gap; the differential impedance
  depends on width, gap and height. Keep the gap constant along the whole route, including
  around vias and pads.

Solder mask lowers outer-layer impedance by a few ohms, and etching makes traces trapezoidal;
for tight tolerances order impedance control from the fab and use their field-solver numbers.
