# Runtime performance benchmarks

This document records the CPU-side performance comparison for the native 2D runtime. It identifies promising optimization areas; it does not claim a particular game FPS.

## Reproduction

Run from rust-brick-breaker/ on the same machine, using the same power profile and without competing CPU-heavy workloads:

    cargo bench --bench runtime --locked -- --noplot

The before/after table below was generated against the pre-optimization implementation during this development session. Criterion's raw samples and baseline directory live under target/criterion/ and are build output, so they are not committed. A fresh checkout cannot recreate that historical baseline from this branch alone; retain the original local target/criterion/before directory when you need the exact historical comparison.

For subsequent optimization rounds, create a new local baseline before changing the measured implementation, then compare against that named baseline after the change:

    cargo bench --bench runtime --locked -- --save-baseline round-1
    # implement the next optimization without deleting target/criterion
    cargo bench --bench runtime --locked -- --baseline round-1 --noplot

Do not overwrite the historical before baseline. Use the same host, power mode, build profile, sample settings and workload for both sides. The suite uses Criterion 0.8.2 and Criterion's optimized benchmark profile.

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
| Box2D | positions_snapshot/{40,256,1024} | Copy cached transforms into a reusable positions map |
| Box2D | positions_cache_borrow/{40,256,1024} | Borrow the current transform cache without traversing it |
| Box2D + Flecs | box2d_frame_sync/step_and_sync | Step physics and synchronize only changed transforms in sparse and dense scenes |
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
- **Round 1 transform snapshots did not materially improve:** retaining a HashMap allocation did not remove per-body Box2D transform access and map insertion. Round 2 replaces that scan with Box2D body-move events and a persistent transform cache; see the second-round results below.
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

## Round 2 — Box2D body-move cache and adaptive Flecs synchronization

Round 2 uses the Box2D 3 CompletedStep body_events stream. Every registered body has an initial transform cached in PhysicsWorld; explicit set_transform updates it immediately, and each Box2D body-move event updates only that body's cache entry and emits a dirty transform record. The event stream includes the final transform when a body goes to sleep. A reverse BodyId to entity_id map connects Box2D events to runtime ECS identity.

The main loop now:
1. Borrows the cached transform map for the synchronous QuickJS update, avoiding a full transform query.
2. Advances Box2D fixed steps and applies body-move events directly to the transform cache.
3. Drains dirty transform records and selects incremental or full-query Flecs synchronization based on the number of unique moved entities.
4. Keeps the previous full positions() / positions_into() and full ECS synchronization APIs for compatibility.

The implementation intentionally does not copy Box2D move events into an intermediate scratch vector and does not coalesce them in PhysicsWorld. For adaptive path selection, EcsWorld reuses a HashSet only when the raw event count could reach the dense threshold; it counts unique entity IDs and stops as soon as half the physics bodies are confirmed moved. This avoids treating repeated events from multiple fixed substeps as distinct bodies.

### End-to-end Box2D to Flecs results

The box2d_frame_sync/step_and_sync benchmark measures the same semantic work before and after. The current path uses incremental entity updates below 50% unique moved bodies and a direct Flecs query at or above that threshold:
- Before: Box2D fixed step + full transform snapshot of all bodies + full Flecs physics synchronization.
- After: Box2D fixed step + transform-cache update from body-move events + adaptive incremental/direct-query Flecs synchronization.

All five scenarios were measured with Criterion on the same Windows / Ryzen 5 5600H host, 1 second warm-up, 2 seconds measurement, 30 samples. The round-1 baseline was captured before changing the physics runtime. Times are point estimates; the change column and confidence intervals come from Criterion's sample comparison.

| Scenario | Before | After | Criterion change |
|---|---:|---:|---:|
| 40 bodies, 2 moving | 9.051 \u00b5s | 3.873 \u00b5s | **\u221258.01%** (95% CI \u221259.41% to \u221256.60%) |
| 256 bodies, 2 moving | 32.037 \u00b5s | 2.611 \u00b5s | **\u221292.10%** (95% CI \u221292.29% to \u221291.90%) |
| 1024 bodies, 2 moving | 180.564 \u00b5s | 2.170 \u00b5s | **\u221298.73%** (95% CI \u221298.80% to \u221298.67%) |
| 256 bodies, 256 moving | 70.108 \u00b5s | 66.175 \u00b5s | \u22123.01% (95% CI \u22126.34% to +0.34%, p = 0.09; no change detected) |
| 1024 bodies, 1024 moving | 346.857 \u00b5s | 302.922 \u00b5s | **\u221211.97%** (95% CI \u221215.55% to \u22128.27%) |

