import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { runInNewContext } from "node:vm";

const source = await readFile(new URL("../assets/game.js", import.meta.url), "utf8");

function createGame() {
    let nextEntityId = 1;
    const entities = new Map();
    const bodies = new Map();
    const tones = [];
    let collisions = [];
    let scriptCollisions = [];
    const keysDown = new Set();
    const keysPressed = new Set();
    const gamepadState = {
        connected: false,
        leftStickX: 0,
        down: new Set(),
        pressed: new Set(),
    };

    const engine = {
        entities: {
            createRectangle(options) {
                const id = nextEntityId++;
                entities.set(id, { kind: "rectangle", ...options });
                return id;
            },
            createCircle(options) {
                const id = nextEntityId++;
                entities.set(id, { kind: "circle", ...options });
                return id;
            },
            createText(options) {
                const id = nextEntityId++;
                entities.set(id, { kind: "text", ...options });
                return id;
            },
            setPosition(id, x, y) {
                const entity = entities.get(id);
                if (entity) Object.assign(entity, { x, y });
            },
            setText(id, value) {
                const entity = entities.get(id);
                if (entity) entity.value = value;
            },
            getPositionX(id) {
                return entities.get(id)?.x;
            },
            getPositionY(id) {
                return entities.get(id)?.y;
            },
            destroy(id) {
                entities.delete(id);
            },
        },
        physics: {
            createBox(options) {
                bodies.set(options.entityId, {
                    shape: "box", ...options, velocity: [0, 0],
                });
            },
            createCircle(options) {
                bodies.set(options.entityId, {
                    shape: "circle", ...options, velocity: [0, 0],
                });
            },
            setLinearVelocity(id, x, y) {
                const body = bodies.get(id);
                if (body) body.velocity = [x, y];
            },
            setPosition(id, x, y) {
                const body = bodies.get(id);
                if (body) Object.assign(body, { x, y });
                engine.entities.setPosition(id, x, y);
            },
            destroyBody(id) {
                bodies.delete(id);
            },
            collisionCount() {
                scriptCollisions = collisions;
                collisions = [];
                return scriptCollisions.length;
            },
            collisionEntityA(index) {
                return scriptCollisions[index]?.entityA;
            },
            collisionEntityB(index) {
                return scriptCollisions[index]?.entityB;
            },
        },
        input: {
            isDown(key) {
                return keysDown.has(key);
            },
            wasPressed(key) {
                return keysPressed.has(key);
            },
            gamepad: {
                connected() {
                    return gamepadState.connected;
                },
                leftStickX() {
                    return gamepadState.leftStickX;
                },
                isDown(button) {
                    return gamepadState.down.has(button);
                },
                wasPressed(button) {
                    return gamepadState.pressed.has(button);
                },
            },
        },
        audio: {
            playTone(frequency, durationSeconds, volume) {
                tones.push({ frequency, durationSeconds, volume });
            },
        },
        time: {
            deltaSeconds() {
                return 1 / 60;
            },
        },
    };

    const sandbox = { engine };
    runInNewContext(source, sandbox);
    sandbox.game.initialize(engine);

    return {
        game: sandbox.game,
        engine,
        entities,
        bodies,
        tones,
        keysDown,
        keysPressed,
        gamepadState,
        setCollisions(value) {
            collisions = value;
        },
        stepPhysics(deltaSeconds) {
            for (const [id, body] of bodies) {
                if (body.bodyType !== "kinematic") continue;
                body.x += body.velocity[0] * deltaSeconds;
                body.y += body.velocity[1] * deltaSeconds;
                engine.entities.setPosition(id, body.x, body.y);
            }
        },
    };
}

function findBall(bodies) {
    return [...bodies.entries()].find(([, body]) => body.shape === "circle");
}

function findBricks(bodies) {
    return [...bodies.entries()]
        .filter(([, body]) => body.shape === "box" && body.width === 0.88 && body.height === 0.32)
        .map(([id]) => id);
}

