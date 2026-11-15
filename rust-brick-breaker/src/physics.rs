use std::collections::{HashMap, HashSet};

use anyhow::{Context as _, Result, bail};
use oxijolt::prelude::math::{Quat, RVec3, Vec3};
use oxijolt::prelude::{
    Activation, ActivationEvent, AllowedDofs, BodyId, BodyPose, BodySettings, ContactEvent,
    EventSettings, MotionQuality, PhysicsWorld as JoltWorld, Shape, WorldSettings,
};

const SOLVER_COLLISION_STEPS: u32 = 4;
const PLANAR_HALF_DEPTH: f32 = 0.25;
const MIN_DYNAMIC_MASS: f32 = 1.0e-3;
const ZERO_DENSITY_DYNAMIC_MASS: f32 = 1.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RigidBodyKind {
    Static,
    Kinematic,
    Dynamic,
}

/// Controls how Jolt schedules physics jobs for this world.
///
/// SingleThreaded avoids worker-pool overhead for small scenes. WorkerThreads(n)
/// enables Jolt's worker pool; the thread calling step also participates in the work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicsExecutionMode {
    SingleThreaded,
    WorkerThreads(u32),
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
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicsMaterial {
    /// Planar surface density in kg/m².
    pub density: f32,
    pub friction: f32,
    pub restitution: f32,
    pub gravity_scale: f32,
    /// Enables Jolt linear-cast continuous collision detection for fast dynamic bodies.
    pub bullet: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CollisionEvent {
    pub entity_a: u32,
    pub entity_b: u32,
}

pub type PhysicsPositions = HashMap<u32, [f32; 2]>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsPositionChange {
    pub entity_id: u32,
    pub position: [f32; 2],
}

/// 2D adapter over Jolt. Bodies move in the XY plane: dynamic and kinematic
/// bodies use planar degrees of freedom, gravity has no Z component, and box
/// colliders have finite depth. Jolt IDs stay inside this adapter.
pub struct PhysicsWorld {
    world: JoltWorld,
    bodies: HashMap<u32, BodyId>,
    body_entities: HashMap<BodyId, u32>,
    unique_entity_pairs: HashSet<(u32, u32)>,
    positions: PhysicsPositions,
    position_changes: Vec<PhysicsPositionChange>,
    position_change_indices: HashMap<u32, usize>,
    active_poses: Vec<BodyPose>,
}

impl PhysicsWorld {
    pub fn new(gravity: [f32; 2]) -> Result<Self> {
        Self::new_with_execution_mode(gravity, PhysicsExecutionMode::SingleThreaded)
    }

    /// Creates a planar world with an explicit Jolt job-scheduling policy.
    ///
    /// The default constructor remains single-threaded for low-overhead small scenes.
    /// Larger games can opt into Jolt's worker pool and select its size from measurements.
    pub fn new_with_execution_mode(
        gravity: [f32; 2],
        execution_mode: PhysicsExecutionMode,
    ) -> Result<Self> {
        if gravity.iter().any(|value| !value.is_finite()) {
            bail!("physics gravity must be finite");
        }

        let settings = WorldSettings::default().gravity(Vec3::new(gravity[0], gravity[1], 0.0));
        let settings = match execution_mode {
            PhysicsExecutionMode::SingleThreaded => settings.single_threaded(),
            PhysicsExecutionMode::WorkerThreads(0) => {
                bail!("Jolt worker thread count must be greater than zero");
            }
            PhysicsExecutionMode::WorkerThreads(worker_threads) => {
                settings.worker_threads(worker_threads)
            }
        };
        let mut world = JoltWorld::new(settings).context("initialize Jolt Physics 5.6 world")?;
        world.set_event_settings(
            EventSettings::default()
                .contacts(true)
                .body_activation(true),
        );

        Ok(Self {
            world,
            bodies: HashMap::new(),
            body_entities: HashMap::new(),
            unique_entity_pairs: HashSet::new(),
            positions: PhysicsPositions::new(),
            position_changes: Vec::new(),
            position_change_indices: HashMap::new(),
            active_poses: Vec::new(),
        })
    }

    pub fn create_box(
        &mut self,
        entity_id: u32,
        position: [f32; 2],
        size: [f32; 2],
        body_type: RigidBodyKind,
        material: PhysicsMaterial,
    ) -> Result<()> {
        self.ensure_entity_slot_available(entity_id)?;
        validate_body(entity_id, position, material)?;
        if size.iter().any(|value| !value.is_finite() || *value <= 0.0) {
            bail!("box collider size must be finite and positive");
        }

        let shape = Shape::new_box(Vec3::new(size[0] * 0.5, size[1] * 0.5, PLANAR_HALF_DEPTH))
            .context("create Jolt planar box shape")?;
        let body = self.create_body(
            entity_id,
            position,
            body_type,
            material,
            size[0] * size[1],
            &shape,
        )?;
        self.register_body(entity_id, body, position);
        Ok(())
    }

    pub fn create_circle(
        &mut self,
        entity_id: u32,
        position: [f32; 2],
        radius: f32,
        body_type: RigidBodyKind,
        material: PhysicsMaterial,
    ) -> Result<()> {
        self.ensure_entity_slot_available(entity_id)?;
        validate_body(entity_id, position, material)?;
        if !radius.is_finite() || radius <= 0.0 {
            bail!("circle collider radius must be finite and positive");
        }

        // The XY cross-section of a sphere is circular. Its finite Z extent
        // gives Jolt a stable 3D contact volume while the body remains planar.
        let shape = Shape::new_sphere(radius).context("create Jolt planar circle shape")?;
        let area = std::f32::consts::PI * radius * radius;
        let body = self.create_body(entity_id, position, body_type, material, area, &shape)?;
        self.register_body(entity_id, body, position);
        Ok(())
    }

    fn create_body(
        &mut self,
        entity_id: u32,
        position: [f32; 2],
        body_type: RigidBodyKind,
        material: PhysicsMaterial,
        planar_area: f32,
        shape: &Shape,
    ) -> Result<BodyId> {
        let mut settings = match body_type {
            RigidBodyKind::Static => BodySettings::new_static(),
            RigidBodyKind::Kinematic => BodySettings::new_kinematic(),
            RigidBodyKind::Dynamic => BodySettings::new_dynamic(),
        }
        .position(RVec3::new(position[0], position[1], 0.0))
        .friction(material.friction)
        .restitution(material.restitution)
        .gravity_factor(material.gravity_scale);

        if body_type != RigidBodyKind::Static {
            settings = settings.allowed_dofs(AllowedDofs::PLANE_2D);
        }

        if body_type == RigidBodyKind::Dynamic {
            // The planar density contract is mass per area; Jolt takes an explicit mass.
            // A zero-density dynamic body gets a 1 kg compatibility fallback.
            let mass = if material.density == 0.0 {
                ZERO_DENSITY_DYNAMIC_MASS
            } else {
                (material.density * planar_area).max(MIN_DYNAMIC_MASS)
            };
            settings = settings.mass(mass);
        }

        if material.bullet && body_type == RigidBodyKind::Dynamic {
            settings = settings.motion_quality(MotionQuality::LinearCast);
        }

        self.world
            .create_body(shape, &settings)
            .with_context(|| format!("create Jolt body for runtime entity {entity_id}"))
    }

    fn ensure_entity_slot_available(&self, entity_id: u32) -> Result<()> {
        if self.bodies.contains_key(&entity_id) {
            bail!("entity {entity_id} already owns a physics body");
        }
        Ok(())
    }

    fn register_body(&mut self, entity_id: u32, body: BodyId, position: [f32; 2]) {
        self.bodies.insert(entity_id, body);
        self.body_entities.insert(body, entity_id);
        self.positions.insert(entity_id, position);
    }

    pub fn set_linear_velocity(&mut self, entity_id: u32, velocity: [f32; 2]) -> Result<()> {
        if velocity.iter().any(|value| !value.is_finite()) {
            bail!("body velocity must be finite");
        }
        let body = self.body(entity_id)?;
        self.world
            .body_mut(body)
            .context("access Jolt body to set linear velocity")?
            .set_linear_velocity(Vec3::new(velocity[0], velocity[1], 0.0))
            .context("set Jolt planar linear velocity")?;
        Ok(())
    }

    pub fn set_transform(&mut self, entity_id: u32, position: [f32; 2]) -> Result<()> {
        if position.iter().any(|value| !value.is_finite()) {
            bail!("body position must be finite");
        }
        let body = self.body(entity_id)?;
        self.world
            .body_mut(body)
            .context("access Jolt body to set transform")?
            .set_position_and_rotation(
                RVec3::new(position[0], position[1], 0.0),
                Quat::IDENTITY,
                Activation::Activate,
            )
            .context("set Jolt planar body transform")?;
        self.record_position_change(entity_id, position);
        Ok(())
    }

    pub fn destroy_body(&mut self, entity_id: u32) -> Result<()> {
        let body = self.body(entity_id)?;
        self.world
            .remove_body(body)
            .with_context(|| format!("remove Jolt body for runtime entity {entity_id}"))?;
        self.bodies.remove(&entity_id);
        self.body_entities.remove(&body);
        self.positions.remove(&entity_id);
        self.remove_pending_position_change(entity_id);
        Ok(())
    }

    pub fn has_body(&self, entity_id: u32) -> bool {
        self.bodies.contains_key(&entity_id)
    }

    fn body(&self, entity_id: u32) -> Result<BodyId> {
        self.bodies
            .get(&entity_id)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("entity {entity_id} has no physics body"))
    }

    pub fn step(&mut self, delta_seconds: f32) -> Result<Vec<CollisionEvent>> {
        let mut collisions = Vec::new();
        self.step_into(delta_seconds, &mut collisions)?;
        Ok(collisions)
    }

    /// Advances Jolt and appends newly-added contacts to a reusable output buffer.
    pub fn step_into(
        &mut self,
        delta_seconds: f32,
        collisions: &mut Vec<CollisionEvent>,
    ) -> Result<()> {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            bail!("physics delta must be finite and positive");
        }

        let report = self
            .world
            .step_with_collision_steps(delta_seconds, SOLVER_COLLISION_STEPS)
            .context("step Jolt Physics world")?;
        if !report.is_complete() {
            bail!(
                "Jolt dropped simulation work (manifold_cache_full={}, body_pair_cache_full={}, contact_constraints_full={})",
                report.manifold_cache_full,
                report.body_pair_cache_full,
                report.contact_constraints_full
            );
        }

        self.unique_entity_pairs.clear();
        let events = self.world.take_events();
        for event in events.contacts {
            if !matches!(event, ContactEvent::Added { .. }) {
                continue;
            }
            let pair = event.pair();
            let (Some(&entity_a), Some(&entity_b)) = (
                self.body_entities.get(&pair.body1),
                self.body_entities.get(&pair.body2),
            ) else {
                continue;
            };
            let normalized = if entity_a <= entity_b {
                (entity_a, entity_b)
            } else {
                (entity_b, entity_a)
            };
            if self.unique_entity_pairs.insert(normalized) {
                collisions.push(CollisionEvent {
                    entity_a: normalized.0,
                    entity_b: normalized.1,
                });
            }
        }

        // Bulk-read awake bodies in one Jolt multi-body lock. Static and sleeping
        // bodies retain their cached transforms and are not queried every step.
        self.world.active_body_poses_into(&mut self.active_poses);
        for index in 0..self.active_poses.len() {
            let pose = self.active_poses[index];
            let Some(&entity_id) = self.body_entities.get(&pose.id) else {
                continue;
            };
            self.record_position_change(entity_id, [pose.position.x, pose.position.y]);
        }

        // A body that sleeps on this step is absent from active_body_poses. Jolt's
        // deactivation event is the one-time final-pose read for that body.
        for event in events.activations {
            let ActivationEvent::Deactivated(body_id) = event else {
                continue;
            };
            let Some(&entity_id) = self.body_entities.get(&body_id) else {
                // Removing an awake body also emits Deactivated; it has no runtime owner now.
                continue;
            };
            let body_position = self
                .world
                .body(body_id)
                .with_context(|| {
                    format!("read final Jolt transform for sleeping entity {entity_id}")
                })?
                .position();
            self.record_position_change(entity_id, [body_position.x, body_position.y]);
        }

        Ok(())
    }

    pub fn positions(&self) -> Result<PhysicsPositions> {
        Ok(self.positions.clone())
    }

    /// Copies the transform cache into a caller-owned map while retaining its allocation.
    pub fn positions_into(&self, result: &mut PhysicsPositions) -> Result<()> {
        result.clear();
        result.reserve(self.positions.len());
        result.extend(
            self.positions
                .iter()
                .map(|(entity_id, position)| (*entity_id, *position)),
        );
        Ok(())
    }

    /// Borrows the transform cache without querying every Jolt body.
    pub fn positions_ref(&self) -> &PhysicsPositions {
        &self.positions
    }

    /// Mutably borrows the transform cache for synchronous zero-copy script handoff.
    pub fn positions_mut(&mut self) -> &mut PhysicsPositions {
        &mut self.positions
    }

    /// Records the latest changed position once per drain interval, even when several fixed
    /// physics steps update the same body before the ECS is synchronized.
    fn record_position_change(&mut self, entity_id: u32, position: [f32; 2]) {
        if self.positions.get(&entity_id).copied() == Some(position) {
            return;
        }
        self.positions.insert(entity_id, position);
        if let Some(&index) = self.position_change_indices.get(&entity_id) {
            self.position_changes[index].position = position;
        } else {
            let index = self.position_changes.len();
            self.position_changes.push(PhysicsPositionChange {
                entity_id,
                position,
            });
            self.position_change_indices.insert(entity_id, index);
        }
    }

    fn remove_pending_position_change(&mut self, entity_id: u32) {
        let Some(index) = self.position_change_indices.remove(&entity_id) else {
            return;
        };
        self.position_changes.swap_remove(index);
        if let Some(change) = self.position_changes.get(index) {
            self.position_change_indices.insert(change.entity_id, index);
        }
    }

    /// Transfers unique, latest transforms changed since the previous drain into a reusable buffer.
    pub fn drain_position_changes_into(&mut self, output: &mut Vec<PhysicsPositionChange>) {
        output.clear();
        output.append(&mut self.position_changes);
        self.position_change_indices.clear();
    }
}

fn validate_body(entity_id: u32, position: [f32; 2], material: PhysicsMaterial) -> Result<()> {
    if entity_id == 0 || position.iter().any(|value| !value.is_finite()) {
        bail!("invalid physics entity identity or position");
    }
    if !material.density.is_finite()
        || material.density < 0.0
        || !material.friction.is_finite()
        || !(0.0..=1000.0).contains(&material.friction)
        || !material.restitution.is_finite()
        || !(0.0..=1.0).contains(&material.restitution)
        || !material.gravity_scale.is_finite()
        || material.gravity_scale.abs() > 1000.0
    {
        bail!("invalid physics material");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material(density: f32) -> PhysicsMaterial {
        PhysicsMaterial {
            density,
            friction: 0.0,
            restitution: 1.0,
            gravity_scale: 0.0,
            bullet: true,
        }
    }

    fn has_pair(collisions: &[CollisionEvent], a: u32, b: u32) -> bool {
        collisions.iter().any(|event| {
            (event.entity_a, event.entity_b) == (a, b) || (event.entity_a, event.entity_b) == (b, a)
        })
    }

    #[test]
    fn physics_execution_mode_validates_worker_count() {
        assert!(
            PhysicsWorld::new_with_execution_mode(
                [0.0, 0.0],
                PhysicsExecutionMode::WorkerThreads(0)
            )
            .is_err()
        );

        let physics = PhysicsWorld::new_with_execution_mode(
            [0.0, 0.0],
            PhysicsExecutionMode::WorkerThreads(1),
        );
        assert!(physics.is_ok());
    }

    #[test]
    fn jolt_steps_dynamic_bodies_and_reports_runtime_entity_contacts() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_box(
                1,
                [-1.0, 0.0],
                [0.5, 2.0],
                RigidBodyKind::Static,
                material(0.0),
            )
            .unwrap();
        physics
            .create_circle(2, [0.0, 0.0], 0.2, RigidBodyKind::Dynamic, material(1.0))
            .unwrap();
        physics.set_linear_velocity(2, [-5.0, 0.0]).unwrap();

        let mut contacts = Vec::new();
        for _ in 0..30 {
            contacts.extend(physics.step(1.0 / 60.0).unwrap());
            if !contacts.is_empty() {
                break;
            }
        }
        assert!(
            has_pair(&contacts, 1, 2),
            "Jolt contacts must map to runtime entity IDs"
        );
        assert!(physics.positions().unwrap().contains_key(&2));
    }

    #[test]
    fn jolt_default_collision_layers_suppress_static_static_contacts() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_box(
                1,
                [0.0, 0.0],
                [1.0, 1.0],
                RigidBodyKind::Static,
                material(0.0),
            )
            .unwrap();
        physics
            .create_box(
                2,
                [0.0, 0.0],
                [1.0, 1.0],
                RigidBodyKind::Static,
                material(0.0),
            )
            .unwrap();

        let collisions = physics.step(1.0 / 60.0).unwrap();
        assert!(
            !has_pair(&collisions, 1, 2),
            "the default NON_MOVING layer must not collide with itself"
        );
    }

    #[test]
    fn physics_rejects_duplicate_bodies_and_invalid_materials() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_box(
                1,
                [0.0, 0.0],
                [1.0, 1.0],
                RigidBodyKind::Static,
                material(0.0),
            )
            .unwrap();
        assert!(
            physics
                .create_box(
                    1,
                    [0.0, 0.0],
                    [1.0, 1.0],
                    RigidBodyKind::Static,
                    material(0.0)
                )
                .is_err()
        );
        assert!(
            physics
                .create_circle(
                    2,
                    [0.0, 0.0],
                    0.1,
                    RigidBodyKind::Dynamic,
                    PhysicsMaterial {
                        restitution: 2.0,
                        ..material(1.0)
                    }
                )
                .is_err()
        );
        assert!(
            physics
                .create_circle(
                    3,
                    [0.0, 0.0],
                    0.1,
                    RigidBodyKind::Dynamic,
                    material(f32::NAN)
                )
                .is_err()
        );
    }

    #[test]
    fn destroying_a_physics_body_releases_its_mapping() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_circle(3, [0.0, 0.0], 0.2, RigidBodyKind::Dynamic, material(1.0))
            .unwrap();
        physics.destroy_body(3).unwrap();
        assert!(!physics.has_body(3));
        assert!(physics.positions().unwrap().is_empty());
        assert!(physics.body_entities.is_empty());
    }

    #[test]
    fn reusable_snapshots_preserve_capacity_and_refresh_positions() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_circle(7, [1.25, -2.5], 0.2, RigidBodyKind::Dynamic, material(1.0))
            .unwrap();
        let mut positions = PhysicsPositions::with_capacity(8);
        let original_capacity = positions.capacity();
        physics.positions_into(&mut positions).unwrap();
        assert_eq!(positions.get(&7), Some(&[1.25, -2.5]));
        let populated_capacity = positions.capacity();

        physics.set_transform(7, [3.0, 4.0]).unwrap();
        physics.positions_into(&mut positions).unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions.get(&7), Some(&[3.0, 4.0]));
        assert_eq!(positions.capacity(), populated_capacity);
        assert!(populated_capacity >= original_capacity);
    }

    #[test]
    fn step_into_appends_contacts_to_reusable_output() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_box(
                1,
                [-1.0, 0.0],
                [0.5, 2.0],
                RigidBodyKind::Static,
                material(0.0),
            )
            .unwrap();
        physics
            .create_circle(2, [0.0, 0.0], 0.2, RigidBodyKind::Dynamic, material(1.0))
            .unwrap();
        physics.set_linear_velocity(2, [-5.0, 0.0]).unwrap();

        let sentinel = CollisionEvent {
            entity_a: 90,
            entity_b: 91,
        };
        let mut collisions = vec![sentinel];
        for _ in 0..30 {
            physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
            if collisions.len() > 1 {
                break;
            }
        }
        assert_eq!(collisions[0], sentinel);
        assert!(has_pair(&collisions, 1, 2));
    }

    #[test]
    fn dynamic_and_kinematic_body_moves_update_the_transform_cache() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_circle(10, [0.0, 0.0], 0.2, RigidBodyKind::Dynamic, material(1.0))
            .unwrap();
        physics
            .create_circle(20, [10.0, 0.0], 0.2, RigidBodyKind::Static, material(0.0))
            .unwrap();
        physics
            .create_circle(
                30,
                [20.0, 0.0],
                0.2,
                RigidBodyKind::Kinematic,
                material(0.0),
            )
            .unwrap();
        physics.set_linear_velocity(10, [3.0, 0.0]).unwrap();
        physics.set_linear_velocity(30, [0.0, 2.0]).unwrap();

        let mut collisions = Vec::new();
        for _ in 0..5 {
            physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
        }
        let dynamic_position = physics.positions_ref()[&10];
        let kinematic_position = physics.positions_ref()[&30];
        assert!(dynamic_position[0] > 0.0);
        assert!(kinematic_position[1] > 0.0);
        assert_eq!(physics.positions_ref().get(&20), Some(&[10.0, 0.0]));

        let mut changes = Vec::new();
        physics.drain_position_changes_into(&mut changes);
        assert!(changes.iter().any(|change| change.entity_id == 10));
        assert!(changes.iter().any(|change| change.entity_id == 30));
        assert!(!changes.iter().any(|change| change.entity_id == 20));
        assert_eq!(
            changes
                .iter()
                .rev()
                .find(|change| change.entity_id == 10)
                .map(|change| change.position),
            Some(dynamic_position)
        );
    }

    #[test]
    fn cached_transform_includes_the_last_step_before_a_body_sleeps() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_circle(8, [0.0, 0.0], 0.2, RigidBodyKind::Dynamic, material(1.0))
            .unwrap();
        physics.set_linear_velocity(8, [2.0, 0.5]).unwrap();

        let mut collisions = Vec::new();
        for _ in 0..15 {
            physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
        }
        physics.set_linear_velocity(8, [0.0, 0.0]).unwrap();
        for _ in 0..180 {
            physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
        }

        let body_position = physics
            .world
            .body(physics.body(8).unwrap())
            .unwrap()
            .position();
        assert_eq!(
            physics.positions_ref().get(&8),
            Some(&[body_position.x, body_position.y])
        );
    }

    #[test]
    fn sleeping_bodies_are_not_synced_repeatedly_and_wake_updates_the_cache() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_circle(8, [0.0, 0.0], 0.2, RigidBodyKind::Dynamic, material(1.0))
            .unwrap();
        physics.set_linear_velocity(8, [2.0, 0.5]).unwrap();

        let mut collisions = Vec::new();
        for _ in 0..15 {
            physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
        }
        physics.set_linear_velocity(8, [0.0, 0.0]).unwrap();

        let mut changes = Vec::new();
        for _ in 0..180 {
            physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
            physics.drain_position_changes_into(&mut changes);
            if !physics
                .world
                .body(physics.body(8).unwrap())
                .unwrap()
                .is_active()
            {
                break;
            }
        }

        let body_id = physics.body(8).unwrap();
        assert!(!physics.world.body(body_id).unwrap().is_active());
        let final_position = physics.world.body(body_id).unwrap().position();
        assert_eq!(
            physics.positions_ref().get(&8),
            Some(&[final_position.x, final_position.y])
        );

        for _ in 0..4 {
            physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
        }
        physics.drain_position_changes_into(&mut changes);
        assert!(
            changes.is_empty(),
            "sleeping bodies must not produce per-step transform updates"
        );

        physics.set_linear_velocity(8, [1.0, 0.0]).unwrap();
        physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
        physics.drain_position_changes_into(&mut changes);
        assert!(
            changes.iter().any(|change| change.entity_id == 8),
            "a woken body must resume transform synchronization"
        );
    }

    #[test]
    fn benchmark_velocity_keeps_the_maximum_dense_body_set_awake() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        let material = material(1.0);
        for entity_id in 1..=1024 {
            let index = entity_id - 1;
            let position = [(index % 32) as f32 * 1.5, (index / 32) as f32 * 1.5];
            physics
                .create_circle(entity_id, position, 0.1, RigidBodyKind::Dynamic, material)
                .unwrap();
            physics.set_linear_velocity(entity_id, [0.4, 0.2]).unwrap();
        }

        let mut collisions = Vec::new();
        for _ in 0..180 {
            physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
        }

        assert_eq!(
            physics.active_poses.len(),
            1024,
            "the benchmark's maximum dense movement workload must remain awake for its measurement window"
        );
    }

    #[test]
    fn destroying_an_awake_body_ignores_its_queued_deactivation_event() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_circle(3, [0.0, 0.0], 0.2, RigidBodyKind::Dynamic, material(1.0))
            .unwrap();
        physics.set_linear_velocity(3, [1.0, 0.0]).unwrap();

        let mut collisions = Vec::new();
        physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
        physics.destroy_body(3).unwrap();

        physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
        assert!(!physics.has_body(3));
        assert!(!physics.positions_ref().contains_key(&3));
    }

    #[test]
    fn explicit_transform_updates_cache_and_destroy_removes_pending_changes() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_circle(5, [1.0, 2.0], 0.2, RigidBodyKind::Dynamic, material(1.0))
            .unwrap();
        physics.set_transform(5, [7.0, -3.0]).unwrap();
        assert_eq!(physics.positions_ref().get(&5), Some(&[7.0, -3.0]));
        physics.set_transform(5, [8.0, -4.0]).unwrap();

        let mut changes = Vec::new();
        physics.drain_position_changes_into(&mut changes);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity_id, 5);
        assert_eq!(changes[0].position, [8.0, -4.0]);

        physics.set_transform(5, [9.0, -5.0]).unwrap();
        physics.destroy_body(5).unwrap();
        assert!(!physics.positions_ref().contains_key(&5));
        physics.drain_position_changes_into(&mut changes);
        assert!(changes.is_empty());
    }

    #[test]
    fn positions_into_copies_the_jolt_transform_cache() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_circle(6, [2.0, 4.0], 0.2, RigidBodyKind::Dynamic, material(1.0))
            .unwrap();
        physics.set_linear_velocity(6, [1.0, 0.0]).unwrap();
        let mut collisions = Vec::new();
        physics.step_into(1.0 / 60.0, &mut collisions).unwrap();

        let mut snapshot = PhysicsPositions::with_capacity(16);
        let original_capacity = snapshot.capacity();
        physics.positions_into(&mut snapshot).unwrap();
        assert_eq!(snapshot, *physics.positions_ref());
        assert!(snapshot.capacity() >= original_capacity);
    }

    #[test]
    fn dynamic_bodies_are_constrained_to_the_xy_plane() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_circle(12, [0.0, 0.0], 0.2, RigidBodyKind::Dynamic, material(1.0))
            .unwrap();
        let body_id = physics.body(12).unwrap();
        {
            let mut body = physics.world.body_mut(body_id).unwrap();
            assert_eq!(body.allowed_dofs(), AllowedDofs::PLANE_2D);
            body.set_linear_velocity(Vec3::new(3.0, 4.0, 20.0)).unwrap();
            body.set_angular_velocity(Vec3::new(15.0, 25.0, 5.0))
                .unwrap();
        }

        let mut collisions = Vec::new();
        for _ in 0..30 {
            physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
        }
        let body = physics.world.body(body_id).unwrap();
        let position = body.position();
        let linear = body.linear_velocity();
        let angular = body.angular_velocity();
        assert!(position.z.abs() < 1.0e-5);
        assert!(linear.z.abs() < 1.0e-5);
        assert!(angular.x.abs() < 1.0e-5);
        assert!(angular.y.abs() < 1.0e-5);
    }

    #[test]
    fn linear_cast_prevents_a_fast_bullet_from_tunneling_through_a_thin_wall() {
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics
            .create_box(
                1,
                [0.0, 0.0],
                [0.08, 2.0],
                RigidBodyKind::Static,
                material(0.0),
            )
            .unwrap();
        physics
            .create_circle(
                2,
                [-1.0, 0.0],
                0.05,
                RigidBodyKind::Dynamic,
                PhysicsMaterial {
                    bullet: true,
                    ..material(1.0)
                },
            )
            .unwrap();
        physics.set_linear_velocity(2, [120.0, 0.0]).unwrap();

        let mut collisions = Vec::new();
        physics.step_into(1.0 / 60.0, &mut collisions).unwrap();
        assert!(has_pair(&collisions, 1, 2));
        assert!(
            physics.positions_ref()[&2][0] < 0.2,
            "bullet passed through wall: {:?}",
            physics.positions_ref()[&2]
        );
    }
}
