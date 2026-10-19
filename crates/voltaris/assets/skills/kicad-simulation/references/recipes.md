# Simulation recipes for simulate_circuit

Replace values with your circuit's; keep the structure.

## RC low-pass corner
```
V1 in 0 AC 1
R1 in out 1k
C1 out 0 100n
.ac dec 20 10 1meg
```
Expect −3 dB at 1/(2πRC) ≈ 1.59 kHz.

## Buck output LC filter with capacitor ESR
```
V1 sw 0 AC 1
L1 sw out 4.7u
C1 out esr 22u
R1 esr 0 5m
Rload out 0 2.5
.ac dec 50 100 10meg
```
Look at peaking near 1/(2π√(LC)); add damping if it exceeds a few dB.

## 3.3 V rail droop during a Wi-Fi transmit burst
```
Vreg src 0 DC 3.3
Rout src a 0.2
Rtrace a rail 0.05
Cbulk rail esr 22u
Resr esr 0 5m
Cdec rail 0 100n
Iload rail 0 PULSE(0 0.35 100u 1u 1u 400u 1m)
.tran 0.2u 1m
```
The minimum of V(rail) must stay above the ESP32's minimum supply with margin.

## Battery divider into an ADC with a filter capacitor
```
Vbat bat 0 DC 4.2
R1 bat adc 100k
R2 adc 0 100k
C1 adc 0 100n
Radc adc 0 1meg
.op
```
Shows the loading error of the ADC's input resistance; the capacitor supplies sampling charge.

## Ideal non-inverting amplifier, gain 10
```
V1 in 0 DC 0.25 AC 1
E1 out 0 in fb 1e6
R1 out fb 9k
R2 fb 0 1k
.op
.ac dec 10 1 1meg
```
