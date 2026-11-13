use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, StartCause},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

use native_brick_breaker::{
    audio::Audio,
    ecs::EcsWorld,
    input::{GamepadManager, GamepadState, InputState, key_name},
    physics::PhysicsWorld,
    render::Renderer,
    scripting::{RuntimeCommand, ScriptEngine},
};

const WINDOW_WIDTH: u32 = 960;
const WINDOW_HEIGHT: u32 = 720;
const FIXED_STEP: f32 = 1.0 / 60.0;
const MAX_FRAME_DELTA: f32 = 0.10;
const MAX_STEPS_PER_FRAME: usize = 6;
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

struct Runtime {
    renderer: Renderer,
    _audio: Audio,
    physics: PhysicsWorld,
    scene: EcsWorld,
    script: ScriptEngine,
    input: InputState,
    gamepad: GamepadManager,
    last_frame: Instant,
    accumulator: f32,
    commands: Vec<RuntimeCommand>,
    pending_collisions: Vec<native_brick_breaker::physics::CollisionEvent>,
    moved_positions: Vec<native_brick_breaker::physics::PhysicsPositionChange>,
    rectangles: Vec<native_brick_breaker::ecs::DrawRectangle>,
    circles: Vec<native_brick_breaker::ecs::DrawCircle>,
    labels: Vec<native_brick_breaker::ecs::DrawText>,
}

impl Runtime {
    fn new(window: Arc<Window>, script_path: &std::path::Path) -> Result<Self> {
        let renderer = pollster::block_on(Renderer::new(window))
            .context("initialize wgpu renderer for the winit window")?;
        let audio = Audio::new();
        let mut scene = EcsWorld::new();
        let mut physics =
            PhysicsWorld::new([0.0, 0.0]).context("initialize Avian 2D physics configuration")?;
        physics.configure_world(scene.world_mut());

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

        println!("GPU adapter: {}", renderer.adapter_name());
        println!("Game script: {}", script_path.display());
        println!(
            "Initial scene: {} ECS entities, {} rectangles, {} circles, {} text labels, {} Avian bodies",
            scene.entity_count(),
            rectangles.len(),
            circles.len(),
            labels.len(),
            physics.positions_ref().len()
        );
        println!("Native runtime initialized. Press Escape to quit.");

        Ok(Self {
            renderer,
            _audio: audio,
            physics,
            scene,
            script,
            input: InputState::default(),
            gamepad: GamepadManager::new(),
            last_frame: Instant::now(),
            accumulator: 0.0,
            commands,
            pending_collisions: Vec::new(),
            moved_positions: Vec::new(),
            rectangles,
            circles,
            labels,
        })
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.renderer.resize(size.width, size.height);
    }

    fn key_event(&mut self, event: KeyEvent) {
        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };
        let Some(name) = key_name(code) else {
            return;
        };

        match event.state {
            ElementState::Pressed if !event.repeat => self.input.key_down(name),
            ElementState::Released => self.input.key_up(name),
            _ => {}
        }
    }

    fn frame(&mut self) -> Result<()> {
        let now = Instant::now();
        let frame_delta = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;
        let frame_delta = frame_delta.min(MAX_FRAME_DELTA);
        self.accumulator =
            (self.accumulator + frame_delta).min(FIXED_STEP * MAX_STEPS_PER_FRAME as f32);

        let gamepad_state: GamepadState = self.gamepad.update();
        self.script
            .update_frame(
                frame_delta,
                &mut self.input.down,
                &mut self.input.pressed,
                gamepad_state,
                self.physics.positions_mut(),
                &mut self.pending_collisions,
            )
            .context("execute TypeScript game update")?;
        self.pending_collisions.clear();

        self.script.drain_commands_into(&mut self.commands);
        apply_commands(&mut self.commands, &mut self.scene, &mut self.physics)?;
        self.scene.apply_pending_changes();

        let mut step_count = 0;
        while self.accumulator >= FIXED_STEP && step_count < MAX_STEPS_PER_FRAME {
            self.physics
                .step_into(
                    self.scene.world_mut(),
                    FIXED_STEP,
                    &mut self.pending_collisions,
                )
                .context("advance fixed-step Avian 2D simulation")?;
            self.accumulator -= FIXED_STEP;
            step_count += 1;
        }

        self.physics
            .drain_position_changes_into(&mut self.moved_positions);
        self.scene
            .synchronize_physics_adaptive(&self.moved_positions, self.physics.positions_ref())
            .context("synchronize Avian positions into Bevy ECS scene transforms")?;

        self.scene.draw_rectangles_into(&mut self.rectangles);
        self.scene.draw_circles_into(&mut self.circles);
        self.scene.draw_text_into(&mut self.labels);
        self.renderer
            .render(&self.rectangles, &self.circles, &self.labels)
            .context("render native ECS scene")?;

        self.input.finish_frame();
        Ok(())
    }

    fn shutdown(&self) -> Result<()> {
        self.script.shutdown().context("shutdown TypeScript game")
    }
}

struct RuntimeApp {
    script_path: PathBuf,
    window: Option<Arc<Window>>,
    window_id: Option<WindowId>,
    runtime: Option<Runtime>,
    error: Option<anyhow::Error>,
    next_redraw: Option<Instant>,
}

impl RuntimeApp {
    fn new(script_path: PathBuf) -> Self {
        Self {
            script_path,
            window: None,
            window_id: None,
            runtime: None,
            error: None,
            next_redraw: None,
        }
    }
}

impl ApplicationHandler for RuntimeApp {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.runtime.is_some() {
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Codogma Native 2D Runtime")
            .with_inner_size(PhysicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
            .with_resizable(true);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.error = Some(anyhow::Error::new(error).context("create winit window"));
                event_loop.exit();
                return;
            }
        };

