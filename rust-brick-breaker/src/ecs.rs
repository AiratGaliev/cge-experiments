use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use anyhow::Result;
use flecs_ecs::prelude::*;

use crate::physics::{BRICK_COUNT, PhysicsPositions};

const PADDLE_SPEED: f32 = 7.0;
const PADDLE_LIMIT: f32 = 3.75;

#[derive(Component, Clone, Copy)]
struct Transform {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Component, Clone, Copy)]
struct Renderable {
    color: [f32; 4],
    visible: bool,
}

#[derive(Component, Clone, Copy)]
struct PhysicsLink {
    slot: u32,
}

#[derive(Component, Clone, Copy)]
struct PlayerControlled;

#[derive(Component, Clone, Copy)]
struct Brick {
    index: usize,
}

#[derive(Clone, Copy, Default)]
struct InputFrame {
    horizontal: f32,
    delta_seconds: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct DrawItem {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: [f32; 4],
}

pub struct EcsScene {
    world: World,
    input: Arc<Mutex<InputFrame>>,
    positions: Arc<Mutex<HashMap<u32, [f32; 2]>>>,
    removed_bricks: Arc<Mutex<HashSet<usize>>>,
}

impl EcsScene {
    pub fn new(initial_positions: &PhysicsPositions) -> Result<Self> {
        let world = World::new();
        world.component::<Transform>();
        world.component::<Renderable>();
        world.component::<PhysicsLink>();
        world.component::<PlayerControlled>();
        world.component::<Brick>();

        let input = Arc::new(Mutex::new(InputFrame::default()));
        let positions = Arc::new(Mutex::new(initial_positions.clone()));
        let removed_bricks = Arc::new(Mutex::new(HashSet::new()));

        let sync_positions = Arc::clone(&positions);
        world
            .system_named::<(&mut Transform, &PhysicsLink)>("SyncPhysicsTransforms")
            .each(move |(transform, link)| {
                let snapshot = sync_positions
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(position) = snapshot.get(&link.slot) {
                    transform.x = position[0];
                    transform.y = position[1];
                }
            });

        let input_frame = Arc::clone(&input);
        world
            .system_named::<(&mut Transform, &PlayerControlled)>("ApplyPaddleInput")
            .each(move |(transform, _control)| {
                let frame = input_frame
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                transform.x = (transform.x + frame.horizontal * PADDLE_SPEED * frame.delta_seconds)
                    .clamp(-PADDLE_LIMIT, PADDLE_LIMIT);
            });

        let hidden = Arc::clone(&removed_bricks);
        world
            .system_named::<(&mut Renderable, &Brick)>("HideDestroyedBricks")
            .each(move |(renderable, brick)| {
                let removed = hidden
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                renderable.visible = !removed.contains(&brick.index);
            });

        world
            .entity()
            .set(Transform {
                x: 0.0,
                y: 0.65,
                width: 1.40,
                height: 0.24,
            })
            .set(Renderable {
                color: [0.30, 0.82, 1.0, 1.0],
                visible: true,
            })
            .set(PlayerControlled);

        world
            .entity()
            .set(Transform {
                x: 0.0,
                y: 0.95,
                width: 0.24,
                height: 0.24,
            })
            .set(Renderable {
                color: [1.0, 0.94, 0.72, 1.0],
                visible: true,
            })
            .set(PhysicsLink { slot: 1 });

        for index in 0..BRICK_COUNT {
            let column = index % 9;
            let row = index / 9;
            let x = (column as f32 - 4.0) * 1.04;
            let y = 4.35 + row as f32 * 0.48;
            let color = match row {
                0 => [1.0, 0.40, 0.43, 1.0],
                1 => [1.0, 0.66, 0.31, 1.0],
                2 => [0.78, 0.60, 1.0, 1.0],
                _ => [0.42, 0.86, 0.62, 1.0],
            };

            world
                .entity()
                .set(Transform {
                    x,
                    y,
                    width: 0.92,
                    height: 0.34,
                })
                .set(Renderable {
                    color,
                    visible: true,
                })
                .set(PhysicsLink {
                    slot: (index + 2) as u32,
                })
                .set(Brick { index });
        }

        Ok(Self {
            world,
            input,
            positions,
            removed_bricks,
        })
    }

    pub fn progress(&self, horizontal: f32, delta_seconds: f32) {
        {
            let mut frame = self
                .input
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            frame.horizontal = horizontal.clamp(-1.0, 1.0);
            frame.delta_seconds = delta_seconds.max(0.0);
        }
        let _ = self.world.progress();
    }

    pub fn set_physics_positions(&self, snapshot: &PhysicsPositions) {
        {
            let mut positions = self
                .positions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            positions.clear();
            positions.extend(snapshot.iter().map(|(slot, position)| (*slot, *position)));
        }
        {
            let mut frame = self
                .input
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            frame.delta_seconds = 0.0;
        }
        let _ = self.world.progress();
    }

    pub fn mark_bricks_destroyed(&self, indices: &[usize]) {
        let mut removed = self
            .removed_bricks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        removed.extend(indices.iter().copied());
    }

    pub fn reset_bricks(&self) {
        self.removed_bricks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        {
            let mut frame = self
                .input
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            frame.delta_seconds = 0.0;
        }
        let _ = self.world.progress();
    }

    pub fn reset_paddle(&self) {
        self.world
            .new_query::<(&mut Transform, &PlayerControlled)>()
            .each(|(transform, _control)| {
                transform.x = 0.0;
                transform.y = 0.65;
            });
    }

    pub fn paddle_x(&self) -> f32 {
        let mut x = 0.0;
        self.world
            .new_query::<(&Transform, &PlayerControlled)>()
            .each(|(transform, _control)| x = transform.x);
        x
    }

    pub fn draw_items(&self) -> Vec<DrawItem> {
        let mut items = Vec::with_capacity(BRICK_COUNT + 2);
        self.world
            .new_query::<(&Transform, &Renderable)>()
            .each(|(transform, renderable)| {
                if renderable.visible {
                    items.push(DrawItem {
                        x: transform.x,
                        y: transform.y,
                        width: transform.width,
                        height: transform.height,
                        color: renderable.color,
                    });
                }
            });
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flecs_system_moves_the_paddle_and_collects_renderables() {
        let positions = HashMap::from([(1, [0.0, 0.95])]);
        let scene = EcsScene::new(&positions).expect("create ECS world");

        scene.progress(1.0, 1.0);

        assert!(scene.paddle_x() > 3.5);
        assert_eq!(scene.draw_items().len(), BRICK_COUNT + 2);
    }

    #[test]
    fn destroyed_bricks_are_hidden_by_a_flecs_system() {
        let positions = HashMap::from([(1, [0.0, 0.95])]);
        let scene = EcsScene::new(&positions).expect("create ECS world");

        scene.mark_bricks_destroyed(&[0, 2, 4]);
        scene.set_physics_positions(&positions);

        assert_eq!(scene.draw_items().len(), BRICK_COUNT - 1);
    }
}
