# Adaptive Brightness RP2040 Firmware

Standalone Rust firmware for a Raspberry Pi Pico / RP2040 and BH1750 ambient light sensor. It exposes a USB CDC lux stream and a Windows HID Ambient Light Sensor interface. Windows uses its inbox HID sensor driver for physical hardware; this repository contains firmware, not a Windows kernel driver.

## Hardware

Connect a 3.3 V BH1750 (GY-302) to the Pico: SDA to GP0/I2C0 SDA, SCL to GP1/I2C0 SCL, and ground to ground. Leave ADDR unconnected for I2C address `0x23`. Do not power the sensor from VBUS or VSYS.

## Build

Install Rust, the `thumbv6m-none-eabi` target, and on Windows the Visual Studio C++ Build Tools workload with a Windows SDK (needed for host procedural macros).

```powershell
rustup target add thumbv6m-none-eabi
cargo build --release
```

Convert the ELF to UF2 with Raspberry Pi `picotool`:

```powershell
picotool uf2 convert target\thumbv6m-none-eabi\release\adaptive-brightness-firmware -t elf target\thumbv6m-none-eabi\release\adaptive-brightness.uf2 --family rp2040
```

Hold BOOTSEL while connecting the board, then copy the UF2 to its mounted RPI-RP2 drive.

## USB interfaces

* CDC emits `LUX:<unsigned integer>` lines every 250 ms; sensor read errors emit `ERROR:BH1750`.
* HID exposes the ambient-light sensor reports used by Windows Sensor APIs. Report ID and feature-report layout are defined in `src/main.rs` and `src/als_feature.rs`.

The Windows HID class driver is included with Windows. The separate VHF repository in this project family is a synthetic test sensor only.

## Validation boundary

The source has previously built and been flashed to a physical RP2040; Windows enumerated its HID interface without a device error and returned native sensor readings. Controlled cover/uncover response and accuracy calibration were not completed. Those observations describe the tested hardware/software revision and are not a guarantee for every board or sensor.