        let runtime = match Runtime::new(Arc::clone(&window), &self.script_path) {
            Ok(runtime) => runtime,
            Err(error) => {
                self.error = Some(error);
                event_loop.exit();
                return;
            }
        };

        self.window_id = Some(window.id());
        self.window = Some(window);
        self.runtime = Some(runtime);
        self.next_redraw = Some(Instant::now() + FRAME_INTERVAL);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window_id != Some(window_id) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(runtime) = self.runtime.as_mut() {
                    runtime.resize(size);
                }
            }
            WindowEvent::Focused(false) => {
                if let Some(runtime) = self.runtime.as_mut() {
                    runtime.input.clear();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if Self::is_escape(&event) {
                    event_loop.exit();
                    return;
                }
                if let Some(runtime) = self.runtime.as_mut() {
                    runtime.key_event(event);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(runtime) = self.runtime.as_mut() {
                    if let Err(error) = runtime.frame() {
                        self.error = Some(error);
                        event_loop.exit();
                    }
                }
                self.next_redraw = Some(Instant::now() + FRAME_INTERVAL);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            let deadline = self
                .next_redraw
                .get_or_insert_with(|| Instant::now() + FRAME_INTERVAL);
            event_loop.set_control_flow(ControlFlow::WaitUntil(*deadline));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(runtime) = self.runtime.as_ref() {
            if let Err(error) = runtime.shutdown() {
                if self.error.is_none() {
                    self.error = Some(error);
                } else {
                    eprintln!("Game shutdown failed: {error:#}");
                }
            }
        }
    }
}

impl RuntimeApp {
    fn is_escape(event: &KeyEvent) -> bool {
        event.state == ElementState::Pressed
            && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
    }
}

fn main() -> Result<()> {
    let script_path = std::env::var_os("CGE_GAME_SCRIPT")
        .map(PathBuf::from)
        .or_else(|| std::env::args_os().nth(1).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("assets/game.js"));

    let event_loop = EventLoop::new().context("create winit event loop")?;
    let mut app = RuntimeApp::new(script_path);
    event_loop
        .run_app(&mut app)
        .context("run winit application event loop")?;

    if let Some(error) = app.error {
        return Err(error);
    }
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
                    physics.set_transform(scene.world_mut(), entity_id, [x, y])?;
                } else {
                    scene.set_position(entity_id, x, y)?;
                }
            }
            RuntimeCommand::SetText { entity_id, value } => {
                scene.set_text(entity_id, value)?;
            }
            RuntimeCommand::DestroyEntity { entity_id } => {
                if physics.has_body(entity_id) {
                    physics.destroy_body(scene.world_mut(), entity_id)?;
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
                physics.create_box(
                    scene.world_mut(),
                    entity_id,
                    position,
                    size,
                    body_type,
                    material,
                )?;
                if let Err(error) = scene.attach_physics(entity_id) {
                    let _ = physics.destroy_body(scene.world_mut(), entity_id);
                    return Err(error);
                }
            }
            RuntimeCommand::CreateCircleBody {
                entity_id,
                position,
                radius,
                body_type,
                material,
            } => {
                physics.create_circle(
                    scene.world_mut(),
                    entity_id,
                    position,
                    radius,
                    body_type,
                    material,
                )?;
                if let Err(error) = scene.attach_physics(entity_id) {
                    let _ = physics.destroy_body(scene.world_mut(), entity_id);
                    return Err(error);
                }
            }
            RuntimeCommand::SetLinearVelocity {
                entity_id,
                velocity,
            } => physics.set_linear_velocity(scene.world_mut(), entity_id, velocity)?,
            RuntimeCommand::SetBodyPosition {
                entity_id,
                position,
            } => physics.set_transform(scene.world_mut(), entity_id, position)?,
            RuntimeCommand::DestroyBody { entity_id } => {
                physics.destroy_body(scene.world_mut(), entity_id)?
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use native_brick_breaker::input::key_name;

    #[test]
    fn input_state_tracks_down_and_edge_triggered_presses() {
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
    fn focus_loss_clears_held_keyboard_state() {
        let mut input = InputState::default();
        input.key_down("A");
        input.key_down("ArrowLeft");
        input.clear();
        assert!(input.down.is_empty());
        assert!(input.pressed.is_empty());
    }

    #[test]
    fn destroying_a_runtime_entity_also_removes_its_avian_body() {
        use native_brick_breaker::physics::{PhysicsMaterial, RigidBodyKind};

        let mut scene = EcsWorld::new();
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics.configure_world(scene.world_mut());
        let mut commands = vec![
            RuntimeCommand::CreateCircle {
                entity_id: 42,
                x: 0.0,
                y: 0.0,
                radius: 0.2,
                color: [1.0, 1.0, 1.0, 1.0],
            },
            RuntimeCommand::CreateCircleBody {
                entity_id: 42,
                position: [0.0, 0.0],
                radius: 0.2,
                body_type: RigidBodyKind::Dynamic,
                material: PhysicsMaterial::default(),
            },
            RuntimeCommand::DestroyEntity { entity_id: 42 },
        ];

        apply_commands(&mut commands, &mut scene, &mut physics).unwrap();

        assert_eq!(scene.entity_count(), 0);
        assert!(!physics.has_body(42));
    }

    #[test]
    fn key_mapping_is_platform_neutral() {
        assert_eq!(key_name(KeyCode::ArrowLeft), Some("ArrowLeft"));
        assert_eq!(key_name(KeyCode::Space), Some("Space"));
        assert_eq!(key_name(KeyCode::KeyA), Some("A"));
    }
}
