type RigidBodyType = "static" | "kinematic" | "dynamic";

interface Color {
    r: number;
    g: number;
    b: number;
    a: number;
}

interface RectangleOptions extends Color {
    x: number;
    y: number;
    width: number;
    height: number;
}

interface CircleOptions extends Color {
    x: number;
    y: number;
    radius: number;
}

interface TextOptions extends Color {
    value: string;
    x: number;
    y: number;
    scale: number;
}

interface BoxBodyOptions {
    entityId: number;
    x: number;
    y: number;
    width: number;
    height: number;
    bodyType: RigidBodyType;
    density: number;
    friction: number;
    restitution: number;
    gravityScale: number;
    bullet: boolean;
}

interface CircleBodyOptions {
    entityId: number;
    x: number;
    y: number;
    radius: number;
    bodyType: RigidBodyType;
    density: number;
    friction: number;
    restitution: number;
    gravityScale: number;
    bullet: boolean;
}

interface Vector2 {
    x: number;
    y: number;
}

interface CollisionEvent {
    entityA: number;
    entityB: number;
}

interface EntityApi {
    createRectangle(options: RectangleOptions): number;
    createCircle(options: CircleOptions): number;
    createText(options: TextOptions): number;
    setPosition(entityId: number, x: number, y: number): void;
    setText(entityId: number, value: string): void;
    getPositionX(entityId: number): number | undefined;
    getPositionY(entityId: number): number | undefined;
    destroy(entityId: number): void;
}

interface PhysicsApi {
    createBox(options: BoxBodyOptions): void;
    createCircle(options: CircleBodyOptions): void;
    setLinearVelocity(entityId: number, x: number, y: number): void;
    setPosition(entityId: number, x: number, y: number): void;
    destroyBody(entityId: number): void;
    collisionCount(): number;
    collisionEntityA(index: number): number | undefined;
    collisionEntityB(index: number): number | undefined;
}

interface InputApi {
    isDown(key: string): boolean;
    wasPressed(key: string): boolean;
}

interface AudioApi {
    playTone(frequency: number, durationSeconds: number, volume: number): boolean;
}

interface TimeApi {
    deltaSeconds(): number;
}

interface EngineApi {
    entities: EntityApi;
    physics: PhysicsApi;
    input: InputApi;
    audio: AudioApi;
    time: TimeApi;
}

interface GameLifecycle {
    initialize(engine: EngineApi): void;
    update(engine: EngineApi): void;
    shutdown(engine: EngineApi): void;
}

const COLORS = {
    paddle: { r: 0.30, g: 0.82, b: 1.0, a: 1.0 },
    ball: { r: 1.0, g: 0.94, b: 0.72, a: 1.0 },
    wall: { r: 0.20, g: 0.30, b: 0.48, a: 1.0 },
    text: { r: 0.88, g: 0.93, b: 1.0, a: 1.0 },
    status: { r: 1.0, g: 0.92, b: 0.64, a: 1.0 },
    bricks: [
        { r: 1.0, g: 0.40, b: 0.43, a: 1.0 },
        { r: 1.0, g: 0.66, b: 0.31, a: 1.0 },
        { r: 0.78, g: 0.60, b: 1.0, a: 1.0 },
        { r: 0.42, g: 0.86, b: 0.62, a: 1.0 },
    ],
};

type GameState = "ready" | "playing" | "victory" | "game_over";

class BrickBreakerGame implements GameLifecycle {
    private runtime!: EngineApi;
    private paddleId = 0;
    private ballId = 0;
    private scoreLabelId = 0;
    private livesLabelId = 0;
    private statusLabelId = 0;
    private readonly bricks = new Set<number>();
    private score = 0;
    private lives = 3;
    private state: GameState = "ready";
    private displayedScore = -1;
    private displayedLives = -1;

    initialize(runtime: EngineApi): void {
        this.runtime = runtime;
        this.createArena();

        this.paddleId = runtime.entities.createRectangle({
            x: 0, y: -3.18, width: 1.40, height: 0.22, ...COLORS.paddle,
        });
        runtime.physics.createBox({
            entityId: this.paddleId, x: 0, y: -3.18, width: 1.40, height: 0.22,
            bodyType: "kinematic", density: 0, friction: 0,
            restitution: 0.92, gravityScale: 0, bullet: false,
        });

        this.ballId = runtime.entities.createCircle({
            x: 0, y: -2.88, radius: 0.12, ...COLORS.ball,
        });
        runtime.physics.createCircle({
            entityId: this.ballId, x: 0, y: -2.88, radius: 0.12,
            bodyType: "dynamic", density: 1, friction: 0,
            restitution: 1, gravityScale: 0, bullet: true,
        });

        this.scoreLabelId = runtime.entities.createText({
            value: "SCORE 0", x: -0.96, y: 0.94, scale: 0.016, ...COLORS.text,
        });
        this.livesLabelId = runtime.entities.createText({
            value: "LIVES 3", x: 0.28, y: 0.94, scale: 0.016, ...COLORS.text,
        });
        this.statusLabelId = runtime.entities.createText({
            value: "", x: -0.90, y: 0.12, scale: 0.014, ...COLORS.status,
        });

        this.createBricks();
        this.setStatus("PRESS SPACE TO LAUNCH");
    }

    update(runtime: EngineApi): void {
        this.processCollisions(runtime);

        if (this.state === "playing") {
            this.updatePaddle(runtime);
            this.checkBallLost(runtime);
        }

        if (runtime.input.wasPressed("Space")) {
            this.handleSpace();
        }

        this.updateHud();
    }

