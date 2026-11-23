# Third-party notices

The following dependencies are pinned by `Cargo.toml`, `package-lock.json` and the generated `Cargo.lock`. Their original license texts and attributions remain with the published crate sources or vendored source distributions.

| Component | Pinned revision | License | Upstream |
|---|---:|---|---|
| winit | `0.30.13` | Apache-2.0 OR MIT | https://github.com/rust-windowing/winit |
| gilrs | `0.11.2` | Apache-2.0 OR MIT | https://github.com/berarma/gilrs |
| wgpu | `30.0.1` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| Flecs Rust binding | `flecs_ecs 0.2.2` | MIT | https://github.com/Indra-db/Flecs-Rust |
| oxijolt | `1.5.0+jolt-5.6.0` | MIT OR Apache-2.0 | https://github.com/pockerhead/oxijolt |
| oxijolt-sys | `1.5.0+jolt-5.6.0` | MIT OR Apache-2.0 | https://github.com/pockerhead/oxijolt |
| Jolt Physics | `5.6.0`, `e77f175595e64cb44218cc9d9d56fc365ad0e36a` | MIT | https://github.com/jrouwe/JoltPhysics |
| joltc | `886e088675bae3a086f8318c7803f8ee962c2f2c` | MIT | https://github.com/amerkoleci/joltc |
| maudio Rust binding | `0.1.15` | MIT | https://github.com/someotherself/maudio |
| miniaudio C library | `0.11.23`, vendored by `maudio-sys` | Unlicense OR MIT No Attribution | https://github.com/mackron/miniaudio |
| rquickjs | `0.14.0` | MIT | https://github.com/DelSkayn/rquickjs |
| QuickJS-NG | `0fdea21ff1090084e91dad812b343d92e79ba9d9` | MIT | https://github.com/quickjs-ng/quickjs |
| TypeScript | `5.9.3` | Apache-2.0 | https://github.com/microsoft/TypeScript |

SDL3, Bevy, Avian and Box2D are not dependencies of this runtime. Flecs remains the ECS implementation. Jolt Physics and joltc are compiled by `oxijolt-sys`: its default `prebuilt` feature uses the matching checksum-verified native archive on supported targets and falls back to its CMake build from the crate's pinned vendor sources when no archive fits. QuickJS-NG is embedded through the pinned `rquickjs-sys` git submodule. miniaudio is built as native C through `maudio-sys`; the Rust wrapper uses pregenerated platform bindings. No third-party source is copied into this repository.
