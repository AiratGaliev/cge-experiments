use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use avian2d::prelude::{Gravity, PhysicsPlugins, SubstepCount};
use bevy_app::App;
use bevy_ecs::{
    component::Component,
    entity::Entity,
    prelude::{Query, Res, Resource, World},
    schedule::{IntoScheduleConfigs, ScheduleLabel},
};
use bevy_math::Vec2;
use bevy_time::Time;

use crate::physics::{NativePhysicsSchedule, PhysicsPositionChange, SOLVER_SUB_STEPS};

#[derive(ScheduleLabel, Clone, Debug, Eq, Hash, PartialEq)]
pub struct NativeRuntimeSchedule;

#[derive(Component, Clone, Copy, Debug)]
struct RuntimeEntityId(u32);

#[derive(Component, Clone, Copy, Debug)]
struct VisualTransform {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Component, Clone, Copy, Debug)]
struct Rectangle {
    color: [f32; 4],
}

#[derive(Component, Clone, Copy, Debug)]
struct Circle {
    radius: f32,
    color: [f32; 4],
}

#[derive(Component, Clone, Copy, Debug)]
struct PhysicsLink;

#[derive(Component, Clone, Debug)]
struct TextLabel {
    value: String,
    scale: f32,
    color: [f32; 4],
}

#[derive(Resource, Default)]
struct PendingScriptChanges {
    transforms: HashMap<u32, [f32; 2]>,
    text: HashMap<u32, String>,
}

fn apply_script_transforms(
    pending: Res<PendingScriptChanges>,
    mut query: Query<(&RuntimeEntityId, &mut VisualTransform)>,
) {
    for (entity_id, mut transform) in &mut query {
        if let Some(position) = pending.transforms.get(&entity_id.0) {
            transform.x = position[0];
            transform.y = position[1];
        }
    }
}

