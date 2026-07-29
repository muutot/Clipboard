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
    ax: i32,
    ay: i32,
    ar: i32,
    ab: i32,
    bx: i32,
    by: i32,
    br: i32,
    bb: i32,
) -> i64 {
    let ix = ax.max(bx) as i64;
    let iy = ay.max(by) as i64;
    let ir = ar.min(br) as i64;
    let ib = ab.min(bb) as i64;
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
    let dx = cx - ax;
    let dy = cy - ay;
    dx * dx + dy * dy
}

fn clamp_window_axis(pos: i32, size: u32, area_pos: i32, area_size: u32) -> i32 {
    let pos = pos.max(area_pos);
    let pos = pos.min(area_pos + area_size as i32 - size as i32);
    pos.max(area_pos)
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
            let wr = window.x + window.width as i32;
            let wb = window.y + window.height as i32;
            let ar = area.x + area.width as i32;
            let ab = area.y + area.height as i32;
            let intersection =
                window_intersection_area(window.x, window.y, wr, wb, area.x, area.y, ar, ab);
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
            width: window.width.clamp(MIN_WIDTH, area.width),
            height: window.height.clamp(MIN_HEIGHT, area.height),
        };
    }

    WindowPosition {
        width: window.width.max(MIN_WIDTH),
        height: window.height.max(MIN_HEIGHT),
        ..window
    }
}
