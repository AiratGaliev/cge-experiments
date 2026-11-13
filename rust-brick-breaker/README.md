# Codogma Native 2D Runtime — CGE Experiments

A minimal, scripting-first native 2D engine vertical slice. Rust owns the platform lifecycle, GPU rendering, ECS, physics and audio; the sample game is authored in TypeScript and executed by embedded QuickJS-NG.

**Brick Breaker is the sample game, not the engine implementation.** Entities, game rules, collision reactions, score, lives, victory/game-over states and restart flow live in `scripts/game.ts`. Rust provides reusable runtime services and does not contain Brick Breaker-specific gameplay.

This experiment is independent of the production Codogma Engine repository. It is a technical foundation study, not a Cocos Creator 4 replacement or a production-ready engine.

## What this slice provides

- **winit 0.30** — native desktop window, event-driven lifecycle, physical keyboard input, focus and resize handling.
- **gilrs 0.11** — controller discovery and hotplug, standardized button/axis state, deterministic active-controller selection and keyboard fallback.
- **wgpu 30** — native GPU surface and a small 2D pipeline for colored rectangles, circles and bitmap text. The virtual world keeps a 4:3 aspect ratio; wider and taller windows are letterboxed instead of stretching game geometry.
- **bevy_ecs 0.20** — standalone ECS entities, components, resources, queries and explicit runtime systems/schedules.
- **bevy_tasks 0.20** — the minimal multithreaded compute pool used by Avian's parallel collider-tree queries; only the runtime's `ComputeTaskPool` is initialized.
- **Avian 2D 0.8** — 2D rigid bodies, colliders, material properties, continuous collision detection and collision messages, integrated into the same ECS world.
- **miniaudio** through `maudio` — native playback device and a bounded lock-free PCM queue.
- **QuickJS-NG** through `rquickjs` — embedded JavaScript execution inside the Rust process.
- **TypeScript 5.9.3** — source language for the complete Brick Breaker sample. Node.js is only used to typecheck, compile and test it.

## Runtime architecture

```text
winit window / OS events -------> Rust Runtime event loop <------ gilrs gamepad
                                         |
                                  fixed-step accumulator
                                         |
                           QuickJS-NG game hooks <------ assets/game.js
                                         |                 compiled from
                                   typed Host API          scripts/game.ts
                                         |
                                  queued commands
                                   /     |      \
                                  v      v       v
                              bevy_ecs  Avian 2D  miniaudio
                                  |      |
                                  |  collision messages
                                  |      |
                                  +------+-----> QuickJS game rules
                                  |
                                  +---- render data ----> wgpu pipeline
```

The runtime owns the platform loop and native resources. winit handles window lifecycle and keyboard events; gilrs is polled from the runtime update and never calls game logic from device callbacks. The ECS and Avian physics backend use one Bevy ECS world. Physics has its own explicit schedule and is advanced only by the runtime's fixed-step accumulator. JavaScript never receives Rust pointers, ECS entities, physics handles, gamepad handles, native window objects or wgpu resources. Host bindings decode typed values, and native boundaries validate inputs before applying them.

### Bevy dependency boundary

The project directly uses the standalone `bevy_ecs`, `bevy_app`, `bevy_math`, `bevy_time` and `bevy_tasks` crates. Avian 2D 0.8 currently depends on the Bevy facade crate with default features disabled and only its minimal standard-library/logging features enabled. That facade is therefore present transitively in Cargo's dependency graph. The runtime also initializes only Bevy's global compute task pool, which Avian requires for parallel collider-tree queries; it does not install Bevy's `TaskPoolPlugin` or any renderer, windowing, input, asset, audio or full game-engine stack. winit, wgpu, gilrs and miniaudio remain the runtime's respective platform, rendering, controller and audio services.

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
| `engine.physics` | Create box/circle bodies; set velocity/position; destroy a body; query the current frame's collision entity pairs. |
| `engine.input` | Query held and just-pressed keyboard keys using platform-neutral names; read gamepad connection, normalized Left Stick X and held/just-pressed South/D-pad buttons. |
| `engine.time` | Read the current frame delta in seconds. |
| `engine.audio` | Queue a synthesized tone by frequency, duration and volume. `playTone()` returns `true` only when the complete tone fits in the bounded sample queue. |

