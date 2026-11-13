# Runtime performance benchmarks

This document records the CPU-side performance comparison for the native 2D runtime. It identifies promising optimization areas; it does not claim a particular game FPS.

## Reproduction

Run from rust-brick-breaker/ on the same machine, using the same power profile and without competing CPU-heavy workloads:

    cargo bench --bench runtime --locked -- --save-baseline before --noplot
    cargo bench --bench runtime --locked -- --baseline before --noplot

The first command records the pre-optimization baseline; the second compares the current implementation against it. Criterion stores raw measurements and reports under target/criterion/, which is build output and is not committed. Keep the before directory when repeating the comparison. Use a new baseline name for a later optimization round rather than overwriting the original.

The suite uses Criterion 0.8.2, 1 second warm-up, 2 seconds measurement and 30 samples per case. Build profile is Criterion's optimized benchmark profile.

## Measurement environment

| Item | Value |
|---|---|
| OS | Windows 11 IoT Enterprise LTSC, build 26100 |
| CPU | AMD Ryzen 5 5600H, 6 cores / 12 logical processors |
| Rust target | x86_64-pc-windows-msvc |
| Rust compiler | rustc 1.99.1 |
| Benchmark framework | Criterion 0.8.2 |
| Workload | Native CPU microbenchmarks; no GPU frame capture |

Results are machine-specific. Comparisons are meaningful only when the same binary target, workload, hardware, power mode and background-load conditions are used.

## What is measured

| Group | Benchmark | Workload |
|---|---|---|
| QuickJS | context_create_and_evaluate | Create the runtime/context and evaluate a small game script |
| QuickJS | noop_hook_update | Invoke a cached empty game.update() hook |
| Host API | frame_submit_and_update/{16,64,256} | Zero-copy handoff of frame buffers and one batched read of 16, 64 or 256 transforms |
| Gameplay | brick_breaker_update | Execute the compiled Brick Breaker TypeScript game update |
| Box2D | fixed_step/{40,256,1024} | Step a world containing active moving dynamic bodies |
| Box2D | positions_snapshot/{40,256,1024} | Read body transforms into a reusable positions map |
| Flecs | draw_rectangles_snapshot/{40,256,1024} | Extract rectangle render data into a reusable vector |

The host API benchmark pre-allocates the JS entity ID array and Float32Array; those allocations are not charged to each measured frame. Frame-state Rust buffers are handed to the host by swapping ownership and are restored after the synchronous QuickJS update. The physics and Flecs snapshot benchmarks likewise reuse output buffers. The Box2D world contains active bodies; this is not an empty-world benchmark.

## Results

Times below are representative Criterion point estimates in microseconds per operation. The change column is Criterion's relative comparison estimate with its 95% confidence interval and significance assessment. It is not calculated by dividing the rounded times in the first two columns, because Criterion compares the sampled distributions.

| Subsystem / case | Before | After | Criterion change |
|---|---:|---:|---:|
| QuickJS context creation + source evaluation | 178.61 µs | 172.46 µs | −2.12% (95% CI −6.10% to +2.01%, p = 0.33; no change detected) |
| Empty cached update hook | 4.319 µs | 0.155 µs | **−96.48%** (95% CI −96.61% to −96.36%) |
| Host API, 16 transforms | 17.03 µs | 10.81 µs | **−42.04%** (95% CI −50.78% to −34.42%) |
| Host API, 64 transforms | 39.41 µs | 27.65 µs | **−32.36%** (95% CI −37.01% to −27.69%) |
| Host API, 256 transforms | 122.82 µs | 100.72 µs | **−22.50%** (95% CI −28.90% to −16.42%) |
| Brick Breaker TypeScript update | 6.338 µs | 0.756 µs | **−87.38%** (95% CI −88.30% to −86.47%) |
| Box2D fixed step, 40 bodies | 5.635 µs | 5.198 µs | **−10.20%** (95% CI −15.40% to −4.82%) |
| Box2D fixed step, 256 bodies | 24.37 µs | 22.36 µs | **−10.81%** (95% CI −15.97% to −5.55%) |
| Box2D fixed step, 1024 bodies | 94.85 µs | 89.81 µs | **−7.06%** (95% CI −10.62% to −3.46%; isolated repeat) |
| Box2D positions, 40 bodies | 3.413 µs | 3.049 µs | **−7.44%** (95% CI −11.31% to −3.48%) |
| Box2D positions, 256 bodies | 21.28 µs | 22.84 µs | +4.26% (95% CI −2.21% to +13.83%, p = 0.33; no change detected) |
| Box2D positions, 1024 bodies | 90.31 µs | 86.83 µs | −5.28% (Criterion classified this result as within its noise threshold) |
| Flecs rectangles, 40 entities | 1.012 µs | 0.927 µs | **−12.42%** (95% CI −16.57% to −8.13%) |
| Flecs rectangles, 256 entities | 1.477 µs | 1.134 µs | **−25.42%** (95% CI −32.47% to −18.16%) |
| Flecs rectangles, 1024 entities | 2.919 µs | 2.036 µs | **−25.51%** (95% CI −31.15% to −20.43%) |

