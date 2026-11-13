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

pub struct PhysicsWorld {
    _foundation: &'static Foundation,
    world: Box2dWorld,
    bodies: HashMap<u32, BodyId>,
    body_shapes: HashMap<u32, ShapeId>,
    shape_entities: HashMap<ShapeId, u32>,
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
            body_shapes: HashMap::new(),
            shape_entities: HashMap::new(),
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
        self.register_body(entity_id, body, shape);
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
        self.register_body(entity_id, body, shape);
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

    fn register_body(&mut self, entity_id: u32, body: BodyId, shape: ShapeId) {
        self.bodies.insert(entity_id, body);
        self.body_shapes.insert(entity_id, shape);
        self.shape_entities.insert(shape, entity_id);
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
        Ok(())
    }

    pub fn destroy_body(&mut self, entity_id: u32) -> Result<()> {
        let body = self.body(entity_id)?;
        self.world
            .body(body)?
            .destroy()
            .context("destroy Box2D body")?;
        self.bodies.remove(&entity_id);
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
        if !delta_seconds.is_finite() || delta_seconds <= 0.0 {
            bail!("physics delta must be finite and positive");
        }
        let shape_pairs: Vec<(ShapeId, ShapeId)> = {
            let completed = self
                .world
                .step(delta_seconds, SOLVER_SUB_STEPS)
                .context("step Box2D v3 world")?;
            let contacts = completed
                .contact_events()
                .context("read Box2D v3 contact events")?;
            contacts
                .begin()
                .iter()
                .map(|event| (event.shape_a, event.shape_b))
                .collect()
        };

        let mut unique = HashSet::new();
        let mut collisions = Vec::new();
        for (shape_a, shape_b) in shape_pairs {
            let (Some(&entity_a), Some(&entity_b)) = (
                self.shape_entities.get(&shape_a),
                self.shape_entities.get(&shape_b),
            ) else {
                continue;
            };
            let pair = if entity_a <= entity_b {
                (entity_a, entity_b)
            } else {
                (entity_b, entity_a)
            };
            if unique.insert(pair) {
                collisions.push(CollisionEvent {
                    entity_a: pair.0,
                    entity_b: pair.1,
                });
            }
        }
        Ok(collisions)
    }

    pub fn positions(&mut self) -> Result<PhysicsPositions> {
        let mut result = PhysicsPositions::with_capacity(self.bodies.len());
        for (entity_id, body) in &self.bodies {
            let position = self.world.body(*body)?.position()?;
            result.insert(*entity_id, [position.x, position.y]);
        }
        Ok(result)
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
}
