/// 菜单栏 template icon：黑 + alpha，系统会按浅色/深色菜单栏着色。
#[allow(dead_code)]
pub fn moon_icon(active: bool) -> (Vec<u8>, u32, u32) {
    const SIZE: u32 = 32;
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    let s = SIZE as f32;
    let cx = s * 0.46;
    let cy = s * 0.50;
    let r_outer = s * 0.34;
    let r_cut = s * 0.30;
    let cut_cx = cx + s * 0.18;
    let stroke = 2.2;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let d_outer = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let d_cut = ((fx - cut_cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let in_moon = d_outer <= r_outer && d_cut > r_cut;
            let on_edge = in_moon && (d_outer > r_outer - stroke || d_cut < r_cut + stroke);
            let mut a = 0.0f32;
            if active {
                if in_moon {
                    a = 1.0;
                }
            } else if on_edge {
                a = 1.0;
            }
            if active {
                let dx = fx - (s * 0.78);
                let dy = fy - (s * 0.72);
                if (dx * dx + dy * dy).sqrt() <= s * 0.09 {
                    a = 1.0;
                }
            }
            if a > 0.0 {
                let i = ((y * SIZE + x) * 4) as usize;
                px[i] = 0;
                px[i + 1] = 0;
                px[i + 2] = 0;
                px[i + 3] = (a * 255.0) as u8;
            }
        }
    }
    (px, SIZE, SIZE)
}

#[cfg(target_os = "macos")]
pub fn tray_icon(active: bool) -> tray_icon::Icon {
    let (rgba, w, h) = moon_icon(active);
    tray_icon::Icon::from_rgba(rgba, w, h).expect("icon")
}
