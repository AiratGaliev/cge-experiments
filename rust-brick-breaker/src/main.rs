use std::{
    collections::HashSet,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use sdl3::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
    video::Window,
};

use native_brick_breaker::{
    audio::Audio,
    ecs::EcsWorld,
    physics::{PhysicsPositions, PhysicsWorld},
    render::Renderer,
    scripting::{RuntimeCommand, ScriptEngine},
};

const WINDOW_WIDTH: u32 = 960;
const WINDOW_HEIGHT: u32 = 720;
const FIXED_STEP: f32 = 1.0 / 60.0;
const MAX_FRAME_DELTA: f32 = 0.10;
const MAX_STEPS_PER_FRAME: usize = 6;

#[derive(Default)]
struct InputState {
    down: HashSet<String>,
    pressed: HashSet<String>,
}

impl InputState {
    fn key_down(&mut self, key: &str) {
        if self.down.insert(key.to_owned()) {
            self.pressed.insert(key.to_owned());
        }
    }

    fn key_up(&mut self, key: &str) {
        self.down.remove(key);
    }

    fn clear(&mut self) {
        self.down.clear();
        self.pressed.clear();
    }

    fn finish_frame(&mut self) {
        self.pressed.clear();
    }
}

fn main() -> Result<()> {
    let script_path = std::env::var_os("CGE_GAME_SCRIPT")
        .map(PathBuf::from)
        .or_else(|| std::env::args_os().nth(1).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("assets/game.js"));

    let sdl = sdl3::init().context("initialize SDL3")?;
    let video = sdl.video().context("initialize SDL3 video subsystem")?;
    let window = video
        .window("Codogma Native 2D Runtime", WINDOW_WIDTH, WINDOW_HEIGHT)
        .position_centered()
        .resizable()
        .build()
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .context("create SDL3 window")?;

    run_runtime(sdl, window, &script_path)
}

fn run_runtime(sdl: sdl3::Sdl, window: Window, script_path: &std::path::Path) -> Result<()> {
    let mut renderer =
        pollster::block_on(Renderer::new(&window)).context("initialize wgpu renderer")?;
    let mut event_pump = sdl.event_pump().context("create SDL3 event pump")?;

    let audio = Audio::new();
    let mut physics = PhysicsWorld::new([0.0, 0.0]).context("initialize Box2D v3 world")?;
    let mut scene = EcsWorld::new();
    let script = ScriptEngine::new(script_path, audio.sample_queue())
        .context("load and initialize embedded QuickJS-NG game script")?;

    script.initialize().context("initialize TypeScript game")?;
    let mut commands = Vec::new();
    script.drain_commands_into(&mut commands);
    apply_commands(&mut commands, &mut scene, &mut physics)?;

    let mut rectangles = Vec::new();
    let mut circles = Vec::new();
    let mut labels = Vec::new();
    scene.draw_rectangles_into(&mut rectangles);
    scene.draw_circles_into(&mut circles);
    scene.draw_text_into(&mut labels);
    let mut positions = PhysicsPositions::new();
    physics.positions_into(&mut positions)?;
    println!("GPU adapter: {}", renderer.adapter_name());
    println!("Game script: {}", script_path.display());
    println!(
        "Initial scene: {} ECS entities, {} rectangles, {} circles, {} text labels, {} physics bodies",
        scene.entity_count(),
        rectangles.len(),
        circles.len(),
        labels.len(),
        positions.len()
    );
    println!("Native runtime initialized. Press Escape to quit.");

    let mut input = InputState::default();
    let mut last_frame = Instant::now();
    let mut accumulator = 0.0_f32;
    let mut pending_collisions = Vec::new();

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::Window {
                    win_event: WindowEvent::CloseRequested,
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(key),
                    repeat: false,
                    ..
                } => {
                    if let Some(name) = key_name(key) {
                        input.key_down(name);
                    }
                }
                Event::KeyUp {
                    keycode: Some(key), ..
                } => {
                    if let Some(name) = key_name(key) {
                        input.key_up(name);
                    }
                }
                Event::Window {
                    win_event: WindowEvent::Resized(..) | WindowEvent::PixelSizeChanged(..),
                    ..
                } => {
                    let (width, height) = window.size_in_pixels();
                    renderer.resize(width, height);
                }
                Event::Window {
                    win_event: WindowEvent::FocusLost,
                    ..
                } => input.clear(),
                _ => {}
            }
        }

        let now = Instant::now();
        let frame_delta = now.duration_since(last_frame).as_secs_f32();
        last_frame = now;
        let frame_delta = frame_delta.min(MAX_FRAME_DELTA);
        accumulator = (accumulator + frame_delta).min(FIXED_STEP * MAX_STEPS_PER_FRAME as f32);

        physics
            .positions_into(&mut positions)
            .context("read native physics transforms before game update")?;
        script
            .update_frame(
                frame_delta,
                &mut input.down,
                &mut input.pressed,
                &mut positions,
                &mut pending_collisions,
            )
            .context("execute TypeScript game update")?;
        pending_collisions.clear();

        script.drain_commands_into(&mut commands);
        apply_commands(&mut commands, &mut scene, &mut physics)?;
        scene.apply_pending_changes();

        let mut step_count = 0;
        while accumulator >= FIXED_STEP && step_count < MAX_STEPS_PER_FRAME {
            physics
                .step_into(FIXED_STEP, &mut pending_collisions)
                .context("advance fixed-step Box2D simulation")?;
            accumulator -= FIXED_STEP;
            step_count += 1;
        }

        physics
            .positions_into(&mut positions)
            .context("read Box2D transforms for Flecs synchronization")?;
        scene.synchronize_physics(&positions);

        scene.draw_rectangles_into(&mut rectangles);
        scene.draw_circles_into(&mut circles);
        scene.draw_text_into(&mut labels);
        renderer
            .render(&rectangles, &circles, &labels)
            .context("render native ECS scene")?;

        input.finish_frame();
        if step_count == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    script.shutdown().context("shutdown TypeScript game")?;
    drop(renderer);
    drop(window);
    drop(audio);
    Ok(())
}

