# Codogma Native 2D Runtime — CGE Experiments

A minimal, scripting-first native 2D engine vertical slice. It uses Rust and a set of focused native libraries for platform, rendering, ECS, physics and audio, while the sample game is authored in TypeScript and executed by embedded QuickJS-NG.

**Brick Breaker is the sample game, not the engine implementation.** Its entities, game rules, collision reactions, score, lives, victory/game-over states and restart flow live in `scripts/game.ts`. Rust provides reusable runtime services and does not contain Brick Breaker-specific gameplay.

This experiment is intentionally independent of the production Codogma Engine repository. It is a technical foundation study, not a Cocos Creator 4 replacement or a production-ready engine.

## What this slice provides

- **SDL3** — native window, keyboard events, focus and resize lifecycle.
- **wgpu 30** — native GPU surface and a small 2D pipeline for colored rectangles, circles and bitmap text. The virtual world keeps a 4:3 aspect ratio; wider and taller windows are letterboxed instead of stretching game geometry.
- **Flecs** — runtime entities/components, structural updates and physics-to-transform synchronization.
- **Box2D v3** through `boxdd` — dynamic/static/kinematic bodies, box/circle colliders, fixed-step simulation and contact events.
- **miniaudio** through `maudio` — native playback device and a bounded lock-free PCM queue.
- **QuickJS-NG** through `rquickjs` — embedded JavaScript execution inside the Rust process.
- **TypeScript 5.9.3** — the source language for the complete Brick Breaker sample. Node.js is only used to typecheck, compile and test it.

## Runtime architecture

```text
SDL3 window / keyboard / OS events
                |
                v
         Rust Runtime loop
        fixed-step scheduling
                |
                v
       QuickJS-NG game hooks <-------- assets/game.js
                |                       compiled from
          typed Host API                scripts/game.ts
                |
        queued runtime commands
          /       |        \
         v        v         v
     Flecs ECS  Box2D v3  miniaudio
         |        |         ^
         |   collision      |
         |     events       |
         |        |         |
         +--------+-----> QuickJS game rules
         |
         +---- render data ----> wgpu pipeline
```

The runtime owns the platform loop and native resources. JavaScript never receives raw Rust pointers, Flecs handles, Box2D handles, SDL objects or wgpu resources. Host bindings decode typed JavaScript values, while native ECS, physics and audio boundaries validate their inputs before applying them. Structural and physics commands are queued and applied outside the Box2D step.

### TypeScript game lifecycle

The loaded script exposes `globalThis.game` with three hooks:

- `initialize(engine)` — create the game world and initialize gameplay.
- `update(engine)` — consume input, time and collision events; issue entity/physics/audio commands.
- `shutdown(engine)` — release game-owned resources when the runtime exits.

The runtime loads the compiled JavaScript from `assets/game.js` **at application startup**. It does not use `include_str!` or compile the game into the Rust executable. After editing `scripts/game.ts`, run `npm run build`; the next launch uses the new script without recompiling Rust. This is startup-time loading, not yet hot reload.

### Host API

The embedded script receives one global `engine` object.

| API | Current contract |
|---|---|
| `engine.entities` | Create rectangle/circle/text entities; set visual position/text; read physics-synchronized X/Y; destroy an entity. |
| `engine.physics` | Create box/circle bodies; set velocity/position; destroy a body; query the current frame's collision entity pairs. Repeated count and entity lookups are non-consuming within the frame. |
| `engine.input` | Query held and just-pressed keyboard keys using platform-neutral names. |
| `engine.time` | Read the current frame delta in seconds. |
| `engine.audio` | Queue a synthesized tone by frequency, duration and volume. `playTone()` returns `true` only when the complete tone fits in the bounded sample queue. |

Entity creation and body creation are separate operations: create the visual ECS entity first, then attach a physics body using its runtime entity ID. Destroying an entity through the entity API also destroys its attached physics body. The API deliberately exposes capabilities rather than implementation objects.

## Build and run

Run all commands from `rust-brick-breaker/`.

### Prerequisites

#### Windows x64

