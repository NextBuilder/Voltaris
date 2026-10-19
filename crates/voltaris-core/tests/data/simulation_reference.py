"""Independent reference for the built-in solver's transient test.

Series R-L-C low-pass (output across C), R=2 ohm, L=10 uH, C=10 uF, 1 V step:
H(s) = 1 / (L*C*s^2 + R*C*s + 1). Evaluated with scipy.signal.step.
"""
import numpy as np
from scipy import signal

R, L, C = 2.0, 10e-6, 10e-6
sys = signal.lti([1.0], [L * C, R * C, 1.0])
times = [5e-6, 10e-6, 20e-6, 31.4e-6, 50e-6, 80e-6, 150e-6]
t = np.linspace(0, 200e-6, 2000001)
_, y = signal.step(sys, T=t)
print("    const REF_RLC_STEP: &[(f64, f64)] = &[")
for tr in times:
    print(f"        ({tr:e}, {np.interp(tr, t, y):.6f}),")
print("    ];")
