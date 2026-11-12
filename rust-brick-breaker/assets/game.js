"use strict";
class GameLogic {
    constructor() {
        this.score = 0;
        this.lives = 3;
        this.status = "ready";
    }
    onLaunched() {
        if (this.status === "ready") {
            this.status = "playing";
        }
    }
    onBounced() {
        if (this.status === "playing") {
            host.playSound("bounce");
        }
    }
    onBrickDestroyed(points, remainingBricks) {
        if (this.status !== "playing") {
            return;
        }
        this.score += points;
        host.playSound("brick");
        if (remainingBricks <= 0) {
            this.status = "victory";
        }
    }
    onBallLost() {
        if (this.status !== "playing") {
            return;
        }
        this.lives = Math.max(0, this.lives - 1);
        host.playSound("life_lost");
        this.status = this.lives > 0 ? "ready" : "game_over";
    }
    restart() {
        this.score = 0;
        this.lives = 3;
        this.status = "ready";
    }
    snapshot() {
        return {
            score: this.score,
            lives: this.lives,
            status: this.status,
        };
    }
}
globalThis.game = new GameLogic();
