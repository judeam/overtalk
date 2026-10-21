//! Cached speaker lanes, using the same clip geometry and peak sampling as the web UI.
use slint::{Color, Image, SharedPixelBuffer};
use tiny_skia::{Paint, PathBuilder, Pixmap, Transform};

const CHIPS: [[u8; 3]; 6] = [
    [15, 154, 159],
    [224, 69, 123],
    [91, 107, 134],
    [94, 140, 49],
    [196, 105, 47],
    [138, 79, 158],
];
const DARK_WAVES: [[u8; 3]; 6] = [
    [63, 198, 198],
    [242, 108, 156],
    [154, 171, 200],
    [156, 204, 99],
    [240, 160, 107],
    [204, 147, 224],
];
const LIGHT_WAVES: [[u8; 3]; 6] = [
    [16, 137, 142],
    [214, 48, 111],
    [70, 85, 111],
    [75, 116, 36],
    [169, 85, 32],
    [116, 64, 138],
];
const DARK_FILLS: [[u8; 3]; 6] = [
    [22, 73, 75],
    [78, 33, 53],
    [45, 57, 76],
    [47, 66, 25],
    [74, 44, 23],
    [61, 37, 71],
];
const LIGHT_FILLS: [[u8; 3]; 6] = [
    [168, 217, 218],
    [238, 179, 205],
    [180, 189, 205],
    [196, 219, 169],
    [240, 198, 166],
    [215, 191, 224],
];
pub(crate) fn chip(index: usize) -> Color {
    let [r, g, b] = CHIPS[index % 6];
    Color::from_rgb_u8(r, g, b)
}
pub(crate) fn ink(index: usize, dark: bool) -> Color {
    let [r, g, b] = if dark { DARK_WAVES } else { LIGHT_WAVES }[index % 6];
    Color::from_rgb_u8(r, g, b)
}

fn paint(rgb: [u8; 3], alpha: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(rgb[0], rgb[1], rgb[2], alpha);
    p
}
fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> tiny_skia::Path {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut p = PathBuilder::new();
    p.move_to(x + r, y);
    p.line_to(x + w - r, y);
    p.quad_to(x + w, y, x + w, y + r);
    p.line_to(x + w, y + h - r);
    p.quad_to(x + w, y + h, x + w - r, y + h);
    p.line_to(x + r, y + h);
    p.quad_to(x, y + h, x, y + h - r);
    p.line_to(x, y + r);
    p.quad_to(x, y, x + r, y);
    p.close();
    p.finish().unwrap()
}
// Area coverage for thin bars avoids tiny-skia's <2px rectangle fast path,
// whose debug assertions reject valid fractional-pixel widths.
fn rectangle(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: &Paint<'_>) {
    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;
    let left = x.max(0.0);
    let right = (x + w).min(width as f32);
    let top = y.max(0.0);
    let bottom = (y + h).min(height as f32);
    if left >= right || top >= bottom {
        return;
    }
    let tiny_skia::Shader::SolidColor(color) = &color.shader else {
        return;
    };
    let rgba = [color.red(), color.green(), color.blue(), color.alpha()];
    let pixels = pixmap.data_mut();
    for row in top.floor() as usize..bottom.ceil() as usize {
        for col in left.floor() as usize..right.ceil() as usize {
            let coverage = ((col as f32 + 1.0).min(right) - (col as f32).max(left))
                * ((row as f32 + 1.0).min(bottom) - (row as f32).max(top));
            let alpha = coverage * rgba[3];
            let offset = (row * width + col) * 4;
            for channel in 0..3 {
                pixels[offset + channel] = (rgba[channel] * alpha * 255.0
                    + pixels[offset + channel] as f32 * (1.0 - alpha))
                    .round() as u8;
            }
            pixels[offset + 3] =
                (alpha * 255.0 + pixels[offset + 3] as f32 * (1.0 - alpha)).round() as u8;
        }
    }
}

pub(crate) struct RenderOptions {
    pub width: usize,
    pub height: u32,
    pub scale: f32,
    pub duration: f64,
    pub index: usize,
    pub dark: bool,
}

pub(crate) fn render(options: RenderOptions, turns: &[(f64, f64)], peaks: &[u8]) -> Image {
    let RenderOptions {
        width,
        height,
        scale,
        duration,
        index,
        dark,
    } = options;
    let mut pixmap =
        Pixmap::new(width.max(1) as u32, height.max(1)).expect("bounded lane dimensions");
    let fill = paint(if dark { DARK_FILLS } else { LIGHT_FILLS }[index % 6], 255);
    let rgb = if dark { DARK_WAVES } else { LIGHT_WAVES }[index % 6];
    let wave = paint(rgb, 255);
    let baseline = paint(rgb, 140);
    let edge = paint(rgb, 56);
    let top = 10.0 * scale;
    let clip_h = (height as f32 - top - 8.0 * scale).max(1.0);
    for &(start, end) in turns {
        let x = (start / duration * width as f64) as f32;
        let w = ((end - start) / duration * width as f64).max(6.0 * scale as f64) as f32;
        if !x.is_finite() || !w.is_finite() || w <= 0.0 {
            continue;
        }
        let path = rounded_rect(x, top, w, clip_h, 4.0 * scale);
        pixmap.fill_path(
            &path,
            &fill,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
        // Keep the border inset, as in the browser clip.
        if w > 2.0 && clip_h > 2.0 {
            let inner = rounded_rect(
                x + 1.0,
                top + 1.0,
                w - 2.0,
                clip_h - 2.0,
                (4.0 * scale - 1.0).max(0.0),
            );
            pixmap.fill_path(
                &path,
                &edge,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
            pixmap.fill_path(
                &inner,
                &fill,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        rectangle(
            &mut pixmap,
            x + 3.0,
            top + clip_h / 2.0 - 0.5,
            (w - 6.0).max(0.0),
            1.0,
            &baseline,
        );
        if peaks.is_empty() || w < 8.0 {
            continue;
        }
        let bars = ((w - 6.0) / 2.2).floor().max(1.0) as usize;
        for i in 0..bars {
            let from = ((start + i as f64 / bars as f64 * (end - start)) * 10.0)
                .floor()
                .max(0.0) as usize;
            let to = (((start + (i + 1) as f64 / bars as f64 * (end - start)) * 10.0)
                .floor()
                .max(0.0) as usize)
                .max(from + 1)
                .min(peaks.len());
            let peak = peaks
                .get(from..to)
                .and_then(|s| s.iter().max())
                .copied()
                .unwrap_or(0);
            let h = (peak as f32 / 255.0 * clip_h * 42.0 / 56.0).max(1.5);
            rectangle(
                &mut pixmap,
                x + 3.0 + i as f32 * 2.2,
                top + (clip_h - h) / 2.0,
                1.25,
                h,
                &wave,
            );
        }
    }
    Image::from_rgba8_premultiplied(SharedPixelBuffer::clone_from_slice(
        pixmap.data(),
        width.max(1) as u32,
        height.max(1),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn silent_and_out_of_range_peaks_are_safe() {
        for width in [1, 2, 32, 749] {
            for height in [1, 40, 45, 72] {
                for peaks in [vec![], vec![0; 10]] {
                    let image = super::render(
                        super::RenderOptions {
                            width,
                            height,
                            scale: 1.125,
                            duration: 60.0,
                            index: 0,
                            dark: true,
                        },
                        &[(1.0, 2.0), (50.0, 55.0)],
                        &peaks,
                    );
                    assert_eq!(image.size().width, width as u32);
                }
            }
        }
    }
}
