import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { runInNewContext } from "node:vm";

const source = await readFile(new URL("../assets/game.js", import.meta.url), "utf8");

function createGame() {
    const sounds = [];
    const sandbox = {
        host: {
            playSound(name) {
                sounds.push(name);
            },
        },
    };
    runInNewContext(source, sandbox);
    return { game: sandbox.game, sounds };
}

test("TypeScript rules score bricks and produce victory", () => {
    const { game, sounds } = createGame();

    game.onLaunched();
    game.onBrickDestroyed(100, 1);
    assert.deepEqual({ ...game.snapshot() }, {
        score: 100,
        lives: 3,
        status: "playing",
    });

    game.onBrickDestroyed(100, 0);
    assert.deepEqual({ ...game.snapshot() }, {
        score: 200,
        lives: 3,
        status: "victory",
    });
    assert.deepEqual(sounds, ["brick", "brick"]);
});

test("TypeScript rules decrement lives and enter game over", () => {
    const { game, sounds } = createGame();

    game.onLaunched();
    game.onBallLost();
    assert.equal(game.snapshot().lives, 2);
    assert.equal(game.snapshot().status, "ready");

    game.onLaunched();
    game.onBallLost();
    game.onLaunched();
    game.onBallLost();
    assert.deepEqual({ ...game.snapshot() }, {
        score: 0,
        lives: 0,
        status: "game_over",
    });
    assert.deepEqual(sounds, ["life_lost", "life_lost", "life_lost"]);
});

test("restart restores the complete initial game state", () => {
    const { game } = createGame();

    game.onLaunched();
    game.onBrickDestroyed(300, 8);
    game.onBallLost();
    game.restart();

    assert.deepEqual({ ...game.snapshot() }, {
        score: 0,
        lives: 3,
        status: "ready",
    });
});
