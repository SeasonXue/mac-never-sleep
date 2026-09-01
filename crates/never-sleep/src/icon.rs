/// Monochrome menu-bar artwork derived from the anthropomorphic sun/moon app identity.
/// macOS recolors the black + alpha template image for light and dark menu bars.
#[cfg(any(test, target_os = "macos"))]
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
                // Template images must be black + alpha; macOS supplies the menu-bar tint.
                rgba[i] = 0;
                rgba[i + 1] = 0;
                rgba[i + 2] = 0;
                rgba[i + 3] = ((covered * 255) / (SAMPLES * SAMPLES)) as u8;
            }
        }
    }

    (rgba, SIZE, SIZE)
}

#[cfg(any(test, target_os = "macos"))]
fn sun_sample(x: f32, y: f32) -> bool {
    let dx = x - 18.0;
    let dy = y - 18.0;
    let radius = (dx * dx + dy * dy).sqrt();

    // A solid core keeps the mark legible at menu-bar scale.
    if radius <= 6.8 {
        return true;
    }

    // Eight bold, evenly tapered rays with a clean gap from the core: a simple
    // sun silhouette that holds up next to other menu-bar glyphs like ChatGPT.
    const RAY_INNER: f32 = 8.6;
    const RAY_OUTER: f32 = 17.0;
    for ray in 0..8 {
        let angle = ray as f32 * std::f32::consts::TAU / 8.0;
        let cos = angle.cos();
        let sin = angle.sin();
        let radial = dx * cos + dy * sin;
        if !(RAY_INNER..=RAY_OUTER).contains(&radial) {
            continue;
        }
        let tangent = (-dx * sin + dy * cos).abs();
        let t = (radial - RAY_INNER) / (RAY_OUTER - RAY_INNER);
        let half_width = 2.2 - t * 1.55;
        if tangent <= half_width {
            return true;
        }
    }
    false
}

#[cfg(any(test, target_os = "macos"))]
fn moon_sample(x: f32, y: f32) -> bool {
    // A clean crescent: a full disc with an offset disc carved out of the right,
    // horns opening toward the trailing edge like the app's moon artwork.
    let body = circle(x, y, 17.0, 18.0, 13.2);
    let bite = circle(x, y, 24.6, 18.0, 12.0);
    body && !bite
}

#[cfg(any(test, target_os = "macos"))]
fn circle(x: f32, y: f32, cx: f32, cy: f32, radius: f32) -> bool {
    (x - cx).powi(2) + (y - cy).powi(2) <= radius.powi(2)
}

#[cfg(target_os = "macos")]
pub fn tray_icon(active: bool) -> tray_icon::Icon {
    let (rgba, width, height) = celestial_icon(active);
    tray_icon::Icon::from_rgba(rgba, width, height).expect("valid celestial tray icon")
}
