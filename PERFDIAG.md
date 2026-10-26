# Performance diagnostics changes

This repository contains the published `egui-wgpu` 0.33.3 package plus optional
native painter instrumentation. The initial source import is a separate Git
commit so the customizations can be reviewed against the upstream release. The
original crate name and version are retained for Cargo overrides; the fork is
not published to crates.io.

The upstream revision and published archive SHA-256 are recorded in
`package.metadata.perfdiag` in `Cargo.toml`. The standalone manifest keeps
`egui`, `epaint`, `wgpu`, and other unmodified dependencies on crates.io.
It does not use sibling crates from the upstream egui workspace.

## Customizations

- `src/lib.rs` exports `timing` and adds optional `WgpuConfiguration` fields:
  `on_present`, `on_paint_timing`, and `gpu_profiler`. They default to `None`.
- `src/winit.rs` invokes the presentation observer immediately before native
  presentation, reports exclusive CPU upload/acquire/encode/submit/present
  durations (including incomplete paints), and instruments the root UI GPU pass.
  Surface ownership and presentation scheduling stay with the painter.
- `src/timing.rs` provides shared CPU timing types, frame identities, and bounded
  asynchronous GPU samples for the scene and UI. On Windows it uses four
  reusable query/readback slots and skips busy slots. On Metal it uses the
  matching HAL fork's native command-buffer completion observer. It never waits
  for GPU completion; GPU collection is enabled independently at runtime.

The scene renderer, feedback interpretation, graphs, and timing export remain
application code in SailSim. Scene GPU timing and the UI painter share this
module's timer. Metal command-buffer timings and Windows render-pass timestamp
queries have different measurement boundaries.

## Matching Metal dependency

The root `[patch.crates-io]` entry in this repository pins the matching
`wgpu-hal-perfdiag` commit so standalone builds and CI compile the Metal timing
hook. Cargo ignores a dependency's patch table: consuming applications must put
both Git overrides in their own root manifest. SailSim's manifest is the
integration reference. No other sibling crate needs a fork.

CI runs the existing library tests on macOS and Windows. The macOS command is:

```sh
cargo test --lib --locked --no-default-features --features winit,wgpu/metal
```

On Windows, use `winit,wgpu/dx12`. Tests compile the native painter and preserve
the existing CPU tests; they do not validate timings on a physical GPU.
SailSim integration should additionally run its tests and `--render-smoke` on
supported hardware. Commit the lockfile when intentionally updating dependencies
or the pinned HAL revision.
