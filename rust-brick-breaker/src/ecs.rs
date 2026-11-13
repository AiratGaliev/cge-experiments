use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{Result, bail};
use flecs_ecs::prelude::*;

#[derive(Component, Clone, Copy)]
struct RuntimeEntityId(u32);

#[derive(Component, Clone, Copy)]
struct Transform {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Component, Clone, Copy)]
struct Rectangle {
    color: [f32; 4],
}

#[derive(Component, Clone, Copy)]
struct Circle {
    radius: f32,
    color: [f32; 4],
}

#[derive(Component, Clone, Copy)]
struct PhysicsLink {
    entity_id: u32,
}

#[derive(Component, Clone)]
struct TextLabel {
    value: String,
    scale: f32,
    color: [f32; 4],
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

type PositionMap = HashMap<u32, [f32; 2]>;

pub struct EcsWorld {
    world: World,
    native_entities: HashMap<u32, u64>,
    physics_positions: Arc<Mutex<PositionMap>>,
    pending_transforms: Arc<Mutex<PositionMap>>,
    pending_text: Arc<Mutex<HashMap<u32, String>>>,
}

impl EcsWorld {
    pub fn new() -> Self {
        let world = World::new();
        world.component::<RuntimeEntityId>();
        world.component::<Transform>();
        world.component::<Rectangle>();
        world.component::<Circle>();
        world.component::<PhysicsLink>();
        world.component::<TextLabel>();

        let physics_positions = Arc::new(Mutex::new(PositionMap::new()));
        let pending_transforms = Arc::new(Mutex::new(PositionMap::new()));
        let pending_text = Arc::new(Mutex::new(HashMap::<u32, String>::new()));

        let positions = Arc::clone(&physics_positions);
        world
            .system_named::<(&mut Transform, &PhysicsLink)>("SynchronizePhysicsTransforms")
            .each(move |(transform, link)| {
                let snapshot = positions
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(position) = snapshot.get(&link.entity_id) {
                    transform.x = position[0];
                    transform.y = position[1];
                }
            });

        let transforms = Arc::clone(&pending_transforms);
        world
            .system_named::<(&mut Transform, &RuntimeEntityId)>("ApplyScriptTransforms")
            .each(move |(transform, entity)| {
                let pending = transforms
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(position) = pending.get(&entity.0) {
                    transform.x = position[0];
                    transform.y = position[1];
                }
            });

        let text_updates = Arc::clone(&pending_text);
        world
            .system_named::<(&mut TextLabel, &RuntimeEntityId)>("ApplyScriptText")
            .each(move |(label, entity)| {
                let pending = text_updates
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(value) = pending.get(&entity.0) {
                    label.value.clone_from(value);
                }
            });

        Self {
            world,
            native_entities: HashMap::new(),
            physics_positions,
            pending_transforms,
            pending_text,
        }
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
        if self.native_entities.contains_key(&entity_id) {
            bail!("runtime entity {entity_id} already exists");
        }
        validate_rect(x, y, width, height, color)?;

        let entity = self.world.entity();
        let native_id = u64::from(entity.id());
        entity
            .set(RuntimeEntityId(entity_id))
            .set(Transform {
                x,
                y,
                width,
                height,
            })
            .set(Rectangle { color });
        self.native_entities.insert(entity_id, native_id);
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
        if self.native_entities.contains_key(&entity_id) {
            bail!("runtime entity {entity_id} already exists");
        }
        if !x.is_finite()
            || !y.is_finite()
            || !radius.is_finite()
            || radius <= 0.0
            || color.iter().any(|channel| !channel.is_finite())
        {
            bail!("invalid circle entity data");
        }

        let entity = self.world.entity();
        let native_id = u64::from(entity.id());
        entity
            .set(RuntimeEntityId(entity_id))
            .set(Transform {
                x,
                y,
                width: radius * 2.0,
                height: radius * 2.0,
            })
            .set(Circle { radius, color });
        self.native_entities.insert(entity_id, native_id);
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
        if self.native_entities.contains_key(&entity_id) {
            bail!("runtime entity {entity_id} already exists");
        }
        if !x.is_finite()
            || !y.is_finite()
            || !scale.is_finite()
            || scale <= 0.0
            || value.len() > 256
            || color.iter().any(|channel| !channel.is_finite())
        {
            bail!("invalid text entity data");
        }

        let entity = self.world.entity();
        let native_id = u64::from(entity.id());
        entity
            .set(RuntimeEntityId(entity_id))
            .set(Transform {
                x,
                y,
                width: 0.0,
                height: 0.0,
            })
            .set(TextLabel {
                value,
                scale,
                color,
            });
        self.native_entities.insert(entity_id, native_id);
        Ok(())
    }

    pub fn attach_physics(&mut self, entity_id: u32) -> Result<()> {
        let native_id = *self
            .native_entities
            .get(&entity_id)
            .ok_or_else(|| anyhow::anyhow!("unknown runtime entity {entity_id}"))?;
        self.world
            .entity_from_id(native_id)
            .set(PhysicsLink { entity_id });
        Ok(())
    }

