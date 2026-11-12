use std::collections::{HashMap, HashSet};

use anyhow::{Context as _, Result};
use boxdd::{
    BodyId, BodyType, Foundation, ShapeDef, ShapeId, SurfaceMaterial, World as Box2dWorld,
    WorldBuilder,
};

pub const BRICK_COLUMNS: usize = 9;
pub const BRICK_ROWS: usize = 4;
pub const BRICK_COUNT: usize = BRICK_COLUMNS * BRICK_ROWS;
const FIXED_SUB_STEPS: i32 = 4;

pub type PhysicsPositions = HashMap<u32, [f32; 2]>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColliderRole {
    Paddle,
    Ball,
    Wall,
    Brick(usize),
}

#[derive(Debug, Default)]
pub struct PhysicsStep {
    pub bounced: bool,
    pub bricks_destroyed: Vec<usize>,
    pub ball_lost: bool,
    pub remaining_bricks: usize,
}

pub struct Physics {
    _foundation: &'static Foundation,
    world: Box2dWorld,
    body_slots: Vec<Option<BodyId>>,
    brick_bodies: Vec<Option<BodyId>>,
    shape_roles: HashMap<ShapeId, ColliderRole>,
    paddle_body: BodyId,
    ball_body: BodyId,
    remaining_bricks: usize,
}

impl Physics {
    pub fn new() -> Result<Self> {
        let foundation =
            Foundation::initialize_default().context("initialize Box2D v3 foundation")?;
        let mut world = foundation
            .create_world(
                WorldBuilder::from(foundation.world_def())
                    .gravity([0.0, 0.0])
                    .enable_continuous(true)
                    .build()?,
            )
            .context("create Box2D v3 world")?;

        let paddle_def = world
            .body_builder()
            .body_type(BodyType::Kinematic)
            .position([0.0, 0.65])
            .gravity_scale(0.0)
            .build()?;
        let paddle_body = world.create_body(paddle_def)?;
        let paddle_shape = ShapeDef::builder()
            .density(0.0)
            .material(
                SurfaceMaterial::new()
                    .with_friction(0.0)?
                    .with_restitution(0.92)?,
            )
            .enable_contact_events(true)
            .build()?;
        let paddle_shape_id = world
            .body(paddle_body)?
            .create_box(&paddle_shape, 0.70, 0.12)?;

        let ball_def = world
            .body_builder()
            .body_type(BodyType::Dynamic)
            .position([0.0, 0.95])
            .gravity_scale(0.0)
            .bullet(true)
            .linear_velocity([0.0, 0.0])
            .build()?;
        let ball_body = world.create_body(ball_def)?;
        let ball_shape = ShapeDef::builder()
            .density(1.0)
            .material(
                SurfaceMaterial::new()
                    .with_friction(0.0)?
                    .with_restitution(1.0)?,
            )
            .enable_contact_events(true)
            .enable_hit_events(true)
            .build()?;
        let ball_shape_id = world
            .body(ball_body)?
            .create_centered_circle(&ball_shape, 0.12)?;

        let mut physics = Self {
            _foundation: foundation,
            world,
            body_slots: vec![Some(paddle_body), Some(ball_body)],
            brick_bodies: vec![None; BRICK_COUNT],
            shape_roles: HashMap::new(),
            paddle_body,
            ball_body,
            remaining_bricks: 0,
        };
        physics
            .shape_roles
            .insert(paddle_shape_id, ColliderRole::Paddle);
        physics
            .shape_roles
            .insert(ball_shape_id, ColliderRole::Ball);
        physics.create_walls()?;
        physics.create_bricks()?;

        Ok(physics)
    }

    fn create_walls(&mut self) -> Result<()> {
        self.create_static_box([-4.88, 3.72], 0.18, 3.88, ColliderRole::Wall)?;
        self.create_static_box([4.88, 3.72], 0.18, 3.88, ColliderRole::Wall)?;
        self.create_static_box([0.0, 7.42], 5.00, 0.18, ColliderRole::Wall)?;
        Ok(())
    }