The large sparse-scene gains are the important result for Brick Breaker: the workload has a small moving set (ball and paddle) among many static bricks. With adaptive synchronization, the dense 256-body case is statistically unchanged, while the dense 1024-body case improves by about 12%. The dense path performs a direct Flecs query against the already-populated transform cache; it does not invoke the legacy per-entity Mutex-backed system.

### Transform cache microbenchmarks

positions_snapshot still returns a complete map, but copies from the persistent cache rather than querying every Box2D body through the FFI. positions_cache_borrow only measures returning a borrowed reference; it does not traverse the map and must not be interpreted as the cost of reading every transform.

| Case | Before | After | Criterion change |
|---|---:|---:|---:|
| Snapshot, 40 bodies | 3.167 µs | 0.558 µs | **−82.19%** (95% CI −82.70% to −81.66%) |
| Snapshot, 256 bodies | 20.442 µs | 3.574 µs | **−82.63%** (95% CI −83.00% to −82.26%) |
| Snapshot, 1024 bodies | 89.093 µs | 15.385 µs | **−83.11%** (95% CI −83.60% to −82.46%) |
| Borrow cache, 40 / 256 / 1024 | — | about 0.52 ns | Separate access-cost benchmark; no historical pre-cache equivalent |

The isolated fixed_step diagnostic includes additional work in the new implementation: body-move event enumeration, transform-cache updates and dirty-record publication. It is not an apples-to-apples measurement of the Box2D solver alone. The diagnostic result is:

| Bodies | Before | After | Criterion change |
|---|---:|---:|---:|
| 40 | 5.213 µs | 8.716 µs | +59.72% (95% CI +51.45% to +67.28%) |
| 256 | 22.673 µs | 48.683 µs | +111.22% (95% CI +99.81% to +124.18%) |
| 1024 | 90.856 µs | 199.453 µs | +121.05% (95% CI +113.12% to +128.33%) |

This increase is the cost of maintaining a complete transform cache from the move-event stream. The end-to-end step_and_sync comparison includes the old full-snapshot/full-ECS work and is the correct measure of runtime impact. Sparse scenes improve dramatically; fully moving scenes are neutral to modestly slower, depending on size.

### Correctness coverage

New tests cover:
- Moving dynamic and kinematic bodies update the cache and emit dirty events while static bodies remain cached without move events.
- The final body-move event preserves the transform when a body goes to sleep.
- Explicit set_transform updates the cache immediately.
- Destroying a body removes its cache entry and queued transform updates.
- The full snapshot compatibility API reads from the cache and preserves caller-owned map capacity.
- Flecs applies sparse changed transforms directly, switches to a full query at 50% unique moved entities, and safely ignores already-destroyed entities on the incremental path.
- Existing contact, lifecycle, Brick Breaker and zero-copy scripting tests remain in the full suite.
The adaptive-path regression test covers repeated move records for one entity, the sparse update path, the 50% unique-entity threshold, and the empty-change fast path. Benchmark scenarios use one fixed step per measured iteration; the unit test separately verifies that duplicate records do not inflate the unique-entity count.

## Next profiling targets

1. Add whole-frame phase timings around input/event polling, host-state handoff, QuickJS update, command application, Box2D fixed steps, ECS synchronization/extraction, GPU buffer upload, render pass submission and presentation.
2. Add an interactive fixed-scene run with a stable number of entities and collect median / p95 / p99 frame time. Compare with vsync disabled for CPU-side profiling and enabled for pacing validation.
3. Investigate contiguous transform storage / extraction for larger Box2D worlds; do not optimize this path based on allocation reuse alone.
4. Measure the retained wgpu vertex buffer with GPU timestamps or a platform GPU profiler before attributing any frame-time gain to it.
5. Evaluate qjsc as a separate script-load/packaging experiment, not as a presumed FPS optimization.
