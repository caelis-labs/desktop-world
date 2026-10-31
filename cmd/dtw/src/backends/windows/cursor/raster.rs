//! CUA theme.rs palette/stable session mapping and build_default_theme.py's
//! eight DART_OUTLINE cubic segments. Small premultiplied vector raster only;
//! no upstream renderer, bitmap/font payload or animation runtime is linked.
const PALETTE: [[u8; 3]; 9] = [
    [178, 132, 255],
    [247, 132, 170],
    [96, 218, 174],
    [244, 178, 66],
    [76, 204, 224],
    [221, 113, 236],
    [232, 82, 98],
    [184, 220, 54],
    [80, 126, 236],
];
pub(super) fn color(session: &str) -> [u8; 3] {
    let suffix = session.rsplit(['-', '_', '.']).next().unwrap_or(session);
    let index = if let Some(n) = suffix.parse::<u64>().ok().filter(|n| *n > 0) {
        (n - 1) as usize % 9
    } else if suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_alphabetic() {
        (suffix.as_bytes()[0].to_ascii_uppercase() - b'A') as usize % 9
    } else {
        session.chars().fold(2_166_136_261u32, |h, c| {
            (h ^ c as u32).wrapping_mul(16_777_619)
        }) as usize
            % 9
    };
    PALETTE[index]
}
const GLYPHS: [[u8; 5]; 16] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
    [2, 5, 7, 5, 5],
    [6, 5, 6, 5, 6],
    [7, 4, 4, 4, 7],
    [6, 5, 5, 5, 6],
    [7, 4, 6, 4, 7],
    [7, 4, 6, 4, 4],
];
fn polygon() -> Vec<(f64, f64)> {
    // vertex, incoming tangent, outgoing tangent
    let s = [
        ((55., 30.), (0., 0.), (-7., -2.)),
        ((43., 41.), (-1., -8.), (0., 0.)),
        ((64., 98.), (0., 0.), (3., 8.)),
        ((77., 99.), (-4., 7.), (0., 0.)),
        ((86., 79.), (0., 0.), (2., -4.)),
        ((95., 70.), (-4., 2.), (0., 0.)),
        ((108., 63.), (0., 0.), (7., -4.)),
        ((107., 50.), (7., 3.), (0., 0.)),
    ];
    let angle = std::f64::consts::PI / 9.;
    let mut points = Vec::new();
    for i in 0..s.len() {
        let (a, _, out) = s[i];
        let (b, input, _) = s[(i + 1) % s.len()];
        for n in 0..6 {
            let t = n as f64 / 6.;
            let u = 1. - t;
            let x = u * u * u * a.0
                + 3. * u * u * t * (a.0 + out.0)
                + 3. * u * t * t * (b.0 + input.0)
                + t * t * t * b.0
                - 55.;
            let y = u * u * u * a.1
                + 3. * u * u * t * (a.1 + out.1)
                + 3. * u * t * t * (b.1 + input.1)
                + t * t * t * b.1
                - 30.;
            points.push((
                14. + (x * angle.cos() - y * angle.sin()) * 0.42,
                14. + (x * angle.sin() + y * angle.cos()) * 0.42,
            ));
        }
    }
    points
}
fn inside(x: f64, y: f64, poly: &[(f64, f64)]) -> bool {
    let mut found = false;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
            found = !found
        }
    }
    found
}
fn border(x: f64, y: f64, poly: &[(f64, f64)]) -> bool {
    poly.iter().enumerate().any(|(i, a)| {
        let b = poly[(i + 1) % poly.len()];
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t =
            (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy).max(0.00001)).clamp(0., 1.);
        (x - a.0 - t * dx).powi(2) + (y - a.1 - t * dy).powi(2) <= 1.4 * 1.4
    })
}
fn rounded(x: f64, y: f64) -> bool {
    if !(32. ..=90.).contains(&x) || !(43. ..=63.).contains(&y) {
        return false;
    }
    let dx = x - x.clamp(37., 85.);
    let dy = y - y.clamp(48., 58.);
    dx * dx + dy * dy <= 25.
}
pub(super) struct Image {
    pub width: i32,
    pub height: i32,
    pub hotspot: i32,
    pub pixels: Vec<u8>,
}
pub(super) fn image(session: &str, dpi: u32, pulse: bool) -> Image {
    let scale = dpi.clamp(48, 768) as f64 / 96.;
    let width = (94. * scale).ceil() as i32;
    let height = (68. * scale).ceil() as i32;
    let fill = color(session);
    let poly = polygon();
    let badge: Vec<_> = session
        .chars()
        .filter_map(|c| c.to_digit(16))
        .take(4)
        .collect();
    let mut pixels = vec![0; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let mut channels = [0f64; 4];
            for sy in [0.25, 0.75] {
                for sx in [0.25, 0.75] {
                    let (px, py) = ((x as f64 + sx) / scale, (y as f64 + sy) / scale);
                    let mut pixel = [0f64; 4];
                    if pulse && (px - 14.).powi(2) + (py - 14.).powi(2) < 13. * 13. {
                        pixel = [
                            fill[2] as f64 * 0.28,
                            fill[1] as f64 * 0.28,
                            fill[0] as f64 * 0.28,
                            255. * 0.28,
                        ]
                    }
                    if inside(px, py, &poly) || rounded(px, py) {
                        pixel = [fill[2] as f64, fill[1] as f64, fill[0] as f64, 255.]
                    } else if border(px, py, &poly) {
                        pixel = [255.; 4]
                    }
                    if rounded(px, py) {
                        let col = ((px - 38.) / 12.).floor() as i32;
                        let gx = (((px - 38.) % 12.) / 2.).floor() as i32;
                        let gy = ((py - 48.) / 2.).floor() as i32;
                        if (0..4).contains(&col)
                            && (0..3).contains(&gx)
                            && (0..5).contains(&gy)
                            && badge.get(col as usize).is_some_and(|n| {
                                GLYPHS[*n as usize][gy as usize] & (1 << (2 - gx)) != 0
                            })
                        {
                            pixel = [255.; 4]
                        }
                    }
                    for c in 0..4 {
                        channels[c] += pixel[c] / 4.;
                    }
                }
            }
            for c in 0..4 {
                pixels[((y * width + x) * 4) as usize + c] = channels[c].round() as u8;
            }
        }
    }
    Image {
        width,
        height,
        hotspot: (14. * scale).round() as i32,
        pixels,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_palette_and_premultiplied_multidpi_vector() {
        assert_eq!(color("session-1"), PALETTE[0]);
        assert_eq!(color("session-B"), PALETTE[1]);
        for dpi in [96, 120, 192] {
            let normal = image("abcd1234", dpi, false);
            let pulse = image("abcd1234", dpi, true);
            assert_eq!(
                normal.pixels.len(),
                (normal.width * normal.height * 4) as usize
            );
            assert_ne!(normal.pixels, pulse.pixels);
            assert!(normal
                .pixels
                .chunks_exact(4)
                .all(|p| p[..3].iter().all(|c| *c <= p[3])));
            assert!(normal
                .pixels
                .chunks_exact(4)
                .any(|p| p[3] > 0 && p[3] < 255));
            assert_eq!(normal.hotspot, (14. * dpi as f64 / 96.).round() as i32);
        }
    }
}