    pub fn set_position(&mut self, entity_id: u32, x: f32, y: f32) -> Result<()> {
        if !self.native_entities.contains_key(&entity_id) {
            bail!("unknown runtime entity {entity_id}");
        }
        if !x.is_finite() || !y.is_finite() {
            bail!("entity position must be finite");
        }
        self.pending_transforms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
        self.pending_text
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(entity_id, value);
        Ok(())
    }

    pub fn destroy_entity(&mut self, entity_id: u32) -> Result<()> {
        let native_id = self
            .native_entities
            .remove(&entity_id)
            .ok_or_else(|| anyhow::anyhow!("unknown runtime entity {entity_id}"))?;
        self.world.entity_from_id(native_id).destruct();
        self.pending_transforms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&entity_id);
        self.pending_text
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&entity_id);
        self.physics_positions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&entity_id);
        Ok(())
    }

    pub fn apply_pending_changes(&mut self) {
        let _ = self.world.progress();
        self.pending_transforms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.pending_text
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    pub fn synchronize_physics(&mut self, positions: &PositionMap) {
        {
            let mut snapshot = self
                .physics_positions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            snapshot.clear();
            snapshot.extend(positions.iter().map(|(id, position)| (*id, *position)));
        }
        let _ = self.world.progress();
    }

    pub fn draw_rectangles(&self) -> Vec<DrawRectangle> {
        let mut items = Vec::with_capacity(self.native_entities.len());
        self.draw_rectangles_into(&mut items);
        items
    }

    pub fn draw_rectangles_into(&self, items: &mut Vec<DrawRectangle>) {
        items.clear();
        self.world
            .new_query::<(&Transform, &Rectangle)>()
            .each(|(transform, rectangle)| {
                items.push(DrawRectangle {
                    x: transform.x,
                    y: transform.y,
                    width: transform.width,
                    height: transform.height,
                    color: rectangle.color,
                });
            });
    }

    pub fn draw_circles(&self) -> Vec<DrawCircle> {
        let mut items = Vec::new();
        self.draw_circles_into(&mut items);
        items
    }

    pub fn draw_circles_into(&self, items: &mut Vec<DrawCircle>) {
        items.clear();
        self.world
            .new_query::<(&Transform, &Circle)>()
            .each(|(transform, circle)| {
                items.push(DrawCircle {
                    x: transform.x,
                    y: transform.y,
                    radius: circle.radius,
                    color: circle.color,
                });
            });
    }

    pub fn draw_text(&self) -> Vec<DrawText> {
        let mut items = Vec::new();
        self.draw_text_into(&mut items);
        items
    }

    pub fn draw_text_into(&self, items: &mut Vec<DrawText>) {
        let mut index = 0;
        self.world
            .new_query::<(&Transform, &TextLabel)>()
            .each(|(transform, label)| {
                if let Some(item) = items.get_mut(index) {
                    item.x = transform.x;
                    item.y = transform.y;
                    item.scale = label.scale;
                    item.color = label.color;
                    item.value.clone_from(&label.value);
                } else {
                    items.push(DrawText {
                        x: transform.x,
                        y: transform.y,
                        scale: label.scale,
                        color: label.color,
                        value: label.value.clone(),
                    });
                }
                index += 1;
            });
        items.truncate(index);
    }

    pub fn entity_count(&self) -> usize {
        self.native_entities.len()
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

    #[test]
    fn flecs_owns_runtime_entities_and_produces_render_data() {
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
    fn flecs_systems_apply_script_changes_and_physics_snapshots() {
        let mut world = EcsWorld::new();
        world
            .create_rectangle(7, 0.0, 0.0, 1.0, 1.0, [1.0; 4])
            .unwrap();
        world.attach_physics(7).unwrap();

        world.synchronize_physics(&HashMap::from([(7, [2.0, 3.0])]));
        assert_eq!(world.draw_rectangles()[0].x, 2.0);
        assert_eq!(world.draw_rectangles()[0].y, 3.0);

        world.set_position(7, -1.0, -2.0).unwrap();
        world.apply_pending_changes();
        assert_eq!(world.draw_rectangles()[0].x, -1.0);
        assert_eq!(world.draw_rectangles()[0].y, -2.0);
    }

    #[test]
    fn destroying_an_entity_releases_its_flecs_record() {
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
    }

    #[test]
    fn reusable_draw_output_keeps_capacity_between_frames() {
        let mut world = EcsWorld::new();
        world
            .create_rectangle(1, 0.0, 0.0, 1.0, 1.0, [1.0, 0.0, 0.0, 1.0])
            .unwrap();
        let mut output = Vec::with_capacity(4);

        world.draw_rectangles_into(&mut output);
        assert_eq!(output.len(), 1);
        let capacity = output.capacity();

        world.set_position(1, 2.0, 3.0).unwrap();
        world.apply_pending_changes();
        world.draw_rectangles_into(&mut output);

        assert_eq!(output.len(), 1);
        assert_eq!(output[0].x, 2.0);
        assert_eq!(output[0].y, 3.0);
        assert_eq!(output.capacity(), capacity);
    }
}
