# Third-party notices

The following dependencies are pinned by `Cargo.toml`, `package-lock.json` and the generated `Cargo.lock`. Their original license texts and attributions remain with the published crate sources or vendored source distributions.

| Component | Pinned revision | License | Upstream |
|---|---:|---|---|
| winit | `0.30.13` | Apache-2.0 OR MIT | https://github.com/rust-windowing/winit |
| gilrs | `0.11.2` | Apache-2.0 OR MIT | https://github.com/berarma/gilrs |
| wgpu | `30.0.1` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| bevy_ecs | `0.20.1` | MIT OR Apache-2.0 | https://github.com/bevyengine/bevy |
| bevy_app | `0.20.1` | MIT OR Apache-2.0 | https://github.com/bevyengine/bevy |
| bevy_tasks | `0.20.1` | MIT OR Apache-2.0 | https://github.com/bevyengine/bevy |
| bevy_math / bevy_time | `0.20.1` | MIT OR Apache-2.0 | https://github.com/bevyengine/bevy |
| avian2d | `0.8.0` | MIT OR Apache-2.0 | https://github.com/avianphysics/avian |
| Bevy facade crate (transitive through Avian 2D) | `0.20.1`, default features disabled | MIT OR Apache-2.0 | https://github.com/bevyengine/bevy |
| maudio Rust binding | `0.1.15` | MIT | https://github.com/someotherself/maudio |
| miniaudio C library | `0.11.23`, vendored by `maudio-sys` | Unlicense OR MIT No Attribution | https://github.com/mackron/miniaudio |
| rquickjs | `0.14.0` | MIT | https://github.com/DelSkayn/rquickjs |
| QuickJS-NG | `0fdea21ff1090084e91dad812b343d92e79ba9d9` | MIT | https://github.com/quickjs-ng/quickjs |
| TypeScript | `5.9.3` | Apache-2.0 | https://github.com/microsoft/TypeScript |
| Criterion | `0.8.2` | Apache-2.0 OR MIT | https://github.com/bheisler/criterion.rs |

Avian 2D 0.8 uses Bevy's 0.20 API family and includes a Bevy facade dependency with default features disabled. Its dependency graph may include internal support crates; the project does not register Bevy rendering, window integration, input, asset-pipeline or audio plugins. The runtime uses winit, wgpu, gilrs and miniaudio for those services, and initializes only Avian's required compute pool.

QuickJS-NG is embedded through the pinned `rquickjs-sys` git submodule. miniaudio is built as native C through `maudio-sys`; the Rust wrapper uses pregenerated platform bindings. No third-party source is copied into this repository.
