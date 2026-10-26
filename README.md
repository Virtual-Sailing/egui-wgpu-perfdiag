# egui-wgpu-perfdiag

Standalone Git fork of `egui-wgpu` 0.33.3 for SailSim performance diagnostics.
The Rust package remains named `egui-wgpu` so it can replace the upstream crate
through Cargo's `[patch.crates-io]` mechanism.

See [PERFDIAG.md](PERFDIAG.md) for the changes, dependency setup, and validation.
Unmodified sibling crates use crates.io releases; no upstream workspace checkout
is required. This fork is consumed from Git and is not published to crates.io.

Metal GPU timing also requires the matching `wgpu-hal-perfdiag` override.
Applications must declare both overrides; patches in dependencies are ignored.

---

# egui-wgpu

[![Latest version](https://img.shields.io/crates/v/egui-wgpu.svg)](https://crates.io/crates/egui-wgpu)
[![Documentation](https://docs.rs/egui-wgpu/badge.svg)](https://docs.rs/egui-wgpu)
![MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Apache](https://img.shields.io/badge/license-Apache-blue.svg)

This crates provides bindings between [`egui`](https://github.com/emilk/egui) and [wgpu](https://crates.io/crates/wgpu).

This was originally hosted at https://github.com/hasenbanck/egui_wgpu_backend
