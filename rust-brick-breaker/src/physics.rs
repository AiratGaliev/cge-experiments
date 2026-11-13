use std::{collections::HashMap, time::Duration};

use anyhow::{Result, bail};
use avian2d::prelude::{
    Collider, ColliderDensity, CollisionEventsEnabled, CollisionStart, Friction, Gravity,
    GravityScale, LinearVelocity, Position, Restitution, RigidBody, Rotation, SweptCcd,
};
use bevy_ecs::{
    component::Component,
    entity::Entity,
    message::{MessageCursor, Messages},
    prelude::World,
    schedule::ScheduleLabel,
};
use bevy_math::Vec2;
use bevy_time::Time;

#[derive(ScheduleLabel, Clone, Debug, Eq, Hash, PartialEq)]
pub struct NativePhysicsSchedule;

pub const SOLVER_SUB_STEPS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RigidBodyKind {
    Static,
    Kinematic,
    Dynamic,
}

impl RigidBodyKind {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "static" => Ok(Self::Static),
            "kinematic" => Ok(Self::Kinematic),
            "dynamic" => Ok(Self::Dynamic),
            _ => bail!("unsupported rigid body type: {value}"),
        }
    }

    fn avian(self) -> RigidBody {
        match self {
            Self::Static => RigidBody::Static,
            Self::Kinematic => RigidBody::Kinematic,
            Self::Dynamic => RigidBody::Dynamic,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsMaterial {
    pub density: f32,
    pub friction: f32,
    pub restitution: f32,
    pub gravity_scale: f32,
    pub bullet: bool,
}

impl Default for PhysicsMaterial {
    fn default() -> Self {
        Self {
            density: 1.0,
            friction: 0.2,
            restitution: 0.0,
            gravity_scale: 1.0,
            bullet: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionEvent {
    pub entity_a: u32,
    pub entity_b: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsPositionChange {
    pub entity_id: u32,
    pub position: [f32; 2],
}

#[derive(Component, Clone, Copy, Debug)]
struct PhysicsRuntimeEntityId(u32);

pub struct PhysicsWorld {
    gravity: [f32; 2],
    bodies: HashMap<u32, Entity>,
    positions: HashMap<u32, [f32; 2]>,
    position_changes: Vec<PhysicsPositionChange>,
    collision_cursor: Option<MessageCursor<CollisionStart>>,
}

impl PhysicsWorld {
    pub fn new(gravity: [f32; 2]) -> Result<Self> {
        if gravity.iter().any(|value| !value.is_finite()) {
            bail!("physics gravity must contain finite values");
        }
        Ok(Self {
            gravity,
            bodies: HashMap::new(),
            positions: HashMap::new(),
            position_changes: Vec::new(),
            collision_cursor: None,
        })
    }

    /// Configures Avian's gravity on the single Bevy ECS world owned by EcsWorld.
    pub fn configure_world(&self, world: &mut World) {
        world.insert_resource(Gravity(Vec2::new(self.gravity[0], self.gravity[1])));
    }

    pub fn create_box(
        &mut self,
        world: &mut World,
        entity_id: u32,
        position: [f32; 2],
        size: [f32; 2],
        body_type: RigidBodyKind,
        material: PhysicsMaterial,
    ) -> Result<()> {
        self.validate_new_body(entity_id, position, material)?;
        if size.iter().any(|value| !value.is_finite() || *value <= 0.0) {
            bail!("box collider size must contain positive finite values");
        }
        let collider = Collider::rectangle(size[0], size[1]);
        self.spawn_body(world, entity_id, position, collider, body_type, material);
        Ok(())
    }

    pub fn create_circle(
        &mut self,
        world: &mut World,
        entity_id: u32,
        position: [f32; 2],
        radius: f32,
        body_type: RigidBodyKind,
        material: PhysicsMaterial,
    ) -> Result<()> {
        self.validate_new_body(entity_id, position, material)?;
        if !radius.is_finite() || radius <= 0.0 {
            bail!("circle collider radius must be positive and finite");
        }
        let collider = Collider::circle(radius);
        self.spawn_body(world, entity_id, position, collider, body_type, material);
        Ok(())
    }

    fn validate_new_body(
        &self,
        entity_id: u32,
        position: [f32; 2],
        material: PhysicsMaterial,
    ) -> Result<()> {
        if self.bodies.contains_key(&entity_id) {
            bail!("physics body for runtime entity {entity_id} already exists");
        }
        if position.iter().any(|value| !value.is_finite()) {
            bail!("physics body position must contain finite values");
        }
        if !material.density.is_finite()
            || material.density < 0.0
            || !material.friction.is_finite()
            || material.friction < 0.0
            || !material.restitution.is_finite()
            || !(0.0..=1.0).contains(&material.restitution)
            || !material.gravity_scale.is_finite()
        {
            bail!("invalid physics material");
        }
        Ok(())
    }

    fn spawn_body(
        &mut self,
        world: &mut World,
        entity_id: u32,
        position: [f32; 2],
        collider: Collider,
        body_type: RigidBodyKind,
        material: PhysicsMaterial,
    ) {
        let entity = world
            .spawn((
                PhysicsRuntimeEntityId(entity_id),
                body_type.avian(),
                collider,
                Position(Vec2::new(position[0], position[1])),
                Rotation::default(),
                LinearVelocity(Vec2::ZERO),
                GravityScale(material.gravity_scale),
                ColliderDensity(material.density),
                Friction::new(material.friction),
                Restitution::new(material.restitution),
                CollisionEventsEnabled,
            ))
            .id();

        if material.bullet {
            world.entity_mut(entity).insert(SweptCcd::default());
        }

        self.bodies.insert(entity_id, entity);
        self.positions.insert(entity_id, position);
        self.position_changes.push(PhysicsPositionChange {
            entity_id,
            position,
        });
    }

    pub fn set_linear_velocity(
        &mut self,
        world: &mut World,
        entity_id: u32,
        velocity: [f32; 2],
    ) -> Result<()> {
        if velocity.iter().any(|value| !value.is_finite()) {
            bail!("linear velocity must contain finite values");
        }
        let entity = self.body_entity(entity_id)?;
        let mut component = world
            .get_mut::<LinearVelocity>(entity)
            .ok_or_else(|| anyhow::anyhow!("physics body {entity_id} has no LinearVelocity"))?;
        component.0 = Vec2::new(velocity[0], velocity[1]);
        Ok(())
    }

    pub fn set_transform(
        &mut self,
        world: &mut World,
        entity_id: u32,
        position: [f32; 2],
    ) -> Result<()> {
        if position.iter().any(|value| !value.is_finite()) {
            bail!("physics transform must contain finite values");
        }
        let entity = self.body_entity(entity_id)?;
        let mut component = world
            .get_mut::<Position>(entity)
            .ok_or_else(|| anyhow::anyhow!("physics body {entity_id} has no Position"))?;
        component.0 = Vec2::new(position[0], position[1]);
        self.positions.insert(entity_id, position);
        self.position_changes.push(PhysicsPositionChange {
            entity_id,
            position,
        });
        Ok(())
    }

    pub fn destroy_body(&mut self, world: &mut World, entity_id: u32) -> Result<()> {
        let entity = self
            .bodies
            .remove(&entity_id)
            .ok_or_else(|| anyhow::anyhow!("unknown physics body {entity_id}"))?;
        world.despawn(entity);
        self.positions.remove(&entity_id);
        self.position_changes
            .retain(|change| change.entity_id != entity_id);
        Ok(())
    }

    pub fn has_body(&self, entity_id: u32) -> bool {
        self.bodies.contains_key(&entity_id)
    }

    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    fn body_entity(&self, entity_id: u32) -> Result<Entity> {
        self.bodies
            .get(&entity_id)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("unknown physics body {entity_id}"))
    }

    /// Runs one Avian step. The caller decides when a step is due; the runtime's
    /// fixed-step accumulator therefore remains the single timing authority.
    pub fn step_into(
        &mut self,
        world: &mut World,
        delta_seconds: f32,
        collisions: &mut Vec<CollisionEvent>,
    ) -> Result<()> {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            bail!("physics delta must be positive and finite");
        }

        if self.collision_cursor.is_none() {
            self.collision_cursor = Some(
                world
                    .resource::<Messages<CollisionStart>>()
                    .get_cursor_current(),
            );
        }

        world
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(delta_seconds));
        world.resource_mut::<Messages<CollisionStart>>().update();
        world.run_schedule(NativePhysicsSchedule);

        let messages = world.resource::<Messages<CollisionStart>>();
        let cursor = self
            .collision_cursor
            .as_mut()
            .expect("collision cursor is initialized before the physics schedule");
        for event in cursor.read(messages) {
            let Some(entity_a) = world
                .get::<PhysicsRuntimeEntityId>(event.collider1)
                .map(|id| id.0)
            else {
                continue;
            };
            let Some(entity_b) = world
                .get::<PhysicsRuntimeEntityId>(event.collider2)
                .map(|id| id.0)
            else {
                continue;
            };
            if entity_a != entity_b {
                collisions.push(CollisionEvent { entity_a, entity_b });
            }
        }

        for (&entity_id, &entity) in &self.bodies {
            let Some(position) = world.get::<Position>(entity) else {
                continue;
            };
            let current = [position.0.x, position.0.y];
            if current.iter().any(|value| !value.is_finite()) {
                bail!("Avian produced a non-finite position for runtime entity {entity_id}");
            }
            let changed = self.positions.get(&entity_id).is_none_or(|previous| {
                (previous[0] - current[0]).abs() > 0.000001
                    || (previous[1] - current[1]).abs() > 0.000001
            });
            if changed {
                self.positions.insert(entity_id, current);
                self.position_changes.push(PhysicsPositionChange {
                    entity_id,
                    position: current,
                });
            }
        }

        world.clear_trackers();
        Ok(())
    }

    pub fn positions_ref(&self) -> &HashMap<u32, [f32; 2]> {
        &self.positions
    }

    pub fn positions_mut(&mut self) -> &mut HashMap<u32, [f32; 2]> {
        &mut self.positions
    }

    pub fn positions_into(&self, output: &mut HashMap<u32, [f32; 2]>) {
        output.clear();
        output.extend(self.positions.iter().map(|(id, position)| (*id, *position)));
    }

    pub fn drain_position_changes_into(&mut self, output: &mut Vec<PhysicsPositionChange>) {
        output.clear();
        std::mem::swap(output, &mut self.position_changes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::EcsWorld;

    fn setup() -> (EcsWorld, PhysicsWorld) {
        let mut scene = EcsWorld::new();
        let physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics.configure_world(scene.world_mut());
        (scene, physics)
    }

    #[test]
    fn avian_creates_and_destroys_runtime_bodies_without_a_separate_world() {
        let (mut scene, mut physics) = setup();
        physics
            .create_circle(
                scene.world_mut(),
                7,
                [0.0, 0.0],
                0.25,
                RigidBodyKind::Dynamic,
                PhysicsMaterial::default(),
            )
            .unwrap();
        assert!(physics.has_body(7));
        assert_eq!(physics.body_count(), 1);
        assert_eq!(physics.positions_ref().get(&7), Some(&[0.0, 0.0]));

        physics.destroy_body(scene.world_mut(), 7).unwrap();
        assert!(!physics.has_body(7));
        assert!(!physics.positions_ref().contains_key(&7));
    }

    #[test]
    fn fixed_steps_advance_dynamic_bodies_and_report_transform_changes() {
        let (mut scene, mut physics) = setup();
        physics
            .create_circle(
                scene.world_mut(),
                1,
                [0.0, 0.0],
                0.1,
                RigidBodyKind::Dynamic,
                PhysicsMaterial {
                    gravity_scale: 0.0,
                    ..PhysicsMaterial::default()
                },
            )
            .unwrap();
        physics
            .set_linear_velocity(scene.world_mut(), 1, [2.0, 0.0])
            .unwrap();

        let mut collisions = Vec::new();
        physics
            .step_into(scene.world_mut(), 1.0 / 60.0, &mut collisions)
            .unwrap();

        let position = physics.positions_ref()[&1];
        assert!(position[0] > 0.0);
        assert!(position[1].abs() < 0.0001);

        let mut changes = Vec::new();
        physics.drain_position_changes_into(&mut changes);
        assert!(changes.iter().any(|change| change.entity_id == 1));
    }

    #[test]
    fn collision_start_events_are_forwarded_as_runtime_entity_ids() {
        let (mut scene, mut physics) = setup();
        let static_material = PhysicsMaterial {
            density: 0.0,
            gravity_scale: 0.0,
            ..PhysicsMaterial::default()
        };
        physics
            .create_box(
                scene.world_mut(),
                10,
                [0.0, 0.0],
                [1.0, 1.0],
                RigidBodyKind::Static,
                static_material,
            )
            .unwrap();
        physics
            .create_circle(
                scene.world_mut(),
                20,
                [0.0, 0.0],
                0.3,
                RigidBodyKind::Dynamic,
                PhysicsMaterial {
                    gravity_scale: 0.0,
                    ..PhysicsMaterial::default()
                },
            )
            .unwrap();

        let mut collisions = Vec::new();
        physics
            .step_into(scene.world_mut(), 1.0 / 60.0, &mut collisions)
            .unwrap();

        assert!(collisions.iter().any(|event| {
            (event.entity_a == 10 && event.entity_b == 20)
                || (event.entity_a == 20 && event.entity_b == 10)
        }));
    }

    #[test]
    fn avian_restitutes_a_dynamic_ball_from_a_static_wall_on_fixed_steps() {
        let (mut scene, mut physics) = setup();
        let surface = PhysicsMaterial {
            density: 0.0,
            friction: 0.0,
            restitution: 1.0,
            gravity_scale: 0.0,
            bullet: false,
        };
        let ball_material = PhysicsMaterial {
            density: 1.0,
            friction: 0.0,
            restitution: 1.0,
            gravity_scale: 0.0,
            bullet: false,
        };

        physics
            .create_box(
                scene.world_mut(),
                100,
                [0.0, 0.0],
                [0.2, 4.0],
                RigidBodyKind::Static,
                surface,
            )
            .unwrap();
        physics
            .create_circle(
                scene.world_mut(),
                200,
                [-1.0, 0.0],
                0.2,
                RigidBodyKind::Dynamic,
                ball_material,
            )
            .unwrap();
        physics
            .set_linear_velocity(scene.world_mut(), 200, [12.0, 0.0])
            .unwrap();

        let mut collisions = Vec::new();
        for _ in 0..30 {
            physics
                .step_into(scene.world_mut(), 1.0 / 60.0, &mut collisions)
                .unwrap();
        }

        let ball_entity = physics.body_entity(200).unwrap();
        let velocity = scene
            .world_mut()
            .get::<LinearVelocity>(ball_entity)
            .unwrap()
            .0;
        assert!(
            velocity.x < 0.0,
            "ball should rebound from the wall: {velocity:?}"
        );
        assert!(collisions.iter().any(|event| {
            (event.entity_a == 100 && event.entity_b == 200)
                || (event.entity_a == 200 && event.entity_b == 100)
        }));
    }

    #[test]
    fn invalid_body_data_and_duplicate_ids_are_rejected() {
        let (mut scene, mut physics) = setup();
        assert!(
            physics
                .create_box(
                    scene.world_mut(),
                    1,
                    [0.0, 0.0],
                    [0.0, 1.0],
                    RigidBodyKind::Static,
                    PhysicsMaterial::default(),
                )
                .is_err()
        );
        physics
            .create_circle(
                scene.world_mut(),
                1,
                [0.0, 0.0],
                0.1,
                RigidBodyKind::Dynamic,
                PhysicsMaterial::default(),
            )
            .unwrap();
        assert!(
            physics
                .create_circle(
                    scene.world_mut(),
                    1,
                    [1.0, 1.0],
                    0.1,
                    RigidBodyKind::Dynamic,
                    PhysicsMaterial::default(),
                )
                .is_err()
        );
    }

    #[test]
    fn kinematic_velocity_and_transform_updates_are_supported() {
        let (mut scene, mut physics) = setup();
        physics
            .create_box(
                scene.world_mut(),
                3,
                [0.0, 0.0],
                [1.0, 0.2],
                RigidBodyKind::Kinematic,
                PhysicsMaterial {
                    density: 0.0,
                    gravity_scale: 0.0,
                    ..PhysicsMaterial::default()
                },
            )
            .unwrap();
        physics
            .set_linear_velocity(scene.world_mut(), 3, [4.0, 0.0])
            .unwrap();

        let mut collisions = Vec::new();
        physics
            .step_into(scene.world_mut(), 1.0 / 60.0, &mut collisions)
            .unwrap();
        assert!(physics.positions_ref()[&3][0] > 0.0);

        physics
            .set_linear_velocity(scene.world_mut(), 3, [0.0, 0.0])
            .unwrap();
        physics
            .set_transform(scene.world_mut(), 3, [-2.0, 1.0])
            .unwrap();
        assert_eq!(physics.positions_ref()[&3], [-2.0, 1.0]);

        let mut changes = Vec::new();
        physics
            .step_into(scene.world_mut(), 1.0 / 60.0, &mut Vec::new())
            .unwrap();
        physics.drain_position_changes_into(&mut changes);
        assert!(
            changes
                .iter()
                .any(|change| { change.entity_id == 3 && change.position == [-2.0, 1.0] })
        );
    }
}
