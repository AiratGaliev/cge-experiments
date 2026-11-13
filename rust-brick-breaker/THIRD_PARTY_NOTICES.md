# Third-party notices

The following dependencies are pinned by `Cargo.toml`, `package-lock.json` and the generated `Cargo.lock`. Their original license texts and attributions remain with the published crate sources or vendored source distributions.

| Component | Pinned revision | License | Upstream |
|---|---:|---|---|
| SDL3 Rust bindings | `sdl3 0.22.1` | MIT | https://github.com/vhspace/sdl3-rs |
| SDL3 native library | Source bundled by `sdl3-sys` | Zlib | https://github.com/libsdl-org/SDL |
| wgpu | `30.0.1` | MIT OR Apache-2.0 | https://github.com/gfx-rs/wgpu |
| Flecs Rust binding | `flecs_ecs 0.2.2` | MIT | https://github.com/Indra-db/Flecs-Rust |
| Box2D Rust binding | `boxdd 0.6.0` | MIT OR Apache-2.0 | https://github.com/Latias94/boxdd |
| Box2D C engine | `3.2.0`, `56edae79f2949d86142b03450d5d60f63bcf5a6f` | MIT | https://github.com/erincatto/box2d |
| maudio Rust binding | `0.1.15` | MIT | https://github.com/someotherself/maudio |
| miniaudio C library | `0.11.23`, vendored by `maudio-sys` | Unlicense OR MIT No Attribution | https://github.com/mackron/miniaudio |
| rquickjs | `0.14.0` | MIT | https://github.com/DelSkayn/rquickjs |
| QuickJS-NG | `0fdea21ff1090084e91dad812b343d92e79ba9d9` | MIT | https://github.com/quickjs-ng/quickjs |
| TypeScript | `5.9.3` | Apache-2.0 | https://github.com/microsoft/TypeScript |

Box2D is built from the pinned vendored upstream C sources through `boxdd-sys`. QuickJS-NG is embedded through the pinned `rquickjs-sys` git submodule. miniaudio is built as native C through `maudio-sys`; the Rust wrapper uses pregenerated platform bindings. No third-party source is copied into this repository.
