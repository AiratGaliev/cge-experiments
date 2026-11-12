# Native Brick Breaker — CGE Experiments

Standalone Rust proof of stack for a classic Brick Breaker game. The prototype is intentionally isolated in this directory: it is not a workspace member, plugin, runtime contract, or dependency of the production Codogma Engine repository.

## Requirements

### Windows x64

- Windows 10/11 x64.
- Visual Studio 2022 Build Tools with the Desktop development with C++ workload and Windows SDK.
- CMake and Git available on PATH (CMake is also used by SDL3's source build).
- Rust stable toolchain 1.97.0 or newer, with the MSVC target.
- Node.js 22.x and npm.
- A working GPU driver with a D3D12 or Vulkan-capable adapter.

### Linux x64

- Ubuntu 24.04 or a compatible distribution.
- C/C++ compiler, linker, CMake, Ninja, pkg-config, Git and libclang development files.
- SDL3 build dependencies: X11, Wayland, XKB, EGL/OpenGL, DRM/GBM, udev, ALSA, PulseAudio and PipeWire development packages. The CI workflow installs the corresponding Ubuntu packages.
- Rust stable toolchain 1.97.0 or newer.
- Node.js 22.x and npm.
- A working Vulkan or OpenGL-capable GPU driver and a desktop session for interactive execution.

SDL3 is built from source through the crate's `build-from-source` feature. The game uses SDL3 only for its window, input and OS event lifecycle; rendering is performed by wgpu. miniaudio's default bindgen feature is disabled: the crate's pregenerated Linux x86_64 and Windows x86_64 bindings are used.

## Build and run

Run all commands from this directory.

1. Compile and test the TypeScript game rules:

   `npm ci`

   `npm run typecheck`

   `npm run build`

   `npm test`

2. Verify and build the Rust application:

   `cargo fmt --check`

   `cargo clippy --all-targets -- -D warnings`

   `cargo test`

   `cargo run`

The committed `assets/game.js` is generated from `scripts/game.ts` by the pinned TypeScript compiler using the ES2020 target. The CI workflow regenerates it and checks that the committed output is current. Re-run `npm run build` whenever the TypeScript source changes.

## Controls

- A / D or Left / Right Arrow — move the paddle.
- Space — launch the ball; restart after Victory or Game Over.
- Escape — exit.

The demo contains a single level with 36 bricks. Score, lives and game status are rendered in the wgpu surface.

## Runtime boundaries

```text
SDL3 window + event pump
        |
        v
Flecs input system ---> paddle target
        |                     |
        v                     v
Flecs transform sync <--- Box2D v3 fixed-step world
                              |
                        contact begin events
                              |
                copy shape IDs, release completed step
                              |
                remove brick bodies outside Box2D step
                              |
                              v
                    TypeScript game rules
                              |
                       QuickJS-NG context
                        /            \
                  score/lives      host.playSound
                      |                 |
                      v                 v
                 Flecs + HUD      lock-free PCM queue
                                        |
                                  miniaudio callback

Flecs render components ---> CPU rectangle vertices ---> wgpu render pipeline
```

- SDL3 owns the window and keyboard/event pump. The window implements raw-window-handle so wgpu can create a presentable surface.
- wgpu owns the GPU device, surface and a small WGSL pipeline. Every visible paddle, ball, brick and HUD glyph is submitted as triangles; SDL_Renderer is not used.
- Flecs owns the paddle, ball and brick entities. Systems apply player input, synchronize physics positions and hide destroyed bricks. ECS stores application slot IDs, not native Box2D handles.
- boxdd is the only source of physical movement and contacts. The ball is a dynamic bullet body; the paddle is kinematic; walls and bricks are static. Simulation runs at a fixed 1/60 s step with four Box2D solver sub-steps and a bounded render-frame accumulator.
- The Box2D completed-step capability is dropped before any brick body is destroyed. Contact callbacks are not used for gameplay mutation.
- TypeScript is typechecked and compiled to ES2020 JavaScript before execution. The compiled script is evaluated in-process by rquickjs; Node.js is used only by the build/test toolchain, never as the game runtime.
- Rust exposes a deliberately small JS host object containing only `playSound(name)`. No Flecs, Box2D, SDL or GPU objects are exposed to JavaScript.
- The miniaudio callback only drains a bounded lock-free mono PCM queue and writes stereo samples. Tone generation, ECS/physics work and JS execution happen on the main thread. Audio initialization failure is reported and does not prevent the game from starting.

## Pinned stack and upstream provenance

| Technology | Pinned version | Role | License / source |
|---|---:|---|---|
| Rust | 1.97.0 CI toolchain | Application and integration | Apache-2.0 OR MIT |
| SDL3 Rust crate | 0.22.1 | Window, events and keyboard input | MIT crate; SDL3 source: Zlib |
| wgpu | 30.0.1 | GPU surface and render pipeline | MIT OR Apache-2.0 |
| flecs_ecs | 0.2.2 | ECS entities, components and systems | MIT |
| boxdd | 0.6.0 | Safe Rust API over the official Box2D v3 C API | MIT OR Apache-2.0 |
| Box2D | 3.2.0 | Native rigid-body simulation vendored by boxdd-sys | MIT |
| miniaudio | 0.10.0 | Desktop audio output | MIT |
| rquickjs | 0.14.0 | High-level Rust bindings to QuickJS-NG | MIT |
| QuickJS-NG | commit `0fdea21ff1090084e91dad812b343d92e79ba9d9` | Embedded ES2020 JavaScript runtime | MIT |
| TypeScript | 5.9.3 | Source language for game rules | Apache-2.0 |

Box2D binding decision: use `boxdd 0.6.0`, a safe Rust wrapper over the official Box2D v3 C API. Its `boxdd-sys` crate uses a vendored source provider by default; no system Box2D library is discovered or linked. The upstream repository is [erincatto/box2d](https://github.com/erincatto/box2d), revision `56edae79f2949d86142b03450d5d60f63bcf5a6f`, recorded in the crate's `box2d-sys/upstream.toml`. That commit's CMake project identifies the source as Box2D 3.2.0. This is not the separate pure-Rust `box2d-rust` port.

QuickJS decision: `rquickjs 0.14.0` / `rquickjs-sys 0.14.0` vendors QuickJS-NG as a git submodule. The submodule gitlink in the `rquickjs v0.14.0` tag resolves to [quickjs-ng/quickjs](https://github.com/quickjs-ng/quickjs/commit/0fdea21ff1090084e91dad812b343d92e79ba9d9). The Rust process embeds this engine directly. There is no Node.js child process.

## Third-party licenses

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for the pinned dependency notices and upstream license references. The dependency crates and vendored native sources retain their original license files; this prototype does not copy or relicense their code.

Primary sources:

- [SDL3 Rust crate](https://docs.rs/sdl3/0.22.1/sdl3/)
- [wgpu 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/)
- [Flecs Rust binding](https://docs.rs/flecs_ecs/0.2.2/flecs_ecs/)
- [boxdd 0.6.0](https://docs.rs/boxdd/0.6.0/boxdd/)
- [Box2D 3.2.0 source revision](https://github.com/erincatto/box2d/commit/56edae79f2949d86142b03450d5d60f63bcf5a6f)
- [miniaudio 0.10.0](https://docs.rs/miniaudio/0.10.0/miniaudio/)
- [rquickjs 0.14.0](https://docs.rs/rquickjs/0.14.0/rquickjs/)
- [QuickJS-NG pinned revision](https://github.com/quickjs-ng/quickjs/commit/0fdea21ff1090084e91dad812b343d92e79ba9d9)
- [TypeScript 5.9.3](https://www.npmjs.com/package/typescript/v/5.9.3)

## Automated checks and limitations

The focused GitHub Actions workflow runs TypeScript build/tests, Rust formatting, Clippy, unit tests and native target compilation on Windows x64 and Linux x64. It is scoped to this prototype and its workflow file. It generates `Cargo.lock` from exact direct dependency pins and commits the resulting transitive dependency graph to the task branch before platform builds.

This is a proof of stack, not a production game engine or release-ready game. It intentionally excludes asset loading, save data, level progression, gamepad support, audio asset decoding, mobile, WASM, packaging, store distribution and production telemetry. CI compilation does not prove a physical GPU presentation path or that a desktop audio device is available; those two runtime behaviors still require execution on Windows and Linux desktop sessions.