fn apply_script_text(
    pending: Res<PendingScriptChanges>,
    mut query: Query<(&RuntimeEntityId, &mut TextLabel)>,
) {
    for (entity_id, mut label) in &mut query {
        if let Some(value) = pending.text.get(&entity_id.0) {
            label.value.clone_from(value);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawRectangle {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawCircle {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub color: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawText {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub color: [f32; 4],
    pub value: String,
}

pub type PositionMap = HashMap<u32, [f32; 2]>;

/// Scene and physics entities share one Bevy ECS world. bevy_app hosts only
/// the plugin/schedule lifecycle; no Bevy renderer, window or input plugins load.
pub struct EcsWorld {
    app: App,
    native_entities: HashMap<u32, Entity>,
    physics_sync_candidates: HashSet<u32>,
}

impl EcsWorld {
    pub fn new() -> Self {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default());
        app.insert_resource(Gravity(Vec2::ZERO));
        app.insert_resource(SubstepCount(SOLVER_SUB_STEPS as u32));
        app.insert_resource(PendingScriptChanges::default());
        app.add_plugins(PhysicsPlugins::new(NativePhysicsSchedule));
        app.add_systems(
            NativeRuntimeSchedule,
            (apply_script_transforms, apply_script_text).chain(),
        );

        Self {
            app,
            native_entities: HashMap::new(),
            physics_sync_candidates: HashSet::new(),
        }
    }

    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    pub fn create_rectangle(
        &mut self,
        entity_id: u32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: [f32; 4],
    ) -> Result<()> {
        self.ensure_available(entity_id)?;
        validate_rect(x, y, width, height, color)?;
        let entity = self.app.world_mut().spawn((
            RuntimeEntityId(entity_id),
            VisualTransform {
                x,
                y,
                width,
                height,
            },
            Rectangle { color },
        ));
        self.native_entities.insert(entity_id, entity.id());
        Ok(())
    }

    pub fn create_circle(
        &mut self,
        entity_id: u32,
        x: f32,
        y: f32,
        radius: f32,
        color: [f32; 4],
    ) -> Result<()> {
        self.ensure_available(entity_id)?;
        if !x.is_finite()
            || !y.is_finite()
            || !radius.is_finite()
            || radius <= 0.0
            || color.iter().any(|channel| !channel.is_finite())
        {
            bail!("invalid circle entity data");
        }
        let entity = self.app.world_mut().spawn((
            RuntimeEntityId(entity_id),
            VisualTransform {
                x,
                y,
                width: radius * 2.0,
                height: radius * 2.0,
            },
            Circle { radius, color },
        ));
        self.native_entities.insert(entity_id, entity.id());
        Ok(())
    }

    pub fn create_text(
        &mut self,
        entity_id: u32,
        value: String,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
    ) -> Result<()> {
        self.ensure_available(entity_id)?;
        if !x.is_finite()
            || !y.is_finite()
            || !scale.is_finite()
            || scale <= 0.0
            || value.len() > 256
            || color.iter().any(|channel| !channel.is_finite())
        {
            bail!("invalid text entity data");
        }
        let entity = self.app.world_mut().spawn((
            RuntimeEntityId(entity_id),
            VisualTransform {
                x,
                y,
                width: 0.0,
                height: 0.0,
            },
            TextLabel {
                value,
                scale,
                color,
            },
        ));
        self.native_entities.insert(entity_id, entity.id());
        Ok(())
    }

    fn ensure_available(&self, entity_id: u32) -> Result<()> {
        if self.native_entities.contains_key(&entity_id) {
            bail!("runtime entity {entity_id} already exists");
        }
        Ok(())
    }

    pub fn attach_physics(&mut self, entity_id: u32) -> Result<()> {
        let native_id = *self
            .native_entities
            .get(&entity_id)
            .ok_or_else(|| anyhow::anyhow!("unknown runtime entity {entity_id}"))?;
        self.app
            .world_mut()
            .entity_mut(native_id)
            .insert(PhysicsLink);
        Ok(())
    }

    pub fn detach_physics(&mut self, entity_id: u32) -> Result<()> {
        let native_id = *self
            .native_entities
            .get(&entity_id)
            .ok_or_else(|| anyhow::anyhow!("unknown runtime entity {entity_id}"))?;
        self.app
            .world_mut()
            .entity_mut(native_id)
            .remove::<PhysicsLink>();
        Ok(())
    }

    pub fn set_position(&mut self, entity_id: u32, x: f32, y: f32) -> Result<()> {
        if !self.native_entities.contains_key(&entity_id) {
            bail!("unknown runtime entity {entity_id}");
        }
        if !x.is_finite() || !y.is_finite() {
            bail!("entity position must be finite");
        }
        self.app
            .world_mut()
            .resource_mut::<PendingScriptChanges>()
            .transforms
            .insert(entity_id, [x, y]);
        Ok(())
    }

    pub fn set_text(&mut self, entity_id: u32, value: String) -> Result<()> {
        if !self.native_entities.contains_key(&entity_id) {
            bail!("unknown runtime entity {entity_id}");
        }
        if value.len() > 256 {
            bail!("text label exceeds 256 bytes");
        }
        self.app
            .world_mut()
            .resource_mut::<PendingScriptChanges>()
            .text
            .insert(entity_id, value);
        Ok(())
    }

    pub fn destroy_entity(&mut self, entity_id: u32) -> Result<()> {
        let native_id = self
            .native_entities
            .remove(&entity_id)
            .ok_or_else(|| anyhow::anyhow!("unknown runtime entity {entity_id}"))?;
        self.app.world_mut().despawn(native_id);
        let mut pending = self.app.world_mut().resource_mut::<PendingScriptChanges>();
        pending.transforms.remove(&entity_id);
        pending.text.remove(&entity_id);
        Ok(())
    }

    pub fn apply_pending_changes(&mut self) {
        let world = self.app.world_mut();
        world.run_schedule(NativeRuntimeSchedule);
        let mut pending = world.resource_mut::<PendingScriptChanges>();
        pending.transforms.clear();
        pending.text.clear();
    }

    pub fn synchronize_physics_changes(&mut self, changes: &[PhysicsPositionChange]) -> Result<()> {
        for change in changes {
            let Some(&native_id) = self.native_entities.get(&change.entity_id) else {
                continue;
            };
            let Some(mut transform) = self.app.world_mut().get_mut::<VisualTransform>(native_id)
            else {
                bail!(
                    "physics-linked ECS entity {} has no VisualTransform",
                    change.entity_id
                );
            };
            transform.x = change.position[0];
            transform.y = change.position[1];
        }
        Ok(())
    }

    pub fn synchronize_physics_adaptive(
        &mut self,
        changes: &[PhysicsPositionChange],
        positions: &PositionMap,
    ) -> Result<()> {
        if changes.is_empty() || positions.is_empty() {
            return Ok(());
        }
        if changes.len().saturating_mul(2) >= positions.len() {
            self.physics_sync_candidates.clear();
            self.physics_sync_candidates
                .extend(changes.iter().map(|change| change.entity_id));
            let updates = self
                .native_entities
                .iter()
                .filter_map(|(entity_id, entity)| {
                    if !self.physics_sync_candidates.contains(entity_id) {
                        return None;
                    }
                    positions
                        .get(entity_id)
                        .copied()
                        .map(|position| (*entity, position, *entity_id))
                })
                .collect::<Vec<_>>();
            for (entity, position, entity_id) in updates {
                let Some(mut transform) = self.app.world_mut().get_mut::<VisualTransform>(entity)
                else {
                    bail!("physics-linked ECS entity {entity_id} has no VisualTransform");
                };
                transform.x = position[0];
                transform.y = position[1];
            }
            self.physics_sync_candidates.clear();
            Ok(())
        } else {
            self.synchronize_physics_changes(changes)
        }
    }

    pub fn draw_rectangles(&mut self) -> Vec<DrawRectangle> {
        let mut output = Vec::new();
        self.draw_rectangles_into(&mut output);
        output
    }

    pub fn draw_rectangles_into(&mut self, output: &mut Vec<DrawRectangle>) {
        output.clear();
        let world = self.app.world_mut();
        let mut query = world.query::<(&VisualTransform, &Rectangle)>();
        for (transform, rectangle) in query.iter(world) {
            output.push(DrawRectangle {
                x: transform.x,
                y: transform.y,
                width: transform.width,
                height: transform.height,
                color: rectangle.color,
            });
        }
    }

    pub fn draw_circles(&mut self) -> Vec<DrawCircle> {
        let mut output = Vec::new();
        self.draw_circles_into(&mut output);
        output
    }

    pub fn draw_circles_into(&mut self, output: &mut Vec<DrawCircle>) {
        output.clear();
        let world = self.app.world_mut();
        let mut query = world.query::<(&VisualTransform, &Circle)>();
        for (transform, circle) in query.iter(world) {
            output.push(DrawCircle {
                x: transform.x,
                y: transform.y,
                radius: circle.radius,
                color: circle.color,
            });
        }
    }

    pub fn draw_text(&mut self) -> Vec<DrawText> {
        let mut output = Vec::new();
        self.draw_text_into(&mut output);
        output
    }

    pub fn draw_text_into(&mut self, output: &mut Vec<DrawText>) {
        output.clear();
        let world = self.app.world_mut();
        let mut query = world.query::<(&VisualTransform, &TextLabel)>();
        for (transform, label) in query.iter(world) {
            output.push(DrawText {
                x: transform.x,
                y: transform.y,
                scale: label.scale,
                color: label.color,
                value: label.value.clone(),
            });
        }
    }

    pub fn entity_count(&self) -> usize {
        self.native_entities.len()
    }

    #[cfg(test)]
    pub(crate) fn entity_has_physics_link(&mut self, entity_id: u32) -> bool {
        self.native_entities
            .get(&entity_id)
            .is_some_and(|entity| self.app.world().get::<PhysicsLink>(*entity).is_some())
    }
}

fn validate_rect(x: f32, y: f32, width: f32, height: f32, color: [f32; 4]) -> Result<()> {
    if !x.is_finite()
        || !y.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || color.iter().any(|channel| !channel.is_finite())
    {
        bail!("invalid rectangle entity data");
    }
    Ok(())
}

impl Default for EcsWorld {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::{PhysicsMaterial, PhysicsWorld, RigidBodyKind};

    #[test]
    fn bevy_ecs_owns_entities_and_produces_render_data() {
        let mut world = EcsWorld::new();
        world
            .create_rectangle(1, 0.0, -2.0, 1.0, 0.2, [0.2, 0.8, 1.0, 1.0])
            .unwrap();
        world
            .create_text(2, "LABEL 0".into(), -0.9, 0.9, 0.02, [1.0; 4])
            .unwrap();
        assert_eq!(world.entity_count(), 2);
        assert_eq!(world.draw_rectangles().len(), 1);
        assert_eq!(world.draw_text()[0].value, "LABEL 0");
    }

    #[test]
    fn scheduled_systems_apply_script_changes_in_order() {
        let mut world = EcsWorld::new();
        world
            .create_rectangle(7, 0.0, 0.0, 1.0, 1.0, [1.0; 4])
            .unwrap();
        world.set_position(7, -1.0, -2.0).unwrap();
        world.apply_pending_changes();
        let rectangle = world.draw_rectangles()[0];
        assert_eq!((rectangle.x, rectangle.y), (-1.0, -2.0));
    }

    #[test]
    fn physics_and_scene_share_one_ecs_world() {
        let mut world = EcsWorld::new();
        world
            .create_rectangle(1, 0.0, 0.0, 1.0, 1.0, [1.0; 4])
            .unwrap();
        world.attach_physics(1).unwrap();
        assert!(world.entity_has_physics_link(1));
        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics.configure_world(world.world_mut());
        physics
            .create_box(
                world.world_mut(),
                1,
                [0.0, 0.0],
                [1.0, 1.0],
                RigidBodyKind::Static,
                PhysicsMaterial::default(),
            )
            .unwrap();
        assert_eq!(world.entity_count(), 1);
        assert_eq!(physics.body_count(), 1);
    }

    #[test]
    fn detached_physics_body_leaves_visual_entity_alive_and_unlinked() {
        let mut scene = EcsWorld::new();
        scene.create_circle(8, 0.0, 0.0, 0.1, [1.0; 4]).unwrap();
        scene.attach_physics(8).unwrap();
        assert!(scene.entity_has_physics_link(8));

        scene.detach_physics(8).unwrap();

        assert!(!scene.entity_has_physics_link(8));
        assert_eq!(scene.entity_count(), 1);
        assert_eq!(scene.draw_circles().len(), 1);
    }

    #[test]
    fn avian_position_changes_are_synchronized_to_visual_entities() {
        let mut scene = EcsWorld::new();
        scene
            .create_circle(7, 0.0, 0.0, 0.1, [1.0, 1.0, 1.0, 1.0])
            .unwrap();
        scene.attach_physics(7).unwrap();

        let mut physics = PhysicsWorld::new([0.0, 0.0]).unwrap();
        physics.configure_world(scene.world_mut());
        physics
            .create_circle(
                scene.world_mut(),
                7,
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
            .set_linear_velocity(scene.world_mut(), 7, [2.0, 0.0])
            .unwrap();

        let mut collisions = Vec::new();
        physics
            .step_into(scene.world_mut(), 1.0 / 60.0, &mut collisions)
            .unwrap();
        let mut changes = Vec::new();
        physics.drain_position_changes_into(&mut changes);
        scene
            .synchronize_physics_adaptive(&changes, physics.positions_ref())
            .unwrap();

        let rendered = scene.draw_circles();
        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].x > 0.0);
        assert!(rendered[0].y.abs() < 0.0001);
    }

    #[test]
    fn destroying_entity_releases_its_bevy_ecs_record() {
        let mut world = EcsWorld::new();
        world
            .create_rectangle(10, 0.0, 0.0, 1.0, 1.0, [1.0; 4])
            .unwrap();
        world.destroy_entity(10).unwrap();
        assert_eq!(world.entity_count(), 0);
        assert!(world.draw_rectangles().is_empty());
    }

    #[test]
    fn invalid_render_data_is_rejected() {
        let mut world = EcsWorld::new();
        assert!(
            world
                .create_rectangle(1, 0.0, 0.0, 0.0, 1.0, [1.0; 4])
                .is_err()
        );
        assert!(
            world
                .create_circle(1, 0.0, 0.0, f32::NAN, [1.0; 4])
                .is_err()
        );
    }

    #[test]
    fn reusable_draw_output_keeps_capacity_between_frames() {
        let mut world = EcsWorld::new();
        world
            .create_rectangle(1, 0.0, 0.0, 1.0, 1.0, [1.0, 0.0, 0.0, 1.0])
            .unwrap();
        let mut output = Vec::with_capacity(4);
        world.draw_rectangles_into(&mut output);
        let capacity = output.capacity();
        world.set_position(1, 2.0, 3.0).unwrap();
        world.apply_pending_changes();
        world.draw_rectangles_into(&mut output);
        assert_eq!((output[0].x, output[0].y), (2.0, 3.0));
        assert_eq!(output.capacity(), capacity);
    }
}
