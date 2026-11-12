type GameStatus = "ready" | "playing" | "victory" | "game_over";

interface GameSnapshot {
    score: number;
    lives: number;
    status: GameStatus;
}

interface HostApi {
    playSound(name: "bounce" | "brick" | "life_lost"): void;
}

declare const host: HostApi;

class GameLogic {
    private score = 0;
    private lives = 3;
    private status: GameStatus = "ready";

    onLaunched(): void {
        if (this.status === "ready") {
            this.status = "playing";
        }
    }

    onBounced(): void {
        if (this.status === "playing") {
            host.playSound("bounce");
        }
    }

    onBrickDestroyed(points: number, remainingBricks: number): void {
        if (this.status !== "playing") {
            return;
        }

        this.score += points;
        host.playSound("brick");
        if (remainingBricks <= 0) {
            this.status = "victory";
        }
    }

    onBallLost(): void {
        if (this.status !== "playing") {
            return;
        }

        this.lives = Math.max(0, this.lives - 1);
        host.playSound("life_lost");
        this.status = this.lives > 0 ? "ready" : "game_over";
    }

    restart(): void {
        this.score = 0;
        this.lives = 3;
        this.status = "ready";
    }

    snapshot(): GameSnapshot {
        return {
            score: this.score,
            lives: this.lives,
            status: this.status,
        };
    }
}

(globalThis as typeof globalThis & { game: GameLogic }).game = new GameLogic();