The gamepad Host API is intentionally small and platform-neutral:

- `engine.input.gamepad.connected()`
- `engine.input.gamepad.leftStickX()` — normalized to `[-1, 1]` after the 0.18 deadzone.
- `engine.input.gamepad.isDown("South" | "DPadLeft" | "DPadRight")`
- `engine.input.gamepad.wasPressed("South" | "DPadLeft" | "DPadRight")`

No gilrs handles or native pointers cross the QuickJS boundary.

Entity creation and body creation are separate operations: create the visual ECS entity first, then attach a physics body using its runtime entity ID. Destroying an entity through the entity API also destroys its attached physics body. The API deliberately exposes capabilities rather than implementation objects.

## Build and run

Run all commands from `rust-brick-breaker/`.

### Prerequisites

#### Windows x64

- Windows 10/11 x64.
- Visual Studio 2022 Build Tools with the Desktop development with C++ workload and Windows SDK.
- CMake and Git available on PATH.
- Rust stable toolchain 1.97.1 or newer, with the MSVC target.
- Node.js 22.x and npm.
- A working GPU driver with a D3D12 or Vulkan-capable adapter.
- A gamepad is optional; keyboard input remains available when no controller is connected or gilrs initialization fails.

#### Linux x64

- Ubuntu 24.04 or a compatible distribution.
- C/C++ compiler, linker, CMake, Ninja, pkg-config and Git.
- Development packages for Vulkan, ALSA, PulseAudio, PipeWire, udev, DBus, Wayland, XKB, X11 and the selected GPU driver. CI installs the corresponding Ubuntu packages; `libudev-dev` is required by gilrs.
- Permission to read the relevant `/dev/input/event*` devices for gamepad access. Without a usable controller or device permission the application still starts with keyboard input.
- Rust stable toolchain 1.97.1 or newer.
- Node.js 22.x and npm.
- A desktop session and a working Vulkan or OpenGL-capable GPU driver for interactive presentation.

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

| Input | Action |
|---|---|
| A / D or Left / Right Arrow | Move the paddle |
| Space | Launch the ball; restart after Victory or Game Over |
| Gamepad Left Stick X | Analog paddle movement |
| Gamepad D-pad Left / Right | Digital paddle movement |
| Gamepad South | Launch the ball; restart after Victory or Game Over |
| Escape | Exit |

Keyboard and controller directions are combined and clamped to the paddle's movement range. The active controller is the connected device with the lexicographically smallest hardware UUID, making selection independent of device enumeration and event arrival order. Disconnecting the active controller clears its held and edge-triggered state; the next eligible connected controller may become active. The South button uses just-pressed semantics so a held button does not repeatedly launch or restart the game.

The TypeScript sample creates 36 bricks, walls, a paddle, a ball and HUD labels. Avian 2D produces collision messages; the JavaScript game interprets those events, removes brick entities, awards score, tracks lives and chooses victory/game-over state. The Rust runtime has no Brick Breaker-specific collision roles, score counter or state machine.

## Current limitations

This is intentionally a small engine foundation, not Cocos Creator 4 feature parity. The bitmap font currently covers uppercase ASCII letters and digits. The virtual world uses a fixed 4:3 aspect ratio with letterboxing, not a general camera system. QuickJS currently has no per-hook execution deadline or explicit heap quota; scripts run on the main thread, so only trusted game scripts should be loaded because a runaway callback can block the runtime. It currently has no sprite/texture or asset importer, camera system, animation/tween system, audio asset decoding, UI toolkit, scene serialization/loader, prefab workflow, scripting debugger, hot reload, editor, package manager, save system, networking, mobile/WASM target or distribution packaging.

The renderer currently supports colored rectangles, circles and a small built-in bitmap font. Audio currently exposes synthesized tones rather than loading sound assets. The physics facade currently exposes Avian 2D box/circle colliders and rigid bodies.

## Pinned stack and upstream provenance

