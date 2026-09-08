//! Pure native viewport mapping. No input is injected by these functions.
use eframe::egui::{self, Pos2, Rect, Vec2};
use sensor_media::{Input, MouseButton};

pub fn pointer_position(pos: Pos2, rect: Rect, dimensions: [u32; 2]) -> (u32, u32) {
    let normalized = (pos - rect.min) / rect.size();
    let axis = |value: f32, size: u32| {
        (value * size as f32)
            .floor()
            .clamp(0.0, size.saturating_sub(1) as f32) as u32
    };
    (
        axis(normalized.x, dimensions[0]),
        axis(normalized.y, dimensions[1]),
    )
}

pub fn pointer_button(
    event: &egui::Event,
    rect: Rect,
    visible: Rect,
    owns_pointer: bool,
    dimensions: [u32; 2],
    held: &[bool; 3],
) -> Option<(usize, bool, [Input; 2])> {
    let egui::Event::PointerButton {
        pos,
        button,
        pressed: down,
        ..
    } = event
    else {
        return None;
    };
    let (index, button) = match button {
        egui::PointerButton::Primary => (0, MouseButton::Left),
        egui::PointerButton::Secondary => (1, MouseButton::Right),
        egui::PointerButton::Middle => (2, MouseButton::Middle),
        _ => return None,
    };
    let inside = owns_pointer && rect.intersect(visible).contains(*pos);
    if *down == held[index] || !(inside || (!down && held[index])) {
        return None;
    }
    let (x, y) = pointer_position(*pos, rect, dimensions);
    Some((
        index,
        *down,
        [
            Input::Move { x, y },
            Input::Button {
                button,
                down: *down,
            },
        ],
    ))
}

pub fn wheel_delta(unit: egui::MouseWheelUnit, delta: Vec2) -> [i32; 2] {
    // Forward raw wheel input once, never egui's animated/smoothed scroll.
    let scale = match unit {
        egui::MouseWheelUnit::Line => 120.0,
        egui::MouseWheelUnit::Point => 3.0,
        egui::MouseWheelUnit::Page => 360.0,
    };
    let ticks = |value: f32| (value * scale).round().clamp(-12_000.0, 12_000.0) as i32;
    [-ticks(delta.x), ticks(delta.y)]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn click(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    #[test]
    fn rapid_down_and_up_in_one_frame_are_both_preserved() {
        let rect = Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(400.0, 300.0));
        let mut held = [false; 3];
        let mut packets = Vec::new();
        for down in [true, false, true, false] {
            let (index, down, packet) = pointer_button(
                &click(rect.center(), down),
                rect,
                rect,
                true,
                [1920, 1080],
                &held,
            )
            .unwrap();
            held[index] = down;
            packets.extend(packet);
        }
        assert_eq!(packets.len(), 8);
        assert!(matches!(packets[0], Input::Move { x: 960, y: 540 }));
        assert!(matches!(packets[1], Input::Button { down: true, .. }));
        assert!(matches!(packets[3], Input::Button { down: false, .. }));
        assert_eq!(held, [false; 3]);
    }
    #[test]
    fn clipped_or_covered_clicks_are_rejected_but_held_release_is_preserved() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0));
        let visible = Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 100.0));
        assert!(pointer_button(
            &click(rect.center(), true),
            rect,
            visible,
            true,
            [800, 600],
            &[false; 3]
        )
        .is_none());
        assert!(pointer_button(
            &click(visible.center(), true),
            rect,
            visible,
            false,
            [800, 600],
            &[false; 3]
        )
        .is_none());
        let (_, down, packet) = pointer_button(
            &click(egui::pos2(500.0, -10.0), false),
            rect,
            visible,
            false,
            [800, 600],
            &[true, false, false],
        )
        .unwrap();
        assert!(!down);
        assert!(matches!(packet[0], Input::Move { x: 799, y: 0 }));
    }
    #[test]
    fn wheel_notches_are_not_amplified_and_horizontal_direction_is_mapped() {
        assert_eq!(
            wheel_delta(egui::MouseWheelUnit::Line, egui::vec2(1.0, -1.0)),
            [-120, -120]
        );
        assert_eq!(
            wheel_delta(egui::MouseWheelUnit::Point, egui::vec2(0.0, 40.0)),
            [0, 120]
        );
    }
}
