mod audio;
mod ecs;
mod physics;
mod render;
mod scripting;

use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use sdl3::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
    video::Window,
};

use audio::Audio;
use ecs::EcsScene;
use physics::Physics;
use render::Renderer;
use scripting::{GameStatus, ScriptEngine};

const WINDOW_WIDTH: u32 = 960;
const WINDOW_HEIGHT: u32 = 720;
const FIXED_STEP: f32 = 1.0 / 60.0;
const MAX_FRAME_DELTA: f32 = 0.10;
const MAX_STEPS_PER_FRAME: usize = 6;

#[derive(Default)]
struct InputState {
    left: bool,
    right: bool,
    launch_pressed: bool,
}

impl InputState {
    fn horizontal(&self) -> f32 {
        (self.right as i8 - self.left as i8) as f32
    }

    fn take_launch(&mut self) -> bool {
        std::mem::take(&mut self.launch_pressed)
    }
}

fn main() -> Result<()> {
    let sdl = sdl3::init().context("initialize SDL3")?;
    let video = sdl.video().context("initialize SDL3 video subsystem")?;
    let window = video
        .window(
            "CGE Experiments — Native Brick Breaker",
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
        )
        .position_centered()
        .resizable()
        .build()
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .context("create SDL3 window")?;

    run_game(sdl, window)
}

fn run_game(sdl: sdl3::Sdl, window: Window) -> Result<()> {
    let mut renderer =
        pollster::block_on(Renderer::new(&window)).context("initialize wgpu renderer")?;
    let mut event_pump = sdl.event_pump().context("create SDL3 event pump")?;

    let audio = Audio::new();
    let scripting = ScriptEngine::new(audio.sample_queue())
        .context("initialize embedded QuickJS-NG and load compiled TypeScript")?;
    let mut physics = Physics::new().context("initialize Box2D v3 world")?;
    let initial_positions = physics.positions()?;
    let scene = EcsScene::new(&initial_positions).context("initialize Flecs ECS world")?;

    println!("GPU adapter: {}", renderer.adapter_name());
    println!("Controls: A/D or Left/Right to move, Space to launch/restart, Escape to quit.");

    let mut input = InputState::default();
    let mut last_frame = Instant::now();
    let mut accumulator = 0.0_f32;
    let mut snapshot = scripting.snapshot()?;

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
                    keycode: Some(Keycode::A | Keycode::Left),
                    repeat: false,
                    ..
                } => input.left = true,
                Event::KeyUp {
                    keycode: Some(Keycode::A | Keycode::Left),
                    ..
                } => input.left = false,
                Event::KeyDown {
                    keycode: Some(Keycode::D | Keycode::Right),
                    repeat: false,
                    ..
                } => input.right = true,
                Event::KeyUp {
                    keycode: Some(Keycode::D | Keycode::Right),
                    ..
                } => input.right = false,
                Event::KeyDown {
                    keycode: Some(Keycode::Space),
                    repeat: false,
                    ..
                } => input.launch_pressed = true,
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
                } => {
                    input.left = false;
                    input.right = false;
                }
                _ => {}
            }
        }

        let now = Instant::now();
        let frame_delta = now.duration_since(last_frame).as_secs_f32();
        last_frame = now;
        let frame_delta = frame_delta.min(MAX_FRAME_DELTA);
        accumulator = (accumulator + frame_delta).min(FIXED_STEP * MAX_STEPS_PER_FRAME as f32);

        if input.take_launch() {
            match snapshot.status {
                GameStatus::Ready => {
                    snapshot = scripting.launch()?;
                    physics.launch()?;
                }
                GameStatus::Victory | GameStatus::GameOver => {
                    let _ = scripting.restart()?;
                    physics.reset()?;
                    scene.reset_paddle();
                    scene.reset_bricks();
                    snapshot = scripting.launch()?;
                    physics.launch()?;
                }
                GameStatus::Playing => {}
            }
        }

        let horizontal = input.horizontal();
        scene.progress(horizontal, frame_delta);

        let mut step_count = 0;
        while accumulator >= FIXED_STEP && step_count < MAX_STEPS_PER_FRAME {
            physics.set_paddle_target(scene.paddle_x(), FIXED_STEP)?;
            if snapshot.status != GameStatus::Playing {
                physics.park_ball()?;
            }

            let step = physics.step(FIXED_STEP)?;
            if snapshot.status == GameStatus::Playing {
                if step.bounced {
                    snapshot = scripting.bounced()?;
                }

                if !step.bricks_destroyed.is_empty() {
                    scene.mark_bricks_destroyed(&step.bricks_destroyed);
                    let bricks_before_step = step.remaining_bricks + step.bricks_destroyed.len();
                    for destroyed_count in 0..step.bricks_destroyed.len() {
                        let remaining = bricks_before_step.saturating_sub(destroyed_count + 1);
                        snapshot = scripting.brick_destroyed(100, remaining)?;
                    }
                }

                if step.ball_lost {
                    snapshot = scripting.ball_lost()?;
                    physics.park_ball()?;
                }
                if matches!(snapshot.status, GameStatus::Victory | GameStatus::GameOver) {
                    physics.park_ball()?;
                }
            }

            accumulator -= FIXED_STEP;
            step_count += 1;
        }

        let positions = physics
            .positions()
            .context("read Box2D transforms for ECS synchronization")?;
        scene.set_physics_positions(&positions);
        snapshot = scripting.snapshot()?;

        renderer
            .render(&scene.draw_items(), snapshot)
            .context("render Brick Breaker frame")?;

        if step_count == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    drop(renderer);
    drop(window);
    drop(audio);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_horizontal_direction_is_symmetric() {
        let mut input = InputState {
            left: true,
            right: false,
            launch_pressed: false,
        };
        assert_eq!(input.horizontal(), -1.0);

        input.left = false;
        input.right = true;
        assert_eq!(input.horizontal(), 1.0);

        input.left = true;
        assert_eq!(input.horizontal(), 0.0);
    }

    #[test]
    fn launch_input_is_consumed_once() {
        let mut input = InputState {
            launch_pressed: true,
            ..InputState::default()
        };
        assert!(input.take_launch());
        assert!(!input.take_launch());
    }
}
