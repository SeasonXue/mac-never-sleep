/// Monochrome menu-bar artwork derived from the anthropomorphic sun/moon app identity.
/// macOS recolors the black + alpha template image for light and dark menu bars.
pub fn celestial_icon(active: bool) -> (Vec<u8>, u32, u32) {
    const SIZE: u32 = 36;
    const SAMPLES: u32 = 4;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut covered = 0u32;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let fx = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let fy = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    if if active {
                        moon_sample(fx, fy)
                    } else {
                        sun_sample(fx, fy)
                    } {
                        covered += 1;
                    }
                }
            }
            if covered > 0 {
                let i = ((y * SIZE + x) * 4) as usize;
                rgba[i + 3] = ((covered * 255) / (SAMPLES * SAMPLES)) as u8;
            }
        }
    }

    (rgba, SIZE, SIZE)
}

fn sun_sample(x: f32, y: f32) -> bool {
    let cx = 18.0;
    let cy = 18.0;
    let dx = x - cx;
    let dy = y - cy;
    let radius = (dx * dx + dy * dy).sqrt();
    let mut body = radius <= 8.7;

    // Twelve tapered, slightly offset rays keep the silhouette handmade at menu-bar scale.
    for ray in 0..12 {
        let angle =
            ray as f32 * std::f32::consts::TAU / 12.0 + if ray % 2 == 0 { -0.035 } else { 0.045 };
        let cos = angle.cos();
        let sin = angle.sin();
        let radial = dx * cos + dy * sin;
        let tangent = -dx * sin + dy * cos;
        let length = if ray % 3 == 0 { 14.4 } else { 13.2 };
        let half_width = 1.55 * (1.0 - ((radial - 8.0) / 7.2).clamp(0.0, 0.72));
        if radial >= 7.6 && radial <= length && tangent.abs() <= half_width {
            body = true;
        }
    }

    if !body {
        return false;
    }

    // The watchful eye and crooked smile are transparent cuts in the template silhouette.
    let eye_cut = ellipse(x, y, 20.5, 16.2, 3.3, 1.15);
    let eye_pupil = circle(x, y, 21.1, 16.35, 0.7);
    let mouth_cut = segment_distance(x, y, 19.1, 22.0, 23.1, 21.1) < 0.55;
    let brow_cut = segment_distance(x, y, 18.6, 13.4, 22.7, 12.6) < 0.42;
    (body && !(eye_cut || mouth_cut || brow_cut)) || eye_pupil
}

fn moon_sample(x: f32, y: f32) -> bool {
    let outer = circle(x, y, 17.8, 18.0, 13.2);
    let inner = circle(x, y, 11.2, 15.9, 10.7);
    let body = outer && !inner;
    if !body {
        return false;
    }

    let eye_cut = ellipse(x, y, 21.8, 15.6, 2.9, 1.05);
    let eye_pupil = circle(x, y, 22.1, 15.7, 0.65);
    let mouth_cut = segment_distance(x, y, 20.4, 22.1, 23.5, 21.35) < 0.5;
    let brow_cut = segment_distance(x, y, 20.0, 12.9, 23.7, 12.2) < 0.4;
    (body && !(eye_cut || mouth_cut || brow_cut)) || eye_pupil
}

fn circle(x: f32, y: f32, cx: f32, cy: f32, radius: f32) -> bool {
    (x - cx).powi(2) + (y - cy).powi(2) <= radius.powi(2)
}

fn ellipse(x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> bool {
    ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0
}

fn segment_distance(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let vx = bx - ax;
    let vy = by - ay;
    let wx = x - ax;
    let wy = y - ay;
    let denom = vx * vx + vy * vy;
    let t = if denom == 0.0 {
        0.0
    } else {
        ((wx * vx + wy * vy) / denom).clamp(0.0, 1.0)
    };
    ((x - (ax + t * vx)).powi(2) + (y - (ay + t * vy)).powi(2)).sqrt()
}

#[cfg(target_os = "macos")]
pub fn tray_icon(active: bool) -> tray_icon::Icon {
    let (rgba, width, height) = celestial_icon(active);
    tray_icon::Icon::from_rgba(rgba, width, height).expect("valid celestial tray icon")
}
