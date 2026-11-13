use std::collections::HashSet;

use gilrs::{Axis, Button, EventType, Gilrs};
use winit::keyboard::KeyCode;

pub const GAMEPAD_DEADZONE: f32 = 0.18;

#[derive(Default)]
pub struct InputState {
    pub down: HashSet<String>,
    pub pressed: HashSet<String>,
}

impl InputState {
    pub fn key_down(&mut self, key: &str) {
        if self.down.insert(key.to_owned()) {
            self.pressed.insert(key.to_owned());
        }
    }

    pub fn key_up(&mut self, key: &str) {
        self.down.remove(key);
    }

    pub fn clear(&mut self) {
        self.down.clear();
        self.pressed.clear();
    }

    pub fn finish_frame(&mut self) {
        self.pressed.clear();
    }
}

pub fn key_name(key: KeyCode) -> Option<&'static str> {
    match key {
        KeyCode::KeyA => Some("A"),
        KeyCode::KeyD => Some("D"),
        KeyCode::KeyW => Some("W"),
        KeyCode::KeyS => Some("S"),
        KeyCode::ArrowLeft => Some("ArrowLeft"),
        KeyCode::ArrowRight => Some("ArrowRight"),
        KeyCode::ArrowUp => Some("ArrowUp"),
        KeyCode::ArrowDown => Some("ArrowDown"),
        KeyCode::Space => Some("Space"),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GamepadState {
    pub connected: bool,
    pub left_stick_x: f32,
    pub dpad_left_down: bool,
    pub dpad_right_down: bool,
    pub south_down: bool,
    pub dpad_left_pressed: bool,
    pub dpad_right_pressed: bool,
    pub south_pressed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GamepadButton {
    South,
    DPadLeft,
    DPadRight,
}

impl GamepadState {
    pub fn is_down(self, button: GamepadButton) -> bool {
        match button {
            GamepadButton::South => self.south_down,
            GamepadButton::DPadLeft => self.dpad_left_down,
            GamepadButton::DPadRight => self.dpad_right_down,
        }
    }

    pub fn was_pressed(self, button: GamepadButton) -> bool {
        match button {
            GamepadButton::South => self.south_pressed,
            GamepadButton::DPadLeft => self.dpad_left_pressed,
            GamepadButton::DPadRight => self.dpad_right_pressed,
        }
    }

    pub fn is_down_named(self, button: &str) -> bool {
        match button {
            "South" => self.is_down(GamepadButton::South),
            "DPadLeft" => self.is_down(GamepadButton::DPadLeft),
            "DPadRight" => self.is_down(GamepadButton::DPadRight),
            _ => false,
        }
    }

    pub fn was_pressed_named(self, button: &str) -> bool {
        match button {
            "South" => self.was_pressed(GamepadButton::South),
            "DPadLeft" => self.was_pressed(GamepadButton::DPadLeft),
            "DPadRight" => self.was_pressed(GamepadButton::DPadRight),
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GamepadSample {
    /// gilrs model UUID. Multiple physical controllers can share the same value.
    pub uuid: [u8; 16],
    /// gilrs ID, unique and stable for the lifetime of its Gilrs context.
    pub device_id: usize,
    pub left_stick_x: f32,
    pub dpad_left_down: bool,
    pub dpad_right_down: bool,
    pub south_down: bool,
}

/// Converts the standardized gilrs axis range [-1, 1] into a drift-free,
/// rescaled value while preserving the complete usable range outside the deadzone.
pub fn normalize_axis(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }

    let value = value.clamp(-1.0, 1.0);
    let magnitude = value.abs();
    if magnitude <= GAMEPAD_DEADZONE {
        0.0
    } else {
        value.signum() * ((magnitude - GAMEPAD_DEADZONE) / (1.0 - GAMEPAD_DEADZONE))
    }
}

#[derive(Default)]
pub struct GamepadStateTracker {
    active_device: Option<([u8; 16], usize)>,
    previous: GamepadState,
    state: GamepadState,
}

impl GamepadStateTracker {
    pub fn update(&mut self, sample: Option<GamepadSample>) -> GamepadState {
        let Some(sample) = sample else {
            self.active_device = None;
            self.previous = GamepadState::default();
            self.state = GamepadState::default();
            return self.state;
        };

        let device_key = (sample.uuid, sample.device_id);
        let same_device = self.active_device == Some(device_key);
        let south_pressed = same_device && sample.south_down && !self.previous.south_down;
        let dpad_left_pressed =
            same_device && sample.dpad_left_down && !self.previous.dpad_left_down;
        let dpad_right_pressed =
            same_device && sample.dpad_right_down && !self.previous.dpad_right_down;

        self.active_device = Some(device_key);
        self.state = GamepadState {
            connected: true,
            left_stick_x: normalize_axis(sample.left_stick_x),
            dpad_left_down: sample.dpad_left_down,
            dpad_right_down: sample.dpad_right_down,
            south_down: sample.south_down,
            dpad_left_pressed,
            dpad_right_pressed,
            south_pressed,
        };
        self.previous = self.state;
        self.state
    }

    pub fn state(&self) -> GamepadState {
        self.state
    }
}

/// Selects the lowest model UUID, then the lowest Gilrs ID for identical models.
/// The ID tiebreak is stable for the current Gilrs context and does not depend on
/// iterator order or input event arrival order.
fn select_active_gamepad(
    samples: impl IntoIterator<Item = GamepadSample>,
) -> Option<GamepadSample> {
    samples
        .into_iter()
        .min_by_key(|sample| (sample.uuid, sample.device_id))
}

/// Optional gamepad backend. Its failure is non-fatal: keyboard input remains available.
pub struct GamepadManager {
    gilrs: Option<Gilrs>,
    tracker: GamepadStateTracker,
}

impl GamepadManager {
    pub fn new() -> Self {
        let gilrs = match Gilrs::new() {
            Ok(gilrs) => Some(gilrs),
            Err(error) => {
                eprintln!("Gamepad input is unavailable; continuing with keyboard: {error}");
                None
            }
        };
        Self {
            gilrs,
            tracker: GamepadStateTracker::default(),
        }
    }

    pub fn update(&mut self) -> GamepadState {
        let Some(gilrs) = self.gilrs.as_mut() else {
            return self.tracker.update(None);
        };

        while let Some(event) = gilrs.next_event() {
            match event.event {
                EventType::Connected => eprintln!("Gamepad connected: {}", event.id),
                EventType::Disconnected => eprintln!("Gamepad disconnected: {}", event.id),
                _ => {}
            }
        }

        // A stable hardware UUID, rather than the enumeration order or the last event,
        // makes controller selection deterministic when several devices are connected.
        let selected = gilrs
            .gamepads()
            .filter(|(_, gamepad)| gamepad.is_connected())
            .map(|(id, gamepad)| GamepadSample {
                uuid: gamepad.uuid(),
                device_id: usize::from(id),
                left_stick_x: gamepad.value(Axis::LeftStickX),
                dpad_left_down: gamepad.is_pressed(Button::DPadLeft),
                dpad_right_down: gamepad.is_pressed(Button::DPadRight),
                south_down: gamepad.is_pressed(Button::South),
            });
        let selected = select_active_gamepad(selected);

        self.tracker.update(selected)
    }

    pub fn state(&self) -> GamepadState {
        self.tracker.state()
    }
}

impl Default for GamepadManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(uuid: u8, device_id: usize, south_down: bool) -> GamepadSample {
        GamepadSample {
            uuid: [uuid; 16],
            device_id,
            south_down,
            ..GamepadSample::default()
        }
    }

    #[test]
    fn analog_deadzone_suppresses_drift_and_rescales_full_range() {
        assert_eq!(normalize_axis(0.0), 0.0);
        assert_eq!(normalize_axis(0.17), 0.0);
        assert_eq!(normalize_axis(-0.18), 0.0);
        assert!((normalize_axis(0.59) - 0.5).abs() < 0.0001);
        assert_eq!(normalize_axis(1.0), 1.0);
        assert_eq!(normalize_axis(-1.0), -1.0);
        assert_eq!(normalize_axis(f32::NAN), 0.0);
        assert_eq!(normalize_axis(f32::INFINITY), 0.0);
    }

    #[test]
    fn gamepad_buttons_have_held_and_edge_triggered_states() {
        let mut tracker = GamepadStateTracker::default();
        assert!(!tracker.update(Some(sample(1, 1, false))).south_pressed);
        let pressed = tracker.update(Some(sample(1, 1, true)));
        assert!(pressed.south_down);
        assert!(pressed.south_pressed);
        let held = tracker.update(Some(sample(1, 1, true)));
        assert!(held.south_down);
        assert!(!held.south_pressed);
    }

    #[test]
    fn disconnect_clears_held_and_pressed_buttons() {
        let mut tracker = GamepadStateTracker::default();
        tracker.update(Some(sample(1, 1, false)));
        tracker.update(Some(sample(1, 1, true)));
        let disconnected = tracker.update(None);
        assert_eq!(disconnected, GamepadState::default());
    }

    #[test]
    fn identical_model_controllers_use_lowest_gilrs_id_independent_of_iterator_order() {
        let first_order = select_active_gamepad([
            sample(1, 9, true),
            sample(1, 3, false),
            sample(1, 6, false),
        ])
        .unwrap();
        let reverse_order = select_active_gamepad([
            sample(1, 6, false),
            sample(1, 3, false),
            sample(1, 9, true),
        ])
        .unwrap();

        assert_eq!(first_order.device_id, 3);
        assert_eq!(reverse_order, first_order);
    }

    #[test]
    fn switching_between_identical_model_controllers_does_not_synthesize_a_press() {
        let mut tracker = GamepadStateTracker::default();
        tracker.update(Some(sample(1, 3, false)));
        let switched = tracker.update(Some(sample(1, 9, true)));
        assert!(switched.connected);
        assert!(switched.south_down);
        assert!(!switched.south_pressed);
    }

    #[test]
    fn switching_active_controller_does_not_synthesize_a_button_press() {
        let mut tracker = GamepadStateTracker::default();
        tracker.update(Some(sample(1, 1, false)));
        let switched = tracker.update(Some(sample(2, 2, true)));
        assert!(switched.connected);
        assert!(switched.south_down);
        assert!(!switched.south_pressed);
    }

    #[test]
    fn keyboard_mapping_uses_physical_platform_neutral_codes() {
        assert_eq!(key_name(KeyCode::ArrowLeft), Some("ArrowLeft"));
        assert_eq!(key_name(KeyCode::Space), Some("Space"));
        assert_eq!(key_name(KeyCode::KeyA), Some("A"));
        assert_eq!(key_name(KeyCode::Escape), None);
    }
}
