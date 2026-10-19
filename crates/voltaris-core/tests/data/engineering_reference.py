"""Independent reference values for the engineering toolset's unit tests.

Run with scikit-rf 2.2.0:  python3 engineering_reference.py
Microstrip: Hammerstad-Jensen with thickness correction, no dispersion,
lossless dielectric. CPWG: conductor-backed CPW, zero thickness.
"""
import warnings
import numpy as np
import skrf
from skrf.media import MLine, CPW

warnings.filterwarnings("ignore")
f = skrf.Frequency(1, 1, 1, unit="MHz")

print("const REF_MICROSTRIP: &[(f64, f64, f64, f64, f64, f64)] = &[")
for w, h, t, er in [
    (0.36, 0.2104, 0.035, 4.4), (0.2, 0.2104, 0.035, 4.4), (0.18, 0.0994, 0.035, 4.1),
    (1.0, 0.2104, 0.035, 4.4), (3.0, 1.53, 0.035, 4.5), (0.5, 1.53, 0.035, 4.5),
    (0.1, 0.0764, 0.0, 3.91), (2.0, 0.5, 0.0, 10.2),
]:
    m = MLine(frequency=f, w=w * 1e-3, h=h * 1e-3, t=(t * 1e-3) or None, ep_r=er,
              model="hammerstadjensen", disp="none", diel="frequencyinvariant", tand=0, rho=1.724e-8)
    z = float(m._z_characteristic[0].real)
    e = float(m.ep_reff_f[0].real) if hasattr(m, "ep_reff_f") else float("nan")
    print(f"    ({w}, {h}, {t}, {er}, {z:.4f}, {e:.4f}),")
print("];")

print("const REF_CPWG: &[(f64, f64, f64, f64, f64, f64)] = &[")
for w, s, h, er in [(0.3, 0.15, 0.2104, 4.4), (0.5, 0.2, 0.2104, 4.4), (1.0, 0.15, 1.53, 4.5), (0.25, 0.1, 0.0994, 4.1)]:
    c = CPW(frequency=f, w=w * 1e-3, s=s * 1e-3, h=h * 1e-3, ep_r=er, t=None,
            has_metal_backside=True, diel="frequencyinvariant", tand=0, rho=1.724e-8)
    z = float(np.ravel(np.real(c._z_characteristic))[0])
    e = float(np.ravel(np.real(c.ep_reff))[0])
    print(f"    ({w}, {s}, {h}, {er}, {z:.4f}, {e:.4f}),")
print("];")