    fn create_static_box(
        &mut self,
        position: [f32; 2],
        half_width: f32,
        half_height: f32,
        role: ColliderRole,
    ) -> Result<()> {
        let body = self
            .world
            .create_body(
                self.world
                    .body_builder()
                    .body_type(BodyType::Static)
                    .position(position)
                    .build()?,
            )
            .context("create static Box2D collider")?;
        let definition = ShapeDef::builder()
            .density(0.0)
            .material(
                SurfaceMaterial::new()
                    .with_friction(0.0)?
                    .with_restitution(1.0)?,
            )
            .enable_contact_events(true)
            .build()?;
        let shape = self
            .world
            .body(body)?
            .create_box(&definition, half_width, half_height)?;
        self.shape_roles.insert(shape, role);
        Ok(())
    }

    fn create_bricks(&mut self) -> Result<()> {
        for index in 0..BRICK_COUNT {
            let column = index % BRICK_COLUMNS;
            let row = index / BRICK_COLUMNS;
            let x = (column as f32 - 4.0) * 1.04;
            let y = 4.35 + row as f32 * 0.48;

            let body = self
                .world
                .create_body(
                    self.world
                        .body_builder()
                        .body_type(BodyType::Static)
                        .position([x, y])
                        .build()?,
                )
                .with_context(|| format!("create Box2D brick body {index}"))?;
            let definition = ShapeDef::builder()
                .density(0.0)
                .material(
                    SurfaceMaterial::new()
                        .with_friction(0.0)?
                        .with_restitution(1.0)?,
                )
                .enable_contact_events(true)
                .build()?;
            let shape = self.world.body(body)?.create_box(&definition, 0.46, 0.17)?;
            self.shape_roles.insert(shape, ColliderRole::Brick(index));
            self.brick_bodies[index] = Some(body);
            let slot = index + 2;
            if self.body_slots.len() <= slot {
                self.body_slots.resize(slot + 1, None);
            }
            self.body_slots[slot] = Some(body);
        }

        self.remaining_bricks = BRICK_COUNT;
        Ok(())
    }

    pub fn set_paddle_target(&mut self, target_x: f32, fixed_step: f32) -> Result<()> {
        if fixed_step <= f32::EPSILON {
            return Ok(());
        }

        let current_x = self.world.body(self.paddle_body)?.position()?.x as f32;
        let target_x = target_x.clamp(-3.75, 3.75);
        let velocity_x = ((target_x - current_x) / fixed_step).clamp(-12.0, 12.0);
        self.world
            .body(self.paddle_body)?
            .set_linear_velocity([velocity_x, 0.0])?;
        Ok(())
    }

    pub fn launch(&mut self) -> Result<()> {
        let paddle_x = self.world.body(self.paddle_body)?.position()?.x as f32;
        self.world
            .body(self.ball_body)?
            .set_position_and_rotation([paddle_x, 0.98], 0.0)?;
        self.world
            .body(self.ball_body)?
            .set_linear_velocity([1.7, 4.9])?;
        Ok(())
    }

    pub fn park_ball(&mut self) -> Result<()> {
        let paddle_x = self.world.body(self.paddle_body)?.position()?.x as f32;
        self.world
            .body(self.ball_body)?
            .set_position_and_rotation([paddle_x, 0.98], 0.0)?;
        self.world
            .body(self.ball_body)?
            .set_linear_velocity([0.0, 0.0])?;
        Ok(())
    }