function findPaddle(bodies) {
    return [...bodies.entries()].find(([, body]) =>
        body.bodyType === "kinematic" && body.width === 1.4 && body.height === 0.22
    );
}

function findLabel(entities, prefix) {
    return [...entities.values()].find((entity) =>
        entity.kind === "text" && entity.value.startsWith(prefix)
    )?.value;
}

test("Brick Breaker is created entirely through the native runtime API", () => {
    const { entities, bodies } = createGame();

    assert.equal([...entities.values()].filter((entity) => entity.kind === "rectangle").length, 40);
    assert.equal([...entities.values()].filter((entity) => entity.kind === "circle").length, 1);
    assert.equal([...entities.values()].filter((entity) => entity.kind === "text").length, 3);
    assert.equal(bodies.size, 41);
    assert.equal(findLabel(entities, "SCORE"), "SCORE 0");
    assert.equal(findLabel(entities, "LIVES"), "LIVES 3");
    assert.equal(findLabel(entities, "PRESS SPACE"), "PRESS SPACE TO LAUNCH");
});

test("Space launches the ball through the generic physics API", () => {
    const { game, engine, bodies, keysPressed } = createGame();
    const [ballId] = findBall(bodies);

    keysPressed.add("Space");
    game.update(engine);
    assert.deepEqual(bodies.get(ballId).velocity, [3.2, 5]);

    keysPressed.clear();
});

test("Box2D collision events drive brick removal and scoring in TypeScript", () => {
    const { game, engine, entities, bodies, tones, setCollisions } = createGame();
    const [ballId] = findBall(bodies);
    const firstBrick = findBricks(bodies)[0];

    setCollisions([{ entityA: ballId, entityB: firstBrick }]);
    game.update(engine);

    assert.equal(bodies.has(firstBrick), false);
    assert.equal(entities.has(firstBrick), false);
    assert.equal(findLabel(entities, "SCORE"), "SCORE 100");
    assert.equal(tones.at(-1).frequency, 760);
});

test("losing the ball decrements lives and resets the round state in TypeScript", () => {
    const { game, engine, entities, bodies, keysPressed } = createGame();
    const [ballId] = findBall(bodies);

    keysPressed.add("Space");
    game.update(engine);
    keysPressed.clear();

    const ball = entities.get(ballId);
    Object.assign(ball, { y: -4.2 });
    Object.assign(bodies.get(ballId), { y: -4.2 });
    game.update(engine);

    assert.equal(findLabel(entities, "LIVES"), "LIVES 2");
    assert.deepEqual(bodies.get(ballId).velocity, [0, 0]);
    assert.equal(entities.get(ballId).y, -2.88);
});

test("clearing the final brick enters victory and Space restarts the level", () => {
    const { game, engine, entities, bodies, keysPressed, setCollisions } = createGame();
    const [ballId] = findBall(bodies);
    const brickIds = findBricks(bodies);

    keysPressed.add("Space");
    game.update(engine);
    keysPressed.clear();

    setCollisions(brickIds.map((entityB) => ({ entityA: ballId, entityB })));
    game.update(engine);

    assert.equal(findLabel(entities, "SCORE"), "SCORE 3600");
    assert.equal(findLabel(entities, "VICTORY"), "VICTORY");
    assert.equal(brickIds.every((id) => !bodies.has(id) && !entities.has(id)), true);

    keysPressed.add("Space");
    game.update(engine);
    keysPressed.clear();

    assert.equal(findLabel(entities, "SCORE"), "SCORE 0");
    assert.equal(findLabel(entities, "LIVES"), "LIVES 3");
    assert.equal(findLabel(entities, "PRESS SPACE"), "PRESS SPACE TO LAUNCH");
    assert.equal(findBricks(bodies).length, 36);
});