| Technology | Pinned version | Role | License / source |
|---|---:|---|---|
| Rust | 1.97.1 CI toolchain | Runtime and native integration | Apache-2.0 OR MIT |
| winit | 0.30.13 | Window and platform event lifecycle | Apache-2.0 OR MIT |
| gilrs | 0.11.2 | Standardized gamepad input and hotplug | Apache-2.0 OR MIT |
| wgpu | 30.0.1 | GPU surface and 2D render pipeline | MIT OR Apache-2.0 |
| bevy_ecs | 0.20.1 | Standalone ECS entities, components and systems | MIT OR Apache-2.0 |
| bevy_app | 0.20.1 | Minimal plugin and schedule host | MIT OR Apache-2.0 |
| bevy_math / bevy_time | 0.20.1 | Math and fixed-step clock resources | MIT OR Apache-2.0 |
| avian2d | 0.8.0 | 2D rigid-body physics and collision detection | MIT OR Apache-2.0 |
| maudio | 0.1.15 | Safe Rust API and playback device over miniaudio | MIT |
| miniaudio | 0.11.23 | Native audio engine embedded by maudio | Unlicense OR MIT No Attribution |
| rquickjs | 0.14.0 | Rust bindings to QuickJS-NG | MIT |
| QuickJS-NG | commit `0fdea21ff1090084e91dad812b343d92e79ba9d9` | Embedded ES2020 JavaScript runtime | MIT |
| TypeScript | 5.9.3 | Game source language | Apache-2.0 |
| Criterion | 0.8.2 | Runtime microbenchmarks | Apache-2.0 OR MIT |

Avian 2D uses the Bevy 0.20 API family. Its own dependency graph includes a minimal-feature Bevy facade; no default Bevy engine features are enabled by this project. The dependency lockfile records the exact resolved transitive crates.

QuickJS binding: `rquickjs 0.14.0` / `rquickjs-sys 0.14.0` vendors QuickJS-NG as a git submodule. The gitlink in the `rquickjs v0.14.0` tag resolves to [quickjs-ng/quickjs](https://github.com/quickjs-ng/quickjs/commit/0fdea21ff1090084e91dad812b343d92e79ba9d9). There is no Node.js runtime child process.

## Third-party licenses

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for dependency notices and upstream license references. The dependency crates and vendored native sources retain their original license files; this experiment does not copy or relicense their code.

Primary sources:

- [winit 0.30.13](https://docs.rs/winit/0.30.13/winit/)
- [gilrs 0.11.2](https://docs.rs/gilrs/0.11.2/gilrs/)
- [wgpu 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/)
- [bevy_ecs 0.20.1](https://docs.rs/bevy_ecs/0.20.1/bevy_ecs/)
- [bevy_app 0.20.1](https://docs.rs/bevy_app/0.20.1/bevy_app/)
- [bevy_tasks 0.20.1](https://docs.rs/bevy_tasks/0.20.1/bevy_tasks/)
- [avian2d 0.8.0](https://docs.rs/avian2d/0.8.0/avian2d/)
- [maudio 0.1.15](https://docs.rs/maudio/0.1.15/maudio/)
- [miniaudio upstream](https://github.com/mackron/miniaudio)
- [rquickjs 0.14.0](https://docs.rs/rquickjs/0.14.0/rquickjs/)
- [QuickJS-NG pinned revision](https://github.com/quickjs-ng/quickjs/commit/0fdea21ff1090084e91dad812b343d92e79ba9d9)
- [TypeScript 5.9.3](https://www.npmjs.com/package/typescript/v/5.9.3)

## Verification

The CI Fast workflow runs TypeScript typecheck/build/tests, generated-JavaScript consistency, Rust formatting, Clippy, Rust tests and target checks on feature PRs. The Windows/Linux native build matrix remains a promotion check for PRs into `test` or `main`, as defined by the repository workflow. Hosted CI proves build/test contracts but not interactive GPU presentation, physical controller handling or audible output on hardware; those remain runtime smoke-test items.

The previous Windows GPU smoke test predates this native-platform migration and is not evidence for the current window and controller implementation. The new platform lifecycle and physical controller behavior must be exercised against the migrated build.