Bold reductions were statistically significant at the 0.05 level in the Criterion comparison. The 1024-body Box2D step showed high variance in one full-suite run (118 µs); an isolated repeat returned 89.8 µs and showed a significant reduction versus baseline. Treat that large-scene result as more sensitive to host load than the other cases and repeat it on an otherwise idle system before using it as a strict regression gate.

## Interpretation

### Confirmed improvements

- **QuickJS hook invocation:** caching the game object and lifecycle functions as persistent QuickJS values removes source-string formatting and eval() from every lifecycle invocation. The empty update microbenchmark fell by about 96%.
- **Actual sample gameplay update:** Brick Breaker update fell by about 87%. This is the most relevant result for the current game workload.
- **Host boundary:** update_frame() temporarily swaps Rust frame buffers into host state instead of cloning the input sets, positions map and collision vector. The script reads many transforms with one readPositions(ids, Float32Array) call and one host-state lock. The measured synthetic workload improved by 22–42%.
- **Flecs extraction:** reusing output vectors reduced time by roughly 12–26% at 40–1024 rectangles.
- **Box2D contact processing:** reusing contact-pair and unique-entity scratch collections improved measured fixed-step cases by roughly 7–11%. Position extraction remains dominated by per-body Box2D transform access and map insertion.

### Limits and trade-offs

- **Startup cost:** context creation plus source evaluation showed no statistically significant change. Persistent lifecycle references do not provide a demonstrated startup improvement; their benefit is steady-state execution.
- **Large transform snapshots did not materially improve:** retaining the HashMap allocation does not remove per-body Box2D transform access and map insertion. At 256 bodies no significant improvement was detected; at 1024 bodies Criterion classified the reduction as within its noise threshold. The next useful investigation is a direct contiguous transform extraction path or a packed transform buffer, if the Box2D API permits it without unsafe lifetime assumptions.
- **No FPS claim:** these are CPU microbenchmarks. They do not measure SDL event latency, complete frame scheduling, GPU command submission, present/vsync wait, GPU upload time or end-to-end frame pacing. The reusable wgpu vertex buffer is compile/test checked but has not yet been measured with GPU timestamps or an interactive frame profiler.
- **No bytecode comparison:** JavaScript source evaluation remains the current load path. qjsc bytecode loading is not included in this optimization round; it would primarily target script packaging and load-time behavior, not presumed frame execution speed.

## Implemented changes covered by this round

- Cache the JS game object, engine object and initialize / update / shutdown functions using rquickjs::Persistent.
- Call cached hooks directly with the original this binding and engine argument.
- Add engine.entities.readPositions(ids, outputFloat32Array) for batched transform access.
- Add update_frame(), which swaps frame buffers into the host for the duration of the synchronous script update and restores them on both success and script-error paths.
- Reuse command vectors, physics position maps, collision scratch collections and ECS render-data vectors.
- Reuse text-label String capacity when refreshing ECS text render data.
- Retain the renderer's CPU vertex vector and grow a reusable GPU vertex buffer instead of recreating it every frame.

## Next profiling targets

1. Add whole-frame phase timings around input/event polling, host-state handoff, QuickJS update, command application, Box2D fixed steps, ECS synchronization/extraction, GPU buffer upload, render pass submission and presentation.
2. Add an interactive fixed-scene run with a stable number of entities and collect median / p95 / p99 frame time. Compare with vsync disabled for CPU-side profiling and enabled for pacing validation.
3. Investigate contiguous transform storage / extraction for larger Box2D worlds; do not optimize this path based on allocation reuse alone.
4. Measure the retained wgpu vertex buffer with GPU timestamps or a platform GPU profiler before attributing any frame-time gain to it.
5. Evaluate qjsc as a separate script-load/packaging experiment, not as a presumed FPS optimization.