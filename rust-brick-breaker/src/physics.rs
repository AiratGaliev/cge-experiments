use std::collections::{HashMap, HashSet};

use anyhow::{Context as _, Result, bail};
use boxdd::{
    BodyId, BodyType, Foundation, ShapeDef, ShapeId, SurfaceMaterial, World as Box2dWorld,
    WorldBuilder,
};

const SOLVER_SUB_STEPS: i32 = 4;

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

    fn as_box2d(self) -> BodyType {
        match self {
            Self::Static => BodyType::Static,
            Self::Kinematic => BodyType::Kinematic,
            Self::Dynamic => BodyType::Dynamic,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicsMaterial {
    pub density: f32,
    pub friction: f32,
    pub restitution: f32,
    pub gravity_scale: f32,
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

pub struct PhysicsWorld {
    _foundation: &'static Foundation,
    world: Box2dWorld,
    bodies: HashMap<u32, BodyId>,
    body_entities: HashMap<BodyId, u32>,
    body_shapes: HashMap<u32, ShapeId>,
    shape_entities: HashMap<ShapeId, u32>,
    contact_shape_pairs: Vec<(ShapeId, ShapeId)>,
    unique_entity_pairs: HashSet<(u32, u32)>,
    positions: PhysicsPositions,

    position_changes: Vec<PhysicsPositionChange>,
}

impl PhysicsWorld {
    pub fn new(gravity: [f32; 2]) -> Result<Self> {
        if gravity.iter().any(|value| !value.is_finite()) {
            bail!("physics gravity must be finite");
        }
        let foundation =
            Foundation::initialize_default().context("initialize Box2D v3 foundation")?;
        let world = foundation
            .create_world(
                WorldBuilder::from(foundation.world_def())
                    .gravity(gravity)
                    .enable_continuous(true)
                    .build()?,
            )
            .context("create Box2D v3 world")?;

        Ok(Self {
            _foundation: foundation,
            world,
            bodies: HashMap::new(),
            body_entities: HashMap::new(),
            body_shapes: HashMap::new(),
            shape_entities: HashMap::new(),
            contact_shape_pairs: Vec::new(),
            unique_entity_pairs: HashSet::new(),
            positions: PhysicsPositions::new(),

            position_changes: Vec::new(),
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

        let definition = shape_definition(material)?;
        let body = self.create_body(position, body_type, material)?;
        let shape = match self
            .world
            .body(body)?
            .create_box(&definition, size[0] * 0.5, size[1] * 0.5)
            .context("create Box2D box shape")
        {
            Ok(shape) => shape,
            Err(error) => {
                self.world
                    .body(body)?
                    .destroy()
                    .context("rollback Box2D body after box shape creation failure")?;
                return Err(error);
            }
        };
        self.register_body(entity_id, body, shape, position);
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

        let definition = shape_definition(material)?;
        let body = self.create_body(position, body_type, material)?;
        let shape = match self
            .world
            .body(body)?
            .create_centered_circle(&definition, radius)
            .context("create Box2D circle shape")
        {
            Ok(shape) => shape,
            Err(error) => {
                self.world
                    .body(body)?
                    .destroy()
                    .context("rollback Box2D body after circle shape creation failure")?;
                return Err(error);
            }
        };
        self.register_body(entity_id, body, shape, position);
        Ok(())
    }

    fn create_body(
        &mut self,
        position: [f32; 2],
        body_type: RigidBodyKind,
        material: PhysicsMaterial,
    ) -> Result<BodyId> {
        let mut definition = self
            .world
            .body_builder()
            .body_type(body_type.as_box2d())
            .position(position)
            .gravity_scale(material.gravity_scale);
        if material.bullet {
            definition = definition.bullet(true);
        }
        self.world
            .create_body(definition.build()?)
            .context("create Box2D body")
    }

    fn ensure_entity_slot_available(&self, entity_id: u32) -> Result<()> {
        if self.bodies.contains_key(&entity_id) {
            bail!("entity {entity_id} already owns a physics body");
        }
        Ok(())
    }

    fn register_body(&mut self, entity_id: u32, body: BodyId, shape: ShapeId, position: [f32; 2]) {
        self.bodies.insert(entity_id, body);
        self.body_entities.insert(body, entity_id);
        self.body_shapes.insert(entity_id, shape);
        self.shape_entities.insert(shape, entity_id);
        self.positions.insert(entity_id, position);
    }

    pub fn set_linear_velocity(&mut self, entity_id: u32, velocity: [f32; 2]) -> Result<()> {
        if velocity.iter().any(|value| !value.is_finite()) {
            bail!("body velocity must be finite");
        }
        let body = self.body(entity_id)?;
        self.world
            .body(body)?
            .set_linear_velocity(velocity)
            .context("set Box2D linear velocity")?;
        Ok(())
    }

    pub fn set_transform(&mut self, entity_id: u32, position: [f32; 2]) -> Result<()> {
        if position.iter().any(|value| !value.is_finite()) {
            bail!("body position must be finite");
        }
        let body = self.body(entity_id)?;
        self.world
            .body(body)?
            .set_position_and_rotation(position, 0.0)
            .context("set Box2D body transform")?;
        self.positions.insert(entity_id, position);
        self.position_changes.push(PhysicsPositionChange {
            entity_id,
            position,
        });
        Ok(())
    }

    pub fn destroy_body(&mut self, entity_id: u32) -> Result<()> {
        let body = self.body(entity_id)?;
        self.world
            .body(body)?
            .destroy()
            .context("destroy Box2D body")?;
        self.bodies.remove(&entity_id);
        self.body_entities.remove(&body);
        self.positions.remove(&entity_id);
        self.position_changes
            .retain(|change| change.entity_id != entity_id);
        if let Some(shape) = self.body_shapes.remove(&entity_id) {
            self.shape_entities.remove(&shape);
        }
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

    /// Advances Box2D and appends newly-begun contacts to a caller-owned buffer.
    /// The internal contact scratch collections retain their capacity between steps.
    pub fn step_into(
        &mut self,
        delta_seconds: f32,
        collisions: &mut Vec<CollisionEvent>,
    ) -> Result<()> {
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            bail!("physics delta must be finite and positive");
        }

        self.contact_shape_pairs.clear();
        {
            // The completed-step view borrows only the Box2D world. Split out the cache,
            // reverse identity map and event buffers so events update them in place.
            let contact_shape_pairs = &mut self.contact_shape_pairs;
            let body_entities = &self.body_entities;
            let positions = &mut self.positions;
            let position_changes = &mut self.position_changes;

            let completed = self
                .world
                .step(delta_seconds, SOLVER_SUB_STEPS)
                .context("step Box2D v3 world")?;

            {
                let contacts = completed
                    .contact_events()
                    .context("read Box2D v3 contact events")?;
                contact_shape_pairs.extend(
                    contacts
                        .begin()
                        .iter()
                        .map(|event| (event.shape_a, event.shape_b)),
                );
            }

            let body_events = completed
                .body_events()
                .context("read Box2D body move events")?;
            for event in body_events.iter() {
                let Some(&entity_id) = body_entities.get(&event.body_id) else {
                    continue;
                };
                let position = event.transform.position();
                let position = [position.x, position.y];
                positions.insert(entity_id, position);
                position_changes.push(PhysicsPositionChange {
                    entity_id,
                    position,
                });
            }
        }

        self.unique_entity_pairs.clear();
        for (shape_a, shape_b) in &self.contact_shape_pairs {
            let (Some(&entity_a), Some(&entity_b)) = (
                self.shape_entities.get(shape_a),
                self.shape_entities.get(shape_b),
            ) else {
                continue;
            };
            let pair = if entity_a <= entity_b {
                (entity_a, entity_b)
            } else {
                (entity_b, entity_a)
            };
            if self.unique_entity_pairs.insert(pair) {
                collisions.push(CollisionEvent {
                    entity_a: pair.0,
                    entity_b: pair.1,
                });
            }
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

    /// Borrows the transform cache without querying every Box2D body.
    pub fn positions_ref(&self) -> &PhysicsPositions {
        &self.positions
    }

    /// Mutably borrows the transform cache for synchronous zero-copy script handoff.
    pub fn positions_mut(&mut self) -> &mut PhysicsPositions {
        &mut self.positions
    }

    /// Transfers only transforms changed since the previous drain into a reusable output buffer.
    pub fn drain_position_changes_into(&mut self, output: &mut Vec<PhysicsPositionChange>) {
        output.clear();
        output.append(&mut self.position_changes);
    }
}

fn validate_body(entity_id: u32, position: [f32; 2], material: PhysicsMaterial) -> Result<()> {
    if entity_id == 0 || position.iter().any(|value| !value.is_finite()) {
        bail!("invalid physics entity identity or position");
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

fn shape_definition(material: PhysicsMaterial) -> Result<ShapeDef> {
    Ok(ShapeDef::builder()
        .density(material.density)
        .material(
            SurfaceMaterial::new()
                .with_friction(material.friction)?
                .with_restitution(material.restitution)?,
        )
        .enable_contact_events(true)
        .enable_hit_events(true)
        .build()?)
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

    #[test]
    fn box2d_steps_dynamic_bodies_and_reports_runtime_entity_contacts() {
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
            contacts.iter().any(|event| {
                (event.entity_a, event.entity_b) == (1, 2)
                    || (event.entity_a, event.entity_b) == (2, 1)
            }),
            "Box2D should return the IDs supplied by the runtime"
        );
        assert!(physics.positions().unwrap().contains_key(&2));
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
        assert!(
            collisions.iter().any(|event| {
                (event.entity_a, event.entity_b) == (1, 2)
                    || (event.entity_a, event.entity_b) == (2, 1)
            }),
            "reusable collision buffer should receive Box2D begin-contact events"
        );
    }

    #[test]
    fn body_move_events_refresh_cached_transforms_and_dirty_entities() {
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

        let moving_position = physics.positions_ref()[&10];
        let kinematic_position = physics.positions_ref()[&30];
        assert!(moving_position[0] > 0.0);
        assert!(kinematic_position[1] > 0.0);
        assert_eq!(physics.positions_ref().get(&20), Some(&[10.0, 0.0]));

        let mut changes = Vec::new();
        physics.drain_position_changes_into(&mut changes);
        assert!(
            changes.iter().any(|change| change.entity_id == 10),
            "dynamic bodies that move must emit transform changes"
        );
        assert!(
            changes.iter().any(|change| change.entity_id == 30),
            "kinematic bodies that move must emit transform changes"
        );
        assert!(
            !changes.iter().any(|change| change.entity_id == 20),
            "static bodies must keep their cached transform without move events"
        );
        assert_eq!(
            changes
                .iter()
                .rev()
                .find(|change| change.entity_id == 10)
                .map(|change| change.position),
            Some(moving_position)
        );
    }

    #[test]
    fn body_move_events_preserve_final_transform_when_body_goes_to_sleep() {
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

        let body = physics.body(8).unwrap();
        let final_position = physics.world.body(body).unwrap().position().unwrap();
        assert_eq!(
            physics.positions_ref().get(&8),
            Some(&[final_position.x, final_position.y]),
            "the cached position must include the final move event when Box2D puts the body to sleep"
        );
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
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].position, [7.0, -3.0]);
        assert_eq!(changes[1].position, [8.0, -4.0]);

        physics.set_transform(5, [9.0, -5.0]).unwrap();
        physics.destroy_body(5).unwrap();
        assert!(!physics.positions_ref().contains_key(&5));
        physics.drain_position_changes_into(&mut changes);
        assert!(
            changes.is_empty(),
            "destroying a body must discard queued transform changes for that entity"
        );
    }

    #[test]
    fn positions_into_clones_the_current_move_event_cache() {
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
}