test("losing a ball stops paddle drift before the next launch", () => {
    const { game, engine, entities, bodies, keysDown, keysPressed, stepPhysics } = createGame();
    const [ballId] = findBall(bodies);
    const [paddleId] = findPaddle(bodies);

    keysPressed.add("Space");
    game.update(engine);
    keysPressed.clear();

    keysDown.add("A");
    Object.assign(entities.get(ballId), { y: -4.2 });
    Object.assign(bodies.get(ballId), { y: -4.2 });
    game.update(engine);

    assert.deepEqual(bodies.get(paddleId).velocity, [0, 0]);
    stepPhysics(1 / 60);
    assert.equal(entities.get(paddleId).x, 0);

    keysDown.delete("A");
    for (let frame = 0; frame < 120; frame += 1) {
        game.update(engine);
        stepPhysics(1 / 60);
    }
    assert.deepEqual(bodies.get(paddleId).velocity, [0, 0]);
    assert.ok(entities.get(paddleId).x >= -3.95 && entities.get(paddleId).x <= 3.95);

    keysPressed.add("Space");
    game.update(engine);
    keysPressed.clear();

    assert.equal(entities.get(ballId).x, entities.get(paddleId).x);
    assert.equal(entities.get(ballId).y, -2.88);
    assert.ok(Math.abs(entities.get(ballId).x) <= 3.95);
});

test("victory stops the kinematic paddle before the next restart", () => {
    const { game, engine, bodies, keysDown, keysPressed, setCollisions } = createGame();
    const [ballId] = findBall(bodies);
    const [paddleId] = findPaddle(bodies);
    const brickIds = findBricks(bodies);

    keysPressed.add("Space");
    game.update(engine);
    keysPressed.clear();

    keysDown.add("D");
    game.update(engine);
    assert.deepEqual(bodies.get(paddleId).velocity, [8, 0]);

    setCollisions(brickIds.map((entityB) => ({ entityA: ballId, entityB })));
    game.update(engine);

    assert.deepEqual(bodies.get(paddleId).velocity, [0, 0]);
});

test("three lost balls enter Game Over in the TypeScript game state machine", () => {
    const { game, engine, entities, bodies, keysPressed } = createGame();
    const [ballId] = findBall(bodies);

    for (let life = 0; life < 3; life += 1) {
        keysPressed.add("Space");
        game.update(engine);
        keysPressed.clear();

        Object.assign(entities.get(ballId), { y: -4.2 });
        Object.assign(bodies.get(ballId), { y: -4.2 });
        game.update(engine);
    }

    assert.equal(findLabel(entities, "LIVES"), "LIVES 0");
    assert.equal(findLabel(entities, "GAME OVER"), "GAME OVER");
});

test("Gamepad South launches the ball through the typed host input API", () => {
    const { game, engine, bodies, gamepadState } = createGame();
    const [ballId] = findBall(bodies);

    gamepadState.connected = true;
    gamepadState.pressed.add("South");
    game.update(engine);
    assert.deepEqual(bodies.get(ballId).velocity, [3.2, 5]);

    gamepadState.pressed.clear();
    game.update(engine);
    assert.deepEqual(bodies.get(ballId).velocity, [3.2, 5]);
});

test("Left Stick X controls paddle velocity as an analog value", () => {
    const { game, engine, bodies, keysPressed, gamepadState } = createGame();
    const [paddleId] = findPaddle(bodies);

    keysPressed.add("Space");
    game.update(engine);
    keysPressed.clear();

    gamepadState.connected = true;
    gamepadState.leftStickX = 0.5;
    game.update(engine);
    assert.deepEqual(bodies.get(paddleId).velocity, [4, 0]);
});

test("Gamepad D-pad controls the paddle and combines predictably with keyboard input", () => {
    const { game, engine, bodies, keysPressed, keysDown, gamepadState } = createGame();
    const [paddleId] = findPaddle(bodies);

    keysPressed.add("Space");
    game.update(engine);
    keysPressed.clear();

    gamepadState.connected = true;
    gamepadState.down.add("DPadLeft");
    game.update(engine);
    assert.deepEqual(bodies.get(paddleId).velocity, [-8, 0]);

    keysDown.add("D");
    game.update(engine);
    assert.deepEqual(bodies.get(paddleId).velocity, [0, 0]);
});
