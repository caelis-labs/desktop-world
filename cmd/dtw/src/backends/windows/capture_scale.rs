//! Pick the smallest raster that keeps locally resolvable high-contrast runs.
use crate::computer::ImageTransform;

const LONG_EDGES: [u32; 5] = [256, 512, 800, 1200, 1600];
const TILE: usize = 96;
const STROKE: usize = 16;

fn high_contrast(a: &[u8], b: &[u8]) -> bool {
    a[3] != 0 && b[3] != 0 && (0..3).any(|channel| a[channel].abs_diff(b[channel]) >= 64)
}

pub(super) fn scale(rgba: &[u8], width: u32, height: u32) -> ([u32; 2], Vec<u8>) {
    let longest = width.max(height);
    let mut sizes = LONG_EDGES
        .map(|edge| {
            let scale = f64::from(edge.min(longest)) / f64::from(longest);
            [width, height].map(|extent| (f64::from(extent) * scale).round().max(1.0) as u32)
        })
        .to_vec();
    sizes.dedup();
    let source = [0., 0., f64::from(width), f64::from(height)];
    let maps: Vec<([u32; 2], [Vec<usize>; 2])> = sizes
        .into_iter()
        .map(|size| {
            let transform = ImageTransform { source, size };
            let axes = [0, 1].map(|axis| {
                (0..size[axis])
                    .map(|p| transform.sample_axis(axis, p) as usize)
                    .collect()
            });
            (size, axes)
        })
        .collect();
    let stride = width as usize;
    let last = maps.len() - 1;
    let tiles_wide = stride.div_ceil(TILE);
    let mut tiles = vec![[0u32; LONG_EDGES.len()]; tiles_wide * (height as usize).div_ceil(TILE)];
    if maps.len() > 1 {
        for (axis, length, lines) in [
            (0, width as usize, height as usize),
            (1, height as usize, width as usize),
        ] {
            let counts: Vec<Vec<usize>> = maps
                .iter()
                .map(|(_, axes): &([u32; 2], [Vec<usize>; 2])| {
                    (0..=length)
                        .map(|p| axes[axis].partition_point(|&sample| sample < p))
                        .collect()
                })
                .collect();
            let pixel = |line, pos| {
                let i = if axis == 0 {
                    line * stride + pos
                } else {
                    pos * stride + line
                } * 4;
                &rgba[i..i + 4]
            };
            for line in (0..lines).step_by(2) {
                let sampled_lines: Vec<_> = maps
                    .iter()
                    .map(|(_, axes)| {
                        let orthogonal = &axes[1 - axis];
                        orthogonal[line * orthogonal.len() / lines]
                    })
                    .collect();
                let mut last_edge = None;
                for pos in 1..length {
                    let left = pixel(line, pos - 1);
                    let right = pixel(line, pos);
                    if left[3] == 0 || right[3] == 0 {
                        last_edge = None;
                        continue;
                    }
                    if !high_contrast(left, right) {
                        continue;
                    }
                    if let Some(edge) = last_edge.filter(|edge| pos - edge <= STROKE) {
                        let preserved = |i: usize| {
                            let samples = &counts[i];
                            let (middle, after) = (samples[edge], samples[pos]);
                            if after == middle
                                || middle == samples[edge.saturating_sub(STROKE)]
                                || after == samples[(pos + STROKE).min(length)]
                            {
                                return false;
                            }
                            let xs = &maps[i].1[axis];
                            let row = sampled_lines[i];
                            let inside = pixel(row, xs[middle]);
                            high_contrast(pixel(row, xs[middle - 1]), inside)
                                && high_contrast(inside, pixel(row, xs[after]))
                        };
                        // Compare the same local runs to the largest candidate.
                        // Features already unresolved at that size do not force
                        // it, and unrelated edges cannot offset lost detail.
                        if preserved(last) {
                            let (x, y) = if axis == 0 {
                                (edge, line)
                            } else {
                                (line, edge)
                            };
                            let tile = &mut tiles[y / TILE * tiles_wide + x / TILE];
                            tile[last] += 1;
                            for (i, score) in tile[..last].iter_mut().enumerate() {
                                *score += u32::from(preserved(i));
                            }
                        }
                    }
                    last_edge = Some(pos);
                }
            }
        }
    }
    let choice = (0..last)
        .find(|&i| {
            tiles
                .iter()
                .all(|counts| counts[last] < 5 || counts[i] * 100 >= counts[last] * 92)
        })
        .unwrap_or(last);
    let (size, [xs, ys]) = &maps[choice];
    let mut pixels = Vec::with_capacity(size[0] as usize * size[1] as usize * 4);
    for &y in ys {
        for &x in xs {
            let i = (y * stride + x) * 4;
            pixels.extend_from_slice(&rgba[i..i + 4]);
        }
    }
    (*size, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_toolbar_selects_a_larger_size_than_the_same_blank_4k_canvas() {
        let (width, height) = (3840, 2160);
        let mut pixels = vec![255; width * height * 4];
        let (sparse, out) = scale(&pixels, width as u32, height as u32);
        assert_eq!(sparse, [256, 144]);
        assert_eq!(out.len(), 256 * 144 * 4);
        for y in 120..160 {
            for x in (900..1040).filter(|x| x % 10 >= 5) {
                let start = (y * width + x) * 4;
                pixels[start..start + 3].fill(0);
            }
        }
        let (detailed, out) = scale(&pixels, width as u32, height as u32);
        assert!(detailed[0] > sparse[0]);
        assert!(out.len() < 16 * 1024 * 1024);
    }
}