fn apply_commands(
    commands: &mut Vec<RuntimeCommand>,
    scene: &mut EcsWorld,
    physics: &mut PhysicsWorld,
) -> Result<()> {
    for command in commands.drain(..) {
        match command {
            RuntimeCommand::CreateRectangle {
                entity_id,
                x,
                y,
                width,
                height,
                color,
            } => scene.create_rectangle(entity_id, x, y, width, height, color)?,
            RuntimeCommand::CreateCircle {
                entity_id,
                x,
                y,
                radius,
                color,
            } => scene.create_circle(entity_id, x, y, radius, color)?,
            RuntimeCommand::CreateText {
                entity_id,
                value,
                x,
                y,
                scale,
                color,
            } => scene.create_text(entity_id, value, x, y, scale, color)?,
            RuntimeCommand::SetPosition { entity_id, x, y } => {
                if physics.has_body(entity_id) {
                    physics.set_transform(entity_id, [x, y])?;
                } else {
                    scene.set_position(entity_id, x, y)?;
                }
            }
            RuntimeCommand::SetText { entity_id, value } => {
                scene.set_text(entity_id, value)?;
            }
            RuntimeCommand::DestroyEntity { entity_id } => {
                if physics.has_body(entity_id) {
                    physics.destroy_body(entity_id)?;
                }
                scene.destroy_entity(entity_id)?;
            }
            RuntimeCommand::CreateBoxBody {
                entity_id,
                position,
                size,
                body_type,
                material,
            } => {
                physics.create_box(entity_id, position, size, body_type, material)?;
                scene.attach_physics(entity_id)?;
            }
            RuntimeCommand::CreateCircleBody {
                entity_id,
                position,
                radius,
                body_type,
                material,
            } => {
                physics.create_circle(entity_id, position, radius, body_type, material)?;
                scene.attach_physics(entity_id)?;
            }
            RuntimeCommand::SetLinearVelocity {
                entity_id,
                velocity,
            } => physics.set_linear_velocity(entity_id, velocity)?,
            RuntimeCommand::SetBodyPosition {
                entity_id,
                position,
            } => physics.set_transform(entity_id, position)?,
            RuntimeCommand::DestroyBody { entity_id } => physics.destroy_body(entity_id)?,
        }
    }
    Ok(())
}

fn key_name(key: Keycode) -> Option<&'static str> {
    match key {
        Keycode::A => Some("A"),
        Keycode::D => Some("D"),
        Keycode::W => Some("W"),
        Keycode::S => Some("S"),
        Keycode::Left => Some("ArrowLeft"),
        Keycode::Right => Some("ArrowRight"),
        Keycode::Up => Some("ArrowUp"),
        Keycode::Down => Some("ArrowDown"),
        Keycode::Space => Some("Space"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_state_reports_down_and_just_pressed_separately() {
        let mut input = InputState::default();
        input.key_down("Space");
        input.key_down("Space");
        assert!(input.down.contains("Space"));
        assert_eq!(input.pressed.len(), 1);

        input.finish_frame();
        assert!(input.pressed.is_empty());
        assert!(input.down.contains("Space"));
        input.key_up("Space");
        assert!(!input.down.contains("Space"));
    }

    #[test]
    fn focus_loss_clears_all_held_keys() {
        let mut input = InputState::default();
        input.key_down("A");
        input.key_down("ArrowLeft");
        input.clear();

        assert!(input.down.is_empty());
        assert!(input.pressed.is_empty());
    }

    #[test]
    fn key_mapping_is_platform_neutral() {
        assert_eq!(key_name(Keycode::Left), Some("ArrowLeft"));
        assert_eq!(key_name(Keycode::Space), Some("Space"));
        assert_eq!(key_name(Keycode::A), Some("A"));
    }

    #[test]
    fn destroying_a_runtime_entity_also_destroys_its_physics_body() {
        use native_brick_breaker::physics::{PhysicsMaterial, RigidBodyKind};

        let mut scene = EcsWorld::new();
        scene
            .create_rectangle(1, 0.0, 0.0, 1.0, 1.0, [1.0; 4])
            .unwrap();
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_box(
                1,
                [0.0, 0.0],
                [1.0, 1.0],
                RigidBodyKind::Dynamic,
                PhysicsMaterial {
                    density: 1.0,
                    restitution: 0.0,
                    ..PhysicsMaterial::default()
                },
            )
            .unwrap();
        scene.attach_physics(1).unwrap();

        apply_commands(
            &mut vec![RuntimeCommand::DestroyEntity { entity_id: 1 }],
            &mut scene,
            &mut physics,
        )
        .unwrap();

        assert!(!physics.has_body(1));
        assert_eq!(scene.entity_count(), 0);
        assert!(scene.draw_rectangles().is_empty());
    }
}
