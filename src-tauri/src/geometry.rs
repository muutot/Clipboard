use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowWorkArea {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[allow(clippy::too_many_arguments)]
fn window_intersection_area(
    ax: i64,
    ay: i64,
    ar: i64,
    ab: i64,
    bx: i64,
    by: i64,
    br: i64,
    bb: i64,
) -> i64 {
    let ix = ax.max(bx);
    let iy = ay.max(by);
    let ir = ar.min(br);
    let ib = ab.min(bb);
    if ix < ir && iy < ib {
        (ir - ix) * (ib - iy)
    } else {
        0
    }
}

fn window_center_distance_squared(win: &WindowPosition, area: &WindowWorkArea) -> i64 {
    let cx = win.x as i64 + win.width as i64 / 2;
    let cy = win.y as i64 + win.height as i64 / 2;
    let ax = area.x as i64 + area.width as i64 / 2;
    let ay = area.y as i64 + area.height as i64 / 2;
    let dx = cx.saturating_sub(ax);
    let dy = cy.saturating_sub(ay);
    // Saturating: config-controlled coordinates can be extreme, and this is
    // only a tie-breaker, so a clamped distance is fine.
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

fn clamp_window_axis(pos: i32, size: u32, area_pos: i32, area_size: u32) -> i32 {
    // Compute in i64: config-controlled coordinates can be near i32::MAX and
    // the intermediate sums would otherwise overflow.
    let lower = area_pos as i64;
    let upper = area_pos as i64 + area_size as i64 - size as i64;
    (pos as i64).max(lower).min(upper).max(lower) as i32
}

pub fn clamp_window_position_to_work_areas(
    window: WindowPosition,
    work_areas: &[WindowWorkArea],
) -> WindowPosition {
    const MIN_WIDTH: u32 = 730;
    const MIN_HEIGHT: u32 = 500;

    if let Some(area) = work_areas
        .iter()
        .map(|area| {
            let wr = window.x as i64 + window.width as i64;
            let wb = window.y as i64 + window.height as i64;
            let ar = area.x as i64 + area.width as i64;
            let ab = area.y as i64 + area.height as i64;
            let intersection = window_intersection_area(
                window.x as i64,
                window.y as i64,
                wr,
                wb,
                area.x as i64,
                area.y as i64,
                ar,
                ab,
            );
            let dist = window_center_distance_squared(&window, area);
            (intersection, dist, area)
        })
        .max_by(|(int_a, dist_a, _), (int_b, dist_b, _)| {
            int_a.cmp(int_b).then_with(|| dist_b.cmp(dist_a))
        })
        .map(|(_, _, area)| area)
    {
        return WindowPosition {
            x: clamp_window_axis(window.x, window.width, area.x, area.width),
            y: clamp_window_axis(window.y, window.height, area.y, area.height),
            width: window.width.clamp(MIN_WIDTH.min(area.width), area.width),
            height: window
                .height
                .clamp(MIN_HEIGHT.min(area.height), area.height),
        };
    }

    WindowPosition {
        width: window.width.max(MIN_WIDTH),
        height: window.height.max(MIN_HEIGHT),
        ..window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_work_area_does_not_panic() {
        let window = WindowPosition {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        let areas = vec![WindowWorkArea {
            x: 0,
            y: 0,
            width: 500,
            height: 400,
        }];
        let result = clamp_window_position_to_work_areas(window, &areas);
        assert_eq!(result.width, 500);
        assert_eq!(result.height, 400);
    }

    #[test]
    fn window_larger_than_area_is_capped_to_area() {
        let window = WindowPosition {
            x: 0,
            y: 0,
            width: 2000,
            height: 2000,
        };
        let areas = vec![WindowWorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        }];
        let result = clamp_window_position_to_work_areas(window, &areas);
        assert_eq!(result.width, 1920);
        assert_eq!(result.height, 1040);
    }

    #[test]
    fn extreme_coordinates_do_not_overflow() {
        // Config-controlled values with no bounds check: the sums must be
        // computed in i64 or a debug build panics (and release wraps).
        let window = WindowPosition {
            x: i32::MAX - 10,
            y: i32::MAX - 10,
            width: 2_000_000_000,
            height: 2_000_000_000,
        };
        let areas = vec![WindowWorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];

        let result = clamp_window_position_to_work_areas(window, &areas);

        assert!(result.width <= 1920);
        assert!(result.height <= 1080);
    }

    #[test]
    fn no_work_areas_keeps_minimum_size() {
        let window = WindowPosition {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let result = clamp_window_position_to_work_areas(window, &[]);
        assert_eq!(result.width, 730);
        assert_eq!(result.height, 500);
    }
}