- Windows 10/11 x64.
- Visual Studio 2022 Build Tools with the Desktop development with C++ workload and Windows SDK.
- LLVM/Clang (`libclang` is required by the Flecs Rust binding's bindgen build step), CMake and Git available on PATH.
- Rust stable toolchain 1.97.0 or newer, with the MSVC target.
- Node.js 22.x and npm.
- A working GPU driver with a D3D12 or Vulkan-capable adapter.

#### Linux x64

- Ubuntu 24.04 or a compatible distribution.
- C/C++ compiler, linker, CMake, Ninja, pkg-config, Git and libclang development files, including X11 XTest (`libxtst-dev`).
- SDL3 build dependencies: X11, Wayland, XKB, EGL/OpenGL, DRM/GBM, udev, ALSA, PulseAudio and PipeWire development packages. The CI workflow installs the corresponding Ubuntu packages.
- Rust stable toolchain 1.97.0 or newer.
- Node.js 22.x and npm.
- A working Vulkan or OpenGL-capable GPU driver and a desktop session for interactive execution.

SDL3 is built from source through the crate's `build-from-source` feature. `maudio` uses pregenerated bindings for Windows x64 and Linux x64, so it does not require a second bindgen version.

### Install, validate, build and launch

```bash
npm ci
npm run typecheck
npm run build
npm test

cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
cargo build --locked
cargo run --locked
```

The committed `assets/game.js` is generated from `scripts/game.ts` by the pinned TypeScript compiler using the ES2020 target. Re-run `npm run build` whenever the TypeScript source changes.

To load another compiled JavaScript game without rebuilding the Rust executable, pass its path as the first argument or set `CGE_GAME_SCRIPT`:

```powershell
$env:CGE_GAME_SCRIPT = "assets/another-game.js"
cargo run --locked
```

## Sample game: Brick Breaker

- A / D or Left / Right Arrow — move the paddle.
- Space — launch the ball; restart after Victory or Game Over.
- Escape — exit.

The TypeScript sample creates 36 bricks, walls, a paddle, a ball and HUD labels. Box2D produces contacts; the JavaScript game interprets those events, removes brick entities, awards score, tracks lives and chooses victory/game-over state. The Rust runtime has no Brick Breaker-specific collision roles, score counter or state machine.

## Current limitations

This is intentionally a small engine foundation, not Cocos Creator 4 feature parity. The bitmap font currently covers uppercase ASCII letters and digits. The virtual world uses a fixed 4:3 aspect ratio with letterboxing, not a general camera system. QuickJS currently has no per-hook execution deadline or explicit heap quota; scripts run on the main thread, so only trusted game scripts should be loaded because a runaway callback can block the runtime. It currently has no sprite/texture or asset importer, camera system, animation/tween system, audio asset decoding, UI toolkit, scene serialization/loader, prefab workflow, scripting debugger, hot reload, editor, package manager, save system, gamepad, networking, mobile/WASM target or distribution packaging.

The renderer currently supports colored rectangles, circles and a small built-in bitmap font. Audio currently exposes synthesized tones rather than loading sound assets. The physics facade currently exposes Box2D box/circle rigid bodies. These are explicit limits of the first vertical slice, not implied future capabilities.

## Pinned stack and upstream provenance

| Technology | Pinned version | Role | License / source |
|---|---:|---|---|
| Rust | 1.97.0 CI toolchain | Runtime and native integration | Apache-2.0 OR MIT |
| SDL3 Rust crate | 0.22.1 | Window, events and keyboard input | MIT crate; SDL3 source: Zlib |
| wgpu | 30.0.1 | GPU surface and 2D render pipeline | MIT OR Apache-2.0 |
| flecs_ecs | 0.2.2 | ECS entities, components and systems | MIT |
| boxdd | 0.6.0 | Safe Rust API over official Box2D v3 C API | MIT OR Apache-2.0 |
| Box2D | 3.2.0 | Native rigid-body simulation vendored by boxdd-sys | MIT |
| maudio | 0.1.15 | Safe Rust API and playback device over miniaudio | MIT |
| miniaudio | 0.11.23 | Native audio engine embedded by maudio-sys | Unlicense OR MIT No Attribution |
| rquickjs | 0.14.0 | Rust bindings to QuickJS-NG | MIT |
| QuickJS-NG | commit `0fdea21ff1090084e91dad812b343d92e79ba9d9` | Embedded ES2020 JavaScript runtime | MIT |
| TypeScript | 5.9.3 | Game source language | Apache-2.0 |

Box2D binding: `boxdd 0.6.0`, a safe Rust wrapper over the official Box2D v3 C API. Its `boxdd-sys` crate vendors the source snapshot; no system Box2D library is discovered or linked. The upstream repository is [erincatto/box2d](https://github.com/erincatto/box2d), revision `56edae79f2949d86142b03450d5d60f63bcf5a6f`, which identifies the source as Box2D 3.2.0. This is not the separate pure-Rust `box2d-rust` port.

QuickJS binding: `rquickjs 0.14.0` / `rquickjs-sys 0.14.0` vendors QuickJS-NG as a git submodule. The gitlink in the `rquickjs v0.14.0` tag resolves to [quickjs-ng/quickjs](https://github.com/quickjs-ng/quickjs/commit/0fdea21ff1090084e91dad812b343d92e79ba9d9). There is no Node.js runtime child process.

## Third-party licenses

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for dependency notices and upstream license references. The dependency crates and vendored native sources retain their original license files; this experiment does not copy or relicense their code.

Primary sources:

- [SDL3 Rust crate](https://docs.rs/sdl3/0.22.1/sdl3/)
- [wgpu 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/)
- [Flecs Rust binding](https://docs.rs/flecs_ecs/0.2.2/flecs_ecs/)
- [boxdd 0.6.0](https://docs.rs/boxdd/0.6.0/boxdd/)
- [Box2D 3.2.0 source revision](https://github.com/erincatto/box2d/commit/56edae79f2949d86142b03450d5d60f63bcf5a6f)
- [maudio 0.1.15](https://docs.rs/maudio/0.1.15/maudio/)
- [miniaudio upstream](https://github.com/mackron/miniaudio)
- [rquickjs 0.14.0](https://docs.rs/rquickjs/0.14.0/rquickjs/)
- [QuickJS-NG pinned revision](https://github.com/quickjs-ng/quickjs/commit/0fdea21ff1090084e91dad812b343d92e79ba9d9)
- [TypeScript 5.9.3](https://www.npmjs.com/package/typescript/v/5.9.3)

## Verification

The focused GitHub Actions workflow runs TypeScript typecheck/build/tests, generated-JavaScript consistency, Rust formatting, Clippy, Rust tests and native target compilation on Windows x64 and Linux x64. Hosted CI proves the build/test contract but not interactive GPU presentation or audible output on physical devices.

A local Windows x64 smoke run has also been exercised on an NVIDIA GeForce RTX 3060 Laptop GPU. Runtime screenshots and the interactive behavior should still be checked during development; Linux desktop presentation/audio require their own manual smoke run.
