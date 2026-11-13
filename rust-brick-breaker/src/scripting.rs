use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context as _, Result};
use crossbeam_queue::ArrayQueue;
use rquickjs::prelude::This;
use rquickjs::{Array, Context, Function, Object, Persistent, Runtime};

use crate::{
    audio::enqueue_tone,
    physics::{CollisionEvent, PhysicsMaterial, RigidBodyKind},
};

#[derive(Debug)]
pub enum RuntimeCommand {
    CreateRectangle {
        entity_id: u32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: [f32; 4],
    },
    CreateCircle {
        entity_id: u32,
        x: f32,
        y: f32,
        radius: f32,
        color: [f32; 4],
    },
    CreateText {
        entity_id: u32,
        value: String,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
    },
    SetPosition {
        entity_id: u32,
        x: f32,
        y: f32,
    },
    SetText {
        entity_id: u32,
        value: String,
    },
    DestroyEntity {
        entity_id: u32,
    },
    CreateBoxBody {
        entity_id: u32,
        position: [f32; 2],
        size: [f32; 2],
        body_type: RigidBodyKind,
        material: PhysicsMaterial,
    },
    CreateCircleBody {
        entity_id: u32,
        position: [f32; 2],
        radius: f32,
        body_type: RigidBodyKind,
        material: PhysicsMaterial,
    },
    SetLinearVelocity {
        entity_id: u32,
        velocity: [f32; 2],
    },
    SetBodyPosition {
        entity_id: u32,
        position: [f32; 2],
    },
    DestroyBody {
        entity_id: u32,
    },
}

#[derive(Default)]
struct HostFrame {
    delta_seconds: f32,
    keys_down: HashSet<String>,
    keys_pressed: HashSet<String>,
    positions: HashMap<u32, [f32; 2]>,
    collisions: Vec<CollisionEvent>,
}

#[derive(Default)]
struct HostState {
    next_entity_id: u32,
    commands: Vec<RuntimeCommand>,
    frame: HostFrame,
}

#[derive(Clone, Copy)]
enum GameHook {
    Initialize,
    Update,
    Shutdown,
}

impl GameHook {
    fn name(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::Update => "update",
            Self::Shutdown => "shutdown",
        }
    }
}

#[derive(Default)]
struct GameHooks {
    game: Option<Persistent<Object<'static>>>,
    engine: Option<Persistent<Object<'static>>>,
    initialize: Option<Persistent<Function<'static>>>,
    update: Option<Persistent<Function<'static>>>,
    shutdown: Option<Persistent<Function<'static>>>,
}

impl GameHooks {
    fn function(&self, hook: GameHook) -> Option<&Persistent<Function<'static>>> {
        match hook {
            GameHook::Initialize => self.initialize.as_ref(),
            GameHook::Update => self.update.as_ref(),
            GameHook::Shutdown => self.shutdown.as_ref(),
        }
    }
}

pub struct ScriptEngine {
    // Persistent JS values must be dropped before Context and Runtime.
    hooks: GameHooks,
    context: Context,
    _runtime: Runtime,
    host: Arc<Mutex<HostState>>,
}

impl ScriptEngine {
    pub fn new(script_path: &Path, audio_queue: Arc<ArrayQueue<f32>>) -> Result<Self> {
        let source = fs::read_to_string(script_path)
            .with_context(|| format!("read game script {}", script_path.display()))?;
        Self::from_source(&source, audio_queue)
    }