    shutdown(_runtime: EngineApi): void {
        // Native ECS and physics resources are released with the runtime.
    }

    private createArena(): void {
        this.createWall(-4.90, 0, 0.20, 7.50);
        this.createWall(4.90, 0, 0.20, 7.50);
        this.createWall(0, 3.68, 10.0, 0.20);
    }

    private createWall(x: number, y: number, width: number, height: number): void {
        const entityId = this.runtime.entities.createRectangle({
            x, y, width, height, ...COLORS.wall,
        });
        this.runtime.physics.createBox({
            entityId, x, y, width, height, bodyType: "static",
            density: 0, friction: 0, restitution: 1, gravityScale: 0, bullet: false,
        });
    }

    private createBricks(): void {
        for (let row = 0; row < 4; row += 1) {
            for (let column = 0; column < 9; column += 1) {
                const x = (column - 4) * 1.04;
                const y = 1.28 + row * 0.48;
                const entityId = this.runtime.entities.createRectangle({
                    x, y, width: 0.88, height: 0.32, ...COLORS.bricks[row]!,
                });
                this.runtime.physics.createBox({
                    entityId, x, y, width: 0.88, height: 0.32, bodyType: "static",
                    density: 0, friction: 0, restitution: 1, gravityScale: 0, bullet: false,
                });
                this.bricks.add(entityId);
            }
        }
    }

    private processCollisions(runtime: EngineApi): void {
        const collisionCount = runtime.physics.collisionCount();
        for (let index = 0; index < collisionCount; index += 1) {
            const entityA = runtime.physics.collisionEntityA(index);
            const entityB = runtime.physics.collisionEntityB(index);
            if (entityA === undefined || entityB === undefined) continue;

            const other = entityA === this.ballId
                ? entityB
                : entityB === this.ballId
                    ? entityA
                    : 0;

            if (other === 0) continue;

            if (this.bricks.delete(other)) {
                runtime.physics.destroyBody(other);
                runtime.entities.destroy(other);
                this.score += 100;
                runtime.audio.playTone(760, 0.085, 0.20);

                if (this.bricks.size === 0) {
                    this.stopPaddle(runtime);
                    this.state = "victory";
                    this.setStatus("VICTORY");
                }
            } else {
                runtime.audio.playTone(520, 0.045, 0.14);
            }
        }
    }

    private updatePaddle(runtime: EngineApi): void {
        const left = runtime.input.isDown("A") || runtime.input.isDown("ArrowLeft");
        const right = runtime.input.isDown("D") || runtime.input.isDown("ArrowRight");
        const direction = Number(right) - Number(left);
        const x = runtime.entities.getPositionX(this.paddleId) ?? 0;

        let velocity = direction * 8.0;
        if ((x <= -3.95 && velocity < 0) || (x >= 3.95 && velocity > 0)) {
            velocity = 0;
        }
        runtime.physics.setLinearVelocity(this.paddleId, velocity, 0);
    }

    private stopPaddle(runtime: EngineApi): void {
        runtime.physics.setLinearVelocity(this.paddleId, 0, 0);
    }

    private checkBallLost(runtime: EngineApi): void {
        const y = runtime.entities.getPositionY(this.ballId);
        if (y !== undefined && y < -3.95) {
            this.stopPaddle(runtime);
            this.lives = Math.max(0, this.lives - 1);
            runtime.physics.setPosition(this.ballId, 0, -2.88);
            runtime.physics.setLinearVelocity(this.ballId, 0, 0);
            runtime.audio.playTone(180, 0.18, 0.24);

            if (this.lives > 0) {
                this.state = "ready";
                this.setStatus("PRESS SPACE TO LAUNCH");
            } else {
                this.state = "game_over";
                this.setStatus("GAME OVER");
            }
        }
    }

    private handleSpace(): void {
        if (this.state === "ready") {
            const paddleX = this.runtime.entities.getPositionX(this.paddleId) ?? 0;
            this.runtime.physics.setPosition(this.ballId, paddleX, -2.88);
            this.runtime.physics.setLinearVelocity(this.ballId, 3.2, 5.0);
            this.state = "playing";
            this.setStatus("");
            return;
        }

        if (this.state === "victory" || this.state === "game_over") {
            this.restart();
        }
    }

    private restart(): void {
        for (const entityId of this.bricks) {
            this.runtime.physics.destroyBody(entityId);
            this.runtime.entities.destroy(entityId);
        }
        this.bricks.clear();

        this.score = 0;
        this.lives = 3;
        this.state = "ready";
        this.runtime.physics.setPosition(this.paddleId, 0, -3.18);
        this.stopPaddle(this.runtime);
        this.runtime.physics.setPosition(this.ballId, 0, -2.88);
        this.runtime.physics.setLinearVelocity(this.ballId, 0, 0);
        this.createBricks();
        this.setStatus("PRESS SPACE TO LAUNCH");
        this.updateHud();
    }

    private setStatus(value: string): void {
        this.runtime.entities.setText(this.statusLabelId, value);
    }

    private updateHud(): void {
        if (this.displayedScore !== this.score) {
            this.runtime.entities.setText(this.scoreLabelId, `SCORE ${this.score}`);
            this.displayedScore = this.score;
        }
        if (this.displayedLives !== this.lives) {
            this.runtime.entities.setText(this.livesLabelId, `LIVES ${this.lives}`);
            this.displayedLives = this.lives;
        }
    }
}

(globalThis as typeof globalThis & { game: GameLifecycle }).game = new BrickBreakerGame();