    pub fn step(&mut self, delta_seconds: f32) -> Result<PhysicsStep> {
        let completed = self.world.step(delta_seconds, FIXED_SUB_STEPS)?;
        let contacts = completed
            .contact_events()
            .context("read Box2D v3 contact events")?;
        let pairs: Vec<(ShapeId, ShapeId)> = contacts
            .begin()
            .iter()
            .map(|event| (event.shape_a, event.shape_b))
            .collect();
        drop(contacts);
        drop(completed);

        let mut destroyed = HashSet::new();
        let mut bounced = false;

        for (shape_a, shape_b) in pairs {
            let role_a = self.shape_roles.get(&shape_a).copied();
            let role_b = self.shape_roles.get(&shape_b).copied();
            if role_a == Some(ColliderRole::Ball) || role_b == Some(ColliderRole::Ball) {
                bounced = true;
                if let Some(ColliderRole::Brick(index)) = role_a {
                    if role_b == Some(ColliderRole::Ball) {
                        destroyed.insert(index);
                    }
                }
                if let Some(ColliderRole::Brick(index)) = role_b {
                    if role_a == Some(ColliderRole::Ball) {
                        destroyed.insert(index);
                    }
                }
            }
        }

        let mut bricks_destroyed = Vec::with_capacity(destroyed.len());
        for index in destroyed {
            self.destroy_brick(index)?;
            bricks_destroyed.push(index);
        }
        bricks_destroyed.sort_unstable();

        let ball_lost = (self.world.body(self.ball_body)?.position()?.y as f32) < -0.20;
        Ok(PhysicsStep {
            bounced,
            bricks_destroyed,
            ball_lost,
            remaining_bricks: self.remaining_bricks,
        })
    }

    fn destroy_brick(&mut self, index: usize) -> Result<()> {
        let Some(body) = self.brick_bodies.get_mut(index).and_then(Option::take) else {
            return Ok(());
        };

        self.world
            .body(body)?
            .destroy()
            .with_context(|| format!("destroy Box2D brick body {index}"))?;
        self.shape_roles
            .retain(|_, role| *role != ColliderRole::Brick(index));
        self.body_slots[index + 2] = None;
        self.remaining_bricks = self.remaining_bricks.saturating_sub(1);
        Ok(())
    }

    pub fn positions(&mut self) -> Result<PhysicsPositions> {
        let mut positions = PhysicsPositions::with_capacity(self.body_slots.len());
        for (slot, body) in self.body_slots.iter().enumerate() {
            let Some(body) = body else {
                continue;
            };
            let position = self.world.body(*body)?.position()?;
            positions.insert(slot as u32, [position.x as f32, position.y as f32]);
        }
        Ok(positions)
    }

    pub fn reset(&mut self) -> Result<()> {
        for index in 0..BRICK_COUNT {
            self.destroy_brick(index)?;
        }
        self.create_bricks()?;

        self.world
            .body(self.paddle_body)?
            .set_position_and_rotation([0.0, 0.65], 0.0)?;
        self.world
            .body(self.paddle_body)?
            .set_linear_velocity([0.0, 0.0])?;
        self.park_ball()?;
        Ok(())
    }

    pub fn remaining_bricks(&self) -> usize {
        self.remaining_bricks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box2d_v3_contact_events_destroy_bricks_after_the_step_is_unlocked() {
        let mut physics = Physics::new().expect("create Box2D world");
        physics.launch().expect("launch ball");

        let mut destroyed = Vec::new();
        for _ in 0..180 {
            let result = physics
                .step(1.0 / 60.0)
                .expect("step Box2D world and capture contacts");
            destroyed.extend(result.bricks_destroyed);
            if !destroyed.is_empty() {
                break;
            }
        }

        assert!(!destroyed.is_empty(), "the ball should hit a brick");
        assert_eq!(physics.remaining_bricks(), BRICK_COUNT - destroyed.len());
    }

    #[test]
    fn reset_recreates_bricks_in_the_same_ecs_slots() {
        let mut physics = Physics::new().expect("create Box2D world");
        physics.launch().expect("launch ball");
        for _ in 0..180 {
            let result = physics.step(1.0 / 60.0).expect("step Box2D world");
            if !result.bricks_destroyed.is_empty() {
                break;
            }
        }

        physics.reset().expect("reset physics world");

        let positions = physics.positions().expect("read Box2D body positions");
        assert_eq!(positions.len(), BRICK_COUNT + 2);
        assert_eq!(physics.remaining_bricks(), BRICK_COUNT);
    }
}
