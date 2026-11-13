use std::{
    collections::{HashMap, HashSet},
    hint::black_box,
    sync::Arc,
    time::Duration,
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use crossbeam_queue::ArrayQueue;
use native_brick_breaker::{
    ecs::EcsWorld,
    input::GamepadState,
    physics::{PhysicsMaterial, PhysicsWorld, RigidBodyKind},
    scripting::ScriptEngine,
};

const FIXED_STEP: f32 = 1.0 / 60.0;
const BENCH_AUDIO_CAPACITY: usize = 4096;
const COLOR: [f32; 4] = [0.8, 0.7, 0.4, 1.0];

fn audio_queue() -> Arc<ArrayQueue<f32>> {
    Arc::new(ArrayQueue::new(BENCH_AUDIO_CAPACITY))
}

fn bench_script(c: &mut Criterion) {
    let mut group = c.benchmark_group("quickjs");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(30);

    let queue = audio_queue();
    let source = r#"
        globalThis.game = {
            frames: 0,
            initialize() {},
            update() { this.frames += 1; },
            shutdown() {}
        };
    "#;
    group.bench_function("context_create_and_evaluate", |b| {
        b.iter(|| {
            black_box(ScriptEngine::from_source(black_box(source), Arc::clone(&queue)).unwrap())
        });
    });

    let script = ScriptEngine::from_source(source, Arc::clone(&queue)).unwrap();
    script.initialize().unwrap();
    group.bench_function("noop_hook_update", |b| {
        b.iter(|| script.update().unwrap());
    });
    drop(group);

    let mut group = c.benchmark_group("quickjs_host_api");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(30);

    for positions_count in [16_u32, 64, 256] {
        let source = format!(
            r#"
                globalThis.game = {{
                    ids: Array.from({{ length: {positions_count} }}, (_, index) => index + 1),
                    positions: new Float32Array({positions_count} * 2),
                    initialize() {{}},
                    update(engine) {{
                        engine.entities.readPositions(this.ids, this.positions);
                        let sum = 0;
                        for (let index = 0; index < this.positions.length; index += 1) {{
                            sum += this.positions[index];
                        }}
                        for (let index = 0; index < 8; index += 1) {{
                            if (engine.input.isDown("A")) sum += 1;
                        }}
                        this.sum = sum + engine.time.deltaSeconds();
                    }},
                    shutdown() {{}}
                }};
            "#
        );
        let script = ScriptEngine::from_source(&source, audio_queue()).unwrap();
        script.initialize().unwrap();
        let mut positions = (1..=positions_count)
            .map(|id| (id, [id as f32 * 0.5, id as f32 * -0.25]))
            .collect::<HashMap<_, _>>();
        let mut keys_down = HashSet::from([String::from("A")]);
        let mut keys_pressed = HashSet::new();
        let mut collisions = Vec::new();

        group.throughput(Throughput::Elements(u64::from(positions_count)));
        group.bench_with_input(
            BenchmarkId::new("frame_submit_and_update", positions_count),
            &positions_count,
            |b, _| {
                b.iter(|| {
                    script
                        .update_frame(
                            FIXED_STEP,
                            &mut keys_down,
                            &mut keys_pressed,
                            GamepadState::default(),
                            &mut positions,
                            &mut collisions,
                        )
                        .unwrap();
                    black_box(script.drain_commands());
                });
            },
        );
    }
    drop(group);

    let mut group = c.benchmark_group("gameplay_typescript");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(30);
    let game_source = include_str!("../assets/game.js");
    let game = ScriptEngine::from_source(game_source, audio_queue()).unwrap();
    game.initialize().unwrap();
    black_box(game.drain_commands());
    let mut positions = (1..=48_u32)
        .map(|id| (id, [id as f32 * 0.01, id as f32 * -0.02]))
        .collect::<HashMap<_, _>>();
    let mut keys_down = HashSet::new();
    let mut keys_pressed = HashSet::new();
    let mut collisions = Vec::new();
    group.bench_function("brick_breaker_update", |b| {
        b.iter(|| {
            game.update_frame(
                FIXED_STEP,
                &mut keys_down,
                &mut keys_pressed,
                GamepadState::default(),
                &mut positions,
                &mut collisions,
            )
            .unwrap();
            black_box(game.drain_commands());
        });
    });
}

fn make_physics_world(body_count: u32) -> (EcsWorld, PhysicsWorld) {
    let mut scene = EcsWorld::new();
    let mut world = PhysicsWorld::new([0.0, 0.0]).unwrap();
    world.configure_world(scene.world_mut());
    let material = PhysicsMaterial {
        density: 1.0,
        friction: 0.0,
        restitution: 0.0,
        gravity_scale: 0.0,
        bullet: false,
    };
    for entity_id in 1..=body_count {
        world
            .create_circle(
                scene.world_mut(),
                entity_id,
                [entity_id as f32 * 0.5, 0.0],
                0.1,
                RigidBodyKind::Dynamic,
                material,
            )
            .unwrap();
        world
            .set_linear_velocity(scene.world_mut(), entity_id, [0.1, (entity_id % 5) as f32 * 0.02 - 0.04])
            .unwrap();
    }
    (scene, world)
}

fn bench_physics(c: &mut Criterion) {
    let mut group = c.benchmark_group("avian2d");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(30);

    for body_count in [40_u32, 256, 1024] {
        let (mut scene, mut world) = make_physics_world(body_count);
        let mut collisions = Vec::new();
        let mut position_changes = Vec::new();
        group.throughput(Throughput::Elements(u64::from(body_count)));
        group.bench_with_input(
            BenchmarkId::new("fixed_step", body_count),
            &body_count,
            |b, _| {
                b.iter(|| {
                    collisions.clear();
                    world.step_into(scene.world_mut(), FIXED_STEP, &mut collisions).unwrap();
                    world.drain_position_changes_into(&mut position_changes);
                    black_box(&collisions);
                    black_box(&position_changes);
                })
            },
        );
        let mut positions = HashMap::with_capacity(body_count as usize);
        group.bench_with_input(
            BenchmarkId::new("positions_snapshot", body_count),
            &body_count,
            |b, _| {
                b.iter(|| {
                    world.positions_into(&mut positions);
                    black_box(&positions);
                })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("positions_cache_borrow", body_count),
            &body_count,
            |b, _| b.iter(|| black_box(world.positions_ref())),
        );
    }
}

fn make_ecs_world(entity_count: u32) -> EcsWorld {
    let mut world = EcsWorld::new();
    for entity_id in 1..=entity_count {
        let x = (entity_id % 32) as f32 * 0.25;
        let y = (entity_id / 32) as f32 * 0.25;
        world
            .create_rectangle(entity_id, x, y, 0.2, 0.2, COLOR)
            .unwrap();
    }
    world
}

fn bench_ecs(c: &mut Criterion) {
    let mut group = c.benchmark_group("bevy_ecs_render_data");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(30);

    for entity_count in [40_u32, 256, 1024] {
        let mut world = make_ecs_world(entity_count);
        let mut rectangles = Vec::with_capacity(entity_count as usize);
        group.throughput(Throughput::Elements(u64::from(entity_count)));
        group.bench_with_input(
            BenchmarkId::new("draw_rectangles_snapshot", entity_count),
            &entity_count,
            |b, _| {
                b.iter(|| {
                    world.draw_rectangles_into(&mut rectangles);
                    black_box(&rectangles);
                })
            },
        );
    }
}

fn make_physics_scene(body_count: u32, moving_body_count: u32) -> (PhysicsWorld, EcsWorld) {
    let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
    let mut scene = EcsWorld::new();
    physics.configure_world(scene.world_mut());
    let material = PhysicsMaterial {
        density: 1.0,
        friction: 0.0,
        restitution: 0.0,
        gravity_scale: 0.0,
        bullet: false,
    };

    for entity_id in 1..=body_count {
        let grid_index = entity_id - 1;
        let position = [
            (grid_index % 32) as f32 * 1.5,
            (grid_index / 32) as f32 * 1.5,
        ];
        scene
            .create_rectangle(entity_id, position[0], position[1], 0.2, 0.2, COLOR)
            .unwrap();
        scene.attach_physics(entity_id).unwrap();

        physics
            .create_circle(scene.world_mut(), entity_id, position, 0.1, RigidBodyKind::Dynamic, material)
            .unwrap();
        if entity_id <= moving_body_count {
            physics.set_linear_velocity(scene.world_mut(), entity_id, [0.4, 0.2]).unwrap();
        }
    }

    (physics, scene)
}

fn bench_physics_frame_sync(c: &mut Criterion) {
    let mut group = c.benchmark_group("avian2d_frame_sync");
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(30);

    for (body_count, moving_body_count) in [
        (40_u32, 2_u32),
        (256, 2),
        (1024, 2),
        (256, 256),
        (1024, 1024),
    ] {
        let (mut physics, mut scene) = make_physics_scene(body_count, moving_body_count);
        let mut collisions = Vec::new();
        let mut position_changes = Vec::new();
        let id = format!("{body_count}_bodies_{moving_body_count}_moving");

        group.throughput(Throughput::Elements(u64::from(body_count)));
        group.bench_function(BenchmarkId::new("step_and_sync", id), |b| {
            b.iter(|| {
                collisions.clear();
                physics.step_into(scene.world_mut(), FIXED_STEP, &mut collisions).unwrap();
                physics.drain_position_changes_into(&mut position_changes);
                scene
                    .synchronize_physics_adaptive(&position_changes, physics.positions_ref())
                    .unwrap();
                black_box(scene.entity_count());
                black_box(&position_changes);
            });
        });
    }
}
criterion_group!(
    runtime_benches,
    bench_script,
    bench_physics,
    bench_ecs,
    bench_physics_frame_sync
);
criterion_main!(runtime_benches);