    pub fn from_source(source: &str, audio_queue: Arc<ArrayQueue<f32>>) -> Result<Self> {
        let runtime = Runtime::new().context("create QuickJS-NG runtime")?;
        let context = Context::full(&runtime).context("create QuickJS-NG context")?;
        let host = Arc::new(Mutex::new(HostState {
            next_entity_id: 1,
            ..HostState::default()
        }));

        let hooks = context
            .with(|ctx| -> rquickjs::Result<GameHooks> {
                let engine = Object::new(ctx.clone())?;

                let entities = Object::new(ctx.clone())?;
                {
                    let state = Arc::clone(&host);
                    entities.set(
                        "createRectangle",
                        Function::new(
                            ctx.clone(),
                            move |config: Object<'_>| -> rquickjs::Result<u32> {
                                let x = config.get::<_, f32>("x")?;
                                let y = config.get::<_, f32>("y")?;
                                let width = config.get::<_, f32>("width")?;
                                let height = config.get::<_, f32>("height")?;
                                let color = [
                                    config.get::<_, f32>("r")?,
                                    config.get::<_, f32>("g")?,
                                    config.get::<_, f32>("b")?,
                                    config.get::<_, f32>("a")?,
                                ];
                                let mut state = lock(&state);
                                let entity_id = allocate_entity_id(&mut state)?;
                                state.commands.push(RuntimeCommand::CreateRectangle {
                                    entity_id,
                                    x,
                                    y,
                                    width,
                                    height,
                                    color,
                                });
                                Ok(entity_id)
                            },
                        )?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    entities.set(
                        "createCircle",
                        Function::new(
                            ctx.clone(),
                            move |config: Object<'_>| -> rquickjs::Result<u32> {
                                let x = config.get::<_, f32>("x")?;
                                let y = config.get::<_, f32>("y")?;
                                let radius = config.get::<_, f32>("radius")?;
                                let color = [
                                    config.get::<_, f32>("r")?,
                                    config.get::<_, f32>("g")?,
                                    config.get::<_, f32>("b")?,
                                    config.get::<_, f32>("a")?,
                                ];
                                let mut state = lock(&state);
                                let entity_id = allocate_entity_id(&mut state)?;
                                state.commands.push(RuntimeCommand::CreateCircle {
                                    entity_id,
                                    x,
                                    y,
                                    radius,
                                    color,
                                });
                                Ok(entity_id)
                            },
                        )?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    entities.set(
                        "createText",
                        Function::new(
                            ctx.clone(),
                            move |config: Object<'_>| -> rquickjs::Result<u32> {
                                let value = config.get::<_, String>("value")?;
                                let x = config.get::<_, f32>("x")?;
                                let y = config.get::<_, f32>("y")?;
                                let scale = config.get::<_, f32>("scale")?;
                                let color = [
                                    config.get::<_, f32>("r")?,
                                    config.get::<_, f32>("g")?,
                                    config.get::<_, f32>("b")?,
                                    config.get::<_, f32>("a")?,
                                ];
                                let mut state = lock(&state);
                                let entity_id = allocate_entity_id(&mut state)?;
                                state.commands.push(RuntimeCommand::CreateText {
                                    entity_id,
                                    value,
                                    x,
                                    y,
                                    scale,
                                    color,
                                });
                                Ok(entity_id)
                            },
                        )?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    entities.set(
                        "setPosition",
                        Function::new(ctx.clone(), move |entity_id: u32, x: f32, y: f32| {
                            lock(&state).commands.push(RuntimeCommand::SetPosition {
                                entity_id,
                                x,
                                y,
                            });
                        })?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    entities.set(
                        "setText",
                        Function::new(ctx.clone(), move |entity_id: u32, value: String| {
                            lock(&state)
                                .commands
                                .push(RuntimeCommand::SetText { entity_id, value });
                        })?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    entities.set(
                        "destroy",
                        Function::new(ctx.clone(), move |entity_id: u32| {
                            lock(&state)
                                .commands
                                .push(RuntimeCommand::DestroyEntity { entity_id });
                        })?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    entities.set(
                        "getPositionX",
                        Function::new(ctx.clone(), move |entity_id: u32| {
                            lock(&state)
                                .frame
                                .positions
                                .get(&entity_id)
                                .map(|position| position[0])
                        })?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    entities.set(
                        "getPositionY",
                        Function::new(ctx.clone(), move |entity_id: u32| {
                            lock(&state)
                                .frame
                                .positions
                                .get(&entity_id)
                                .map(|position| position[1])
                        })?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    entities.set(
                        "readPositions",
                        Function::new(
                            ctx.clone(),
                            move |entity_ids: Array<'_>, output: Object<'_>| -> rquickjs::Result<()> {
                                let required_len = entity_ids.len() * 2;
                                let output_len = output.get::<_, usize>("length")?;
                                if !output.is_typed_array::<f32>() || output_len != required_len {
                                    return Err(rquickjs::Error::new_from_js(
                                        "Float32Array",
                                        "output must be a Float32Array with entityIds.length * 2 elements",
                                    ));
                                }
                                let state = lock(&state);
                                for (index, entity_id) in entity_ids.iter::<u32>().enumerate() {
                                    let position = state
                                        .frame
                                        .positions
                                        .get(&entity_id?)
                                        .copied()
                                        .unwrap_or([0.0, 0.0]);
                                    output.set((index * 2) as u32, position[0])?;
                                    output.set((index * 2 + 1) as u32, position[1])?;
                                }
                                Ok(())
                            },
                        )?,
                    )?;
                }
                engine.set("entities", entities)?;

                let physics = Object::new(ctx.clone())?;
                {
                    let state = Arc::clone(&host);
                    physics.set(
                        "createBox",
                        Function::new(
                            ctx.clone(),
                            move |config: Object<'_>| -> rquickjs::Result<()> {
                                let entity_id = config.get::<_, u32>("entityId")?;
                                let x = config.get::<_, f32>("x")?;
                                let y = config.get::<_, f32>("y")?;
                                let width = config.get::<_, f32>("width")?;
                                let height = config.get::<_, f32>("height")?;
                                let body_type =
                                    parse_body_type(&config.get::<_, String>("bodyType")?)?;
                                let material = PhysicsMaterial {
                                    density: config.get::<_, f32>("density")?,
                                    friction: config.get::<_, f32>("friction")?,
                                    restitution: config.get::<_, f32>("restitution")?,
                                    gravity_scale: config.get::<_, f32>("gravityScale")?,
                                    bullet: config.get::<_, bool>("bullet")?,
                                };
                                lock(&state).commands.push(RuntimeCommand::CreateBoxBody {
                                    entity_id,
                                    position: [x, y],
                                    size: [width, height],
                                    body_type,
                                    material,
                                });
                                Ok(())
                            },
                        )?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    physics.set(
                        "createCircle",
                        Function::new(
                            ctx.clone(),
                            move |config: Object<'_>| -> rquickjs::Result<()> {
                                let entity_id = config.get::<_, u32>("entityId")?;
                                let x = config.get::<_, f32>("x")?;
                                let y = config.get::<_, f32>("y")?;
                                let radius = config.get::<_, f32>("radius")?;
                                let body_type =
                                    parse_body_type(&config.get::<_, String>("bodyType")?)?;
                                let material = PhysicsMaterial {
                                    density: config.get::<_, f32>("density")?,
                                    friction: config.get::<_, f32>("friction")?,
                                    restitution: config.get::<_, f32>("restitution")?,
                                    gravity_scale: config.get::<_, f32>("gravityScale")?,
                                    bullet: config.get::<_, bool>("bullet")?,
                                };
                                lock(&state)
                                    .commands
                                    .push(RuntimeCommand::CreateCircleBody {
                                        entity_id,
                                        position: [x, y],
                                        radius,
                                        body_type,
                                        material,
                                    });
                                Ok(())
                            },
                        )?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    physics.set(
                        "setLinearVelocity",
                        Function::new(ctx.clone(), move |entity_id: u32, x: f32, y: f32| {
                            lock(&state)
                                .commands
                                .push(RuntimeCommand::SetLinearVelocity {
                                    entity_id,
                                    velocity: [x, y],
                                });
                        })?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    physics.set(
                        "setPosition",
                        Function::new(ctx.clone(), move |entity_id: u32, x: f32, y: f32| {
                            lock(&state).commands.push(RuntimeCommand::SetBodyPosition {
                                entity_id,
                                position: [x, y],
                            });
                        })?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    physics.set(
                        "destroyBody",
                        Function::new(ctx.clone(), move |entity_id: u32| {
                            lock(&state)
                                .commands
                                .push(RuntimeCommand::DestroyBody { entity_id });
                        })?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    physics.set(
                        "collisionCount",
                        Function::new(ctx.clone(), move || {
                            lock(&state).frame.collisions.len() as u32
                        })?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    physics.set(
                        "collisionEntityA",
                        Function::new(ctx.clone(), move |index: u32| {
                            lock(&state)
                                .frame
                                .collisions
                                .get(index as usize)
                                .map(|event| event.entity_a)
                        })?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    physics.set(
                        "collisionEntityB",
                        Function::new(ctx.clone(), move |index: u32| {
                            lock(&state)
                                .frame
                                .collisions
                                .get(index as usize)
                                .map(|event| event.entity_b)
                        })?,
                    )?;
                }
                engine.set("physics", physics)?;

                let input = Object::new(ctx.clone())?;
                {
                    let state = Arc::clone(&host);
                    input.set(
                        "isDown",
                        Function::new(ctx.clone(), move |key: String| {
                            lock(&state).frame.keys_down.contains(&key)
                        })?,
                    )?;
                }
                {
                    let state = Arc::clone(&host);
                    input.set(
                        "wasPressed",
                        Function::new(ctx.clone(), move |key: String| {
                            lock(&state).frame.keys_pressed.contains(&key)
                        })?,
                    )?;
                }
                engine.set("input", input)?;

                let time = Object::new(ctx.clone())?;
                {
                    let state = Arc::clone(&host);
                    time.set(
                        "deltaSeconds",
                        Function::new(ctx.clone(), move || lock(&state).frame.delta_seconds)?,
                    )?;
                }
                engine.set("time", time)?;

                let audio = Object::new(ctx.clone())?;
                {
                    let queue = Arc::clone(&audio_queue);
                    audio.set(
                        "playTone",
                        Function::new(
                            ctx.clone(),
                            move |frequency: f32, duration_seconds: f32, volume: f32| {
                                enqueue_tone(&queue, frequency, duration_seconds, volume)
                            },
                        )?,
                    )?;
                }
                engine.set("audio", audio)?;

                ctx.globals().set("engine", engine.clone())?;
                ctx.eval::<(), _>(source)?;

                let Some(game) = ctx.globals().get::<_, Option<Object>>("game")? else {
                    return Ok(GameHooks::default());
                };
                let initialize = game.get::<_, Option<Function>>("initialize")?;
                let update = game.get::<_, Option<Function>>("update")?;
                let shutdown = game.get::<_, Option<Function>>("shutdown")?;

                Ok(GameHooks {
                    game: Some(Persistent::save(&ctx, game)),
                    engine: Some(Persistent::save(&ctx, engine)),
                    initialize: initialize.map(|function| Persistent::save(&ctx, function)),
                    update: update.map(|function| Persistent::save(&ctx, function)),
                    shutdown: shutdown.map(|function| Persistent::save(&ctx, function)),
                })
            })
            .context("install native engine API, evaluate script and cache lifecycle hooks")?;

        Ok(Self {
            hooks,
            context,
            _runtime: runtime,
            host,
        })
    }

    pub fn initialize(&self) -> Result<()> {
        self.call_hook(GameHook::Initialize)
    }

    pub fn update(&self) -> Result<()> {
        self.call_hook(GameHook::Update)
    }

    pub fn shutdown(&self) -> Result<()> {
        self.call_hook(GameHook::Shutdown)
    }

    fn call_hook(&self, hook: GameHook) -> Result<()> {
        let (Some(game), Some(engine), Some(function)) = (
            self.hooks.game.as_ref(),
            self.hooks.engine.as_ref(),
            self.hooks.function(hook),
        ) else {
            return Ok(());
        };

        self.context.with(|ctx| -> anyhow::Result<()> {
            let game = game.clone().restore(&ctx)?;
            let engine = engine.clone().restore(&ctx)?;
            let function = function.clone().restore(&ctx)?;
            function
                .call::<_, ()>((This(game), engine))
                .with_context(|| format!("execute game.{}() in QuickJS-NG", hook.name()))?;
            Ok(())
        })
    }

    pub fn set_frame(
        &self,
        delta_seconds: f32,
        keys_down: HashSet<String>,
        keys_pressed: HashSet<String>,
        positions: HashMap<u32, [f32; 2]>,
        collisions: Vec<CollisionEvent>,
    ) {
        let mut state = lock(&self.host);
        state.frame.delta_seconds = delta_seconds;
        state.frame.keys_down = keys_down;
        state.frame.keys_pressed = keys_pressed;
        state.frame.positions = positions;
        state.frame.collisions = collisions;
    }

    /// Temporarily hands frame buffers to the host state without cloning their contents.
    /// All buffers are returned to the caller after the synchronous QuickJS update,
    /// including when the script returns an exception.
    pub fn update_frame(
        &self,
        delta_seconds: f32,
        keys_down: &mut HashSet<String>,
        keys_pressed: &mut HashSet<String>,
        positions: &mut HashMap<u32, [f32; 2]>,
        collisions: &mut Vec<CollisionEvent>,
    ) -> Result<()> {
        {
            let mut state = lock(&self.host);
            state.frame.delta_seconds = delta_seconds;
            std::mem::swap(&mut state.frame.keys_down, keys_down);
            std::mem::swap(&mut state.frame.keys_pressed, keys_pressed);
            std::mem::swap(&mut state.frame.positions, positions);
            std::mem::swap(&mut state.frame.collisions, collisions);
        }

        let update_result = self.update();

        {
            let mut state = lock(&self.host);
            std::mem::swap(&mut state.frame.keys_down, keys_down);
            std::mem::swap(&mut state.frame.keys_pressed, keys_pressed);
            std::mem::swap(&mut state.frame.positions, positions);
            std::mem::swap(&mut state.frame.collisions, collisions);
        }

        update_result
    }
    pub fn drain_commands(&self) -> Vec<RuntimeCommand> {
        std::mem::take(&mut lock(&self.host).commands)
    }

    pub fn drain_commands_into(&self, commands: &mut Vec<RuntimeCommand>) {
        commands.clear();
        let mut state = lock(&self.host);
        std::mem::swap(commands, &mut state.commands);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn allocate_entity_id(state: &mut HostState) -> rquickjs::Result<u32> {
    let entity_id = state.next_entity_id;
    state.next_entity_id = state
        .next_entity_id
        .checked_add(1)
        .ok_or_else(|| rquickjs::Error::new_from_js("entity id", "u32 overflow"))?;
    Ok(entity_id)
}

fn parse_body_type(value: &str) -> rquickjs::Result<RigidBodyKind> {
    RigidBodyKind::parse(value).map_err(|_| {
        rquickjs::Error::new_from_js("body type", "expected static, kinematic or dynamic")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio_queue() -> Arc<ArrayQueue<f32>> {
        Arc::new(ArrayQueue::new(32_768))
    }

    #[test]
    fn javascript_owns_game_creation_physics_and_audio_commands() {
        let source = r#"
            globalThis.game = {
                initialize() {
                    const kinematicEntity = engine.entities.createRectangle({
                        x: 0, y: -3, width: 1.4, height: 0.2,
                        r: 0.2, g: 0.8, b: 1, a: 1
                    });
                    engine.physics.createBox({
                        entityId: kinematicEntity, x: 0, y: -3, width: 1.4, height: 0.2,
                        bodyType: "kinematic", density: 0, friction: 0,
                        restitution: 0.9, gravityScale: 0, bullet: false
                    });
                    this.audioAccepted = engine.audio.playTone(440, 0.05, 0.2);
                    this.invalidAudioAccepted = engine.audio.playTone(0, 0.05, 0.2);
                },
                update() {
                    if (engine.input.wasPressed("Space")) {
                        engine.physics.setLinearVelocity(1, 2, 4);
                    }
                },
                shutdown() {}
            };
        "#;
        let queue = audio_queue();
        let script = ScriptEngine::from_source(source, Arc::clone(&queue)).unwrap();

        script.initialize().unwrap();
        let commands = script.drain_commands();
        assert!(commands.iter().any(|command| matches!(
            command,
            RuntimeCommand::CreateRectangle { entity_id: 1, .. }
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            RuntimeCommand::CreateBoxBody {
                entity_id: 1,
                body_type: RigidBodyKind::Kinematic,
                ..
            }
        )));
        assert!(!queue.is_empty());
        let (audio_accepted, invalid_audio_accepted) = script
            .context
            .with(|ctx| {
                let game: Object<'_> = ctx.globals().get("game")?;
                Ok::<_, rquickjs::Error>((
                    game.get::<_, bool>("audioAccepted")?,
                    game.get::<_, bool>("invalidAudioAccepted")?,
                ))
            })
            .unwrap();
        assert!(audio_accepted);
        assert!(!invalid_audio_accepted);

        script.set_frame(
            1.0 / 60.0,
            HashSet::new(),
            HashSet::from(["Space".to_owned()]),
            HashMap::new(),
            Vec::new(),
        );
        script.update().unwrap();
        assert!(script.drain_commands().iter().any(|command| matches!(
            command,
            RuntimeCommand::SetLinearVelocity {
                entity_id: 1,
                velocity: [2.0, 4.0]
            }
        )));
    }

    #[test]
    fn collision_queries_are_repeatable_and_replaced_for_each_frame() {
        let source = r#"
            globalThis.game = {
                firstCount: 0,
                secondCount: 0,
                firstEntityA: 0,
                initialize() {},
                update() {
                    this.firstCount = engine.physics.collisionCount();
                    this.secondCount = engine.physics.collisionCount();
                    this.firstEntityA = engine.physics.collisionEntityA(0) ?? 0;
                },
                shutdown() {}
            };
        "#;
        let script = ScriptEngine::from_source(source, audio_queue()).unwrap();

        script.set_frame(
            1.0 / 60.0,
            HashSet::new(),
            HashSet::new(),
            HashMap::new(),
            vec![CollisionEvent {
                entity_a: 42,
                entity_b: 7,
            }],
        );
        script.update().unwrap();

        let first_frame = script
            .context
            .with(|ctx| {
                let game: Object<'_> = ctx.globals().get("game")?;
                Ok::<_, rquickjs::Error>((
                    game.get::<_, u32>("firstCount")?,
                    game.get::<_, u32>("secondCount")?,
                    game.get::<_, u32>("firstEntityA")?,
                ))
            })
            .unwrap();
        assert_eq!(first_frame, (1, 1, 42));

        script.set_frame(
            1.0 / 60.0,
            HashSet::new(),
            HashSet::new(),
            HashMap::new(),
            Vec::new(),
        );
        script.update().unwrap();

        let next_frame = script
            .context
            .with(|ctx| {
                let game: Object<'_> = ctx.globals().get("game")?;
                Ok::<_, rquickjs::Error>((
                    game.get::<_, u32>("firstCount")?,
                    game.get::<_, u32>("secondCount")?,
                    game.get::<_, u32>("firstEntityA")?,
                ))
            })
            .unwrap();
        assert_eq!(next_frame, (0, 0, 0));
    }

    #[test]
    fn javascript_receives_native_positions_and_collision_events() {
        let source = r#"
            globalThis.game = {
                collisionCount: 0,
                x: 0,
                initialize() {},
                update() {
                    const x = engine.entities.getPositionX(42);
                    const y = engine.entities.getPositionY(42);
                    if (x !== undefined) this.x = x;
                    if (y !== undefined) this.y = y;
                    const count = engine.physics.collisionCount();
                    for (let index = 0; index < count; index += 1) {
                        this.collisionCount += 1;
                        this.lastEntityA = engine.physics.collisionEntityA(index);
                        this.lastEntityB = engine.physics.collisionEntityB(index);
                    }
                },
                shutdown() {}
            };
        "#;
        let script = ScriptEngine::from_source(source, audio_queue()).unwrap();
        script.set_frame(
            0.016,
            HashSet::new(),
            HashSet::new(),
            HashMap::from([(42, [3.0, -2.0])]),
            vec![CollisionEvent {
                entity_a: 42,
                entity_b: 7,
            }],
        );
        script.update().unwrap();

        script
            .context
            .with(|ctx| -> rquickjs::Result<()> {
                let game: Object = ctx.globals().get("game")?;
                assert_eq!(game.get::<_, f32>("x")?, 3.0);
                assert_eq!(game.get::<_, f32>("y")?, -2.0);
                assert_eq!(game.get::<_, u32>("collisionCount")?, 1);
                assert_eq!(game.get::<_, u32>("lastEntityA")?, 42);
                assert_eq!(game.get::<_, u32>("lastEntityB")?, 7);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn cached_lifecycle_hooks_preserve_this_binding_and_engine_argument() {
        let source = r#"
            globalThis.game = {
                calls: 0,
                engineMatches: true,
                initialize(engine) {
                    this.calls += 1;
                    this.engineMatches = this.engineMatches && engine === globalThis.engine;
                },
                update(engine) {
                    this.calls += 1;
                    this.engineMatches = this.engineMatches && engine === globalThis.engine;
                },
                shutdown(engine) {
                    this.calls += 1;
                    this.engineMatches = this.engineMatches && engine === globalThis.engine;
                }
            };
        "#;
        let script = ScriptEngine::from_source(source, audio_queue()).unwrap();
        script.initialize().unwrap();
        script.update().unwrap();
        script.shutdown().unwrap();

        script
            .context
            .with(|ctx| -> rquickjs::Result<()> {
                let game: Object<'_> = ctx.globals().get("game")?;
                assert_eq!(game.get::<_, u32>("calls")?, 3);
                assert!(game.get::<_, bool>("engineMatches")?);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn batch_position_api_fills_reusable_typed_array() {
        let source = r#"
            globalThis.game = {
                output: new Float32Array(4),
                initialize() {},
                update(engine) {
                    engine.entities.readPositions([42, 7], this.output);
                },
                shutdown() {}
            };
        "#;
        let script = ScriptEngine::from_source(source, audio_queue()).unwrap();
        script.set_frame(
            1.0 / 60.0,
            HashSet::new(),
            HashSet::new(),
            HashMap::from([(42, [3.5, -2.25]), (7, [10.0, 4.5])]),
            Vec::new(),
        );
        script.update().unwrap();

        script
            .context
            .with(|ctx| -> rquickjs::Result<()> {
                let values: Vec<f32> = ctx.eval("Array.from(game.output)")?;
                assert_eq!(values, vec![3.5, -2.25, 10.0, 4.5]);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn batch_position_api_rejects_mismatched_output_buffer_length() {
        let source = r#"
            globalThis.game = {
                initialize() {},
                update(engine) {
                    engine.entities.readPositions([1, 2], new Float32Array(2));
                },
                shutdown() {}
            };
        "#;
        let script = ScriptEngine::from_source(source, audio_queue()).unwrap();
        assert!(script.update().is_err());
    }
    #[test]
    fn zero_copy_frame_handoff_restores_all_caller_owned_buffers() {
        let source = r#"
            globalThis.game = {
                observed: null,
                initialize() {},
                update(engine) {
                    this.observed = {
                        x: engine.entities.getPositionX(42),
                        down: engine.input.isDown("A"),
                        pressed: engine.input.wasPressed("Space"),
                        collisions: engine.physics.collisionCount()
                    };
                },
                shutdown() {}
            };
        "#;
        let script = ScriptEngine::from_source(source, audio_queue()).unwrap();
        let mut keys_down = HashSet::from([String::from("A")]);
        let mut keys_pressed = HashSet::from([String::from("Space")]);
        let mut positions = HashMap::from([(42, [3.5, -2.25])]);
        let mut collisions = vec![CollisionEvent {
            entity_a: 42,
            entity_b: 7,
        }];

        script
            .update_frame(
                1.0 / 60.0,
                &mut keys_down,
                &mut keys_pressed,
                &mut positions,
                &mut collisions,
            )
            .unwrap();

        assert!(keys_down.contains("A"));
        assert!(keys_pressed.contains("Space"));
        assert_eq!(positions.get(&42), Some(&[3.5, -2.25]));
        assert_eq!(collisions.len(), 1);
        assert_eq!((collisions[0].entity_a, collisions[0].entity_b), (42, 7));

        script
            .context
            .with(|ctx| -> rquickjs::Result<()> {
                let game: Object<'_> = ctx.globals().get("game")?;
                let observed: Object<'_> = game.get("observed")?;
                assert_eq!(observed.get::<_, f32>("x")?, 3.5);
                assert!(observed.get::<_, bool>("down")?);
                assert!(observed.get::<_, bool>("pressed")?);
                assert_eq!(observed.get::<_, u32>("collisions")?, 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn zero_copy_frame_handoff_restores_buffers_when_script_fails() {
        let source = r#"
            globalThis.game = {
                initialize() {},
                update() { throw new Error("expected test failure"); },
                shutdown() {}
            };
        "#;
        let script = ScriptEngine::from_source(source, audio_queue()).unwrap();
        let mut keys_down = HashSet::from([String::from("D")]);
        let mut keys_pressed = HashSet::new();
        let mut positions = HashMap::from([(5, [1.0, 2.0])]);
        let mut collisions = vec![CollisionEvent {
            entity_a: 5,
            entity_b: 6,
        }];

        assert!(
            script
                .update_frame(
                    1.0 / 60.0,
                    &mut keys_down,
                    &mut keys_pressed,
                    &mut positions,
                    &mut collisions,
                )
                .is_err()
        );
        assert!(keys_down.contains("D"));
        assert!(keys_pressed.is_empty());
        assert_eq!(positions.get(&5), Some(&[1.0, 2.0]));
        assert_eq!(collisions.len(), 1);
        assert_eq!((collisions[0].entity_a, collisions[0].entity_b), (5, 6));
    }
    #[test]
    fn runtime_propagates_script_errors_instead_of_silently_skipping_updates() {
        let script = ScriptEngine::from_source(
            "globalThis.game = { update() { throw new Error('game update failed'); } };",
            audio_queue(),
        )
        .unwrap();
        assert!(script.update().is_err());
    }
}
