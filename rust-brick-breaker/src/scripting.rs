use std::sync::Arc;

use anyhow::{Context as _, Result};
use crossbeam_queue::ArrayQueue;
use rquickjs::{Context, Function, Object, Runtime};

use crate::audio::enqueue_sound;

const GAME_SCRIPT: &str = include_str!("../assets/game.js");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameStatus {
    Ready,
    Playing,
    Victory,
    GameOver,
}

#[derive(Clone, Copy, Debug)]
pub struct GameSnapshot {
    pub score: u32,
    pub lives: u32,
    pub status: GameStatus,
}

pub struct ScriptEngine {
    context: Context,
    _runtime: Runtime,
}

impl ScriptEngine {
    pub fn new(audio_queue: Arc<ArrayQueue<f32>>) -> Result<Self> {
        let runtime = Runtime::new().context("create QuickJS-NG runtime")?;
        let context = Context::full(&runtime).context("create QuickJS-NG context")?;

        context
            .with(|ctx| -> rquickjs::Result<()> {
                let host = Object::new(ctx.clone())?;
                let sound_queue = Arc::clone(&audio_queue);
                host.set(
                    "playSound",
                    Function::new(ctx.clone(), move |name: String| {
                        enqueue_sound(&sound_queue, &name);
                    })?,
                )?;
                ctx.globals().set("host", host)?;
                ctx.eval::<(), _>(GAME_SCRIPT)?;
                Ok(())
            })
            .context("install typed Rust host API and evaluate compiled TypeScript")?;

        Ok(Self {
            context,
            _runtime: runtime,
        })
    }

    pub fn launch(&self) -> Result<GameSnapshot> {
        self.evaluate("game.onLaunched()")
    }

    pub fn bounced(&self) -> Result<GameSnapshot> {
        self.evaluate("game.onBounced()")
    }

    pub fn brick_destroyed(&self, points: u32, remaining: usize) -> Result<GameSnapshot> {
        self.evaluate(&format!("game.onBrickDestroyed({points}, {remaining})"))
    }

    pub fn ball_lost(&self) -> Result<GameSnapshot> {
        self.evaluate("game.onBallLost()")
    }

    pub fn restart(&self) -> Result<GameSnapshot> {
        self.evaluate("game.restart()")
    }

    pub fn snapshot(&self) -> Result<GameSnapshot> {
        self.read_snapshot()
    }

    fn evaluate(&self, expression: &str) -> Result<GameSnapshot> {
        self.context.with(|ctx| -> anyhow::Result<GameSnapshot> {
            ctx.eval::<(), _>(expression)
                .context("execute TypeScript game rule in QuickJS-NG")?;
            read_snapshot(ctx).context("read QuickJS game state")
        })
    }

    fn read_snapshot(&self) -> Result<GameSnapshot> {
        self.context.with(|ctx| read_snapshot(ctx))
    }
}

fn read_snapshot(ctx: rquickjs::Ctx<'_>) -> Result<GameSnapshot> {
    let snapshot: Object = ctx
        .eval("game.snapshot()")
        .context("call game.snapshot()")?;
    let status = match snapshot.get::<_, String>("status")?.as_str() {
        "ready" => GameStatus::Ready,
        "playing" => GameStatus::Playing,
        "victory" => GameStatus::Victory,
        "game_over" => GameStatus::GameOver,
        value => anyhow::bail!("QuickJS returned unknown game status: {value}"),
    };

    Ok(GameSnapshot {
        score: snapshot.get("score")?,
        lives: snapshot.get("lives")?,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quickjs_executes_compiled_typescript_and_updates_game_rules() {
        let queue = Arc::new(ArrayQueue::new(4096));
        let game = ScriptEngine::new(queue).expect("initialize QuickJS-NG");

        assert_eq!(game.snapshot().unwrap().status, GameStatus::Ready);
        assert_eq!(game.launch().unwrap().status, GameStatus::Playing);

        let state = game.brick_destroyed(100, 2).unwrap();
        assert_eq!(state.score, 100);
        assert_eq!(state.status, GameStatus::Playing);

        let state = game.brick_destroyed(100, 0).unwrap();
        assert_eq!(state.score, 200);
        assert_eq!(state.status, GameStatus::Victory);

        let reset = game.restart().unwrap();
        assert_eq!(reset.score, 0);
        assert_eq!(reset.lives, 3);
        assert_eq!(reset.status, GameStatus::Ready);
    }

    #[test]
    fn losing_last_life_enters_game_over_inside_quickjs() {
        let game = ScriptEngine::new(Arc::new(ArrayQueue::new(4096))).unwrap();
        game.launch().unwrap();
        game.ball_lost().unwrap();
        game.launch().unwrap();
        game.ball_lost().unwrap();
        game.launch().unwrap();
        let state = game.ball_lost().unwrap();

        assert_eq!(state.lives, 0);
        assert_eq!(state.status, GameStatus::GameOver);
    }
}
