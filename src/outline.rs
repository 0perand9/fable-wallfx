//! The outline maps, for a wallpaper with a subject: a thin line draws the
//! subject's outline, the subject fills in from that outline, and the rest of
//! the picture spreads out of it.
//!
//! Ported from `perfil` in Diego Malagrida's pocket-dots
//! (https://github.com/diegoMalagrida/pocket-dots, home/.local/bin/wallfx,
//! GPL-3.0), where numpy and OpenCV do the work.
//!
//! Everything comes from the subject's silhouette, in the picture's own space
//! (the shader stretches and interpolates it):
//!
//!   `field`  G     how deep inside the subject: 0 on its outline, 255 at its deepest
//!            B     the signed distance to the outline, ±16 px around it
//!   `line`   R     where along the outline the nearest point of it is, 1..=255,
//!                  for a line drawn by two pens from the top of each contour
//!                  that meet at its far side; 0 where no line belongs
//!            G, B  the order in which the outside arrives, in 16 bits and
//!                  smooth: outwards from the subject, roughened with noise,
//!                  ranked so that the covered area grows evenly

use crate::field::{blur, read_netpbm, sixteen, value_noise, write_ppm, Rng};
use fable_plugin::*;
use std::path::Path;

/// `field` holds the signed distance in ± this many pixels around the outline.
const SD_RANGE: f64 = 16.0;

/// A subject cut out of a picture: which of its pixels are the subject.
pub struct Silhouette {
    w: usize,
    h: usize,
    inside: Vec<bool>,
}

impl Silhouette {
    /// The model's grey mask (white: the subject) as a silhouette, without the
    /// specks, and whether it is a subject worth outlining: something that
    /// takes part of the picture, not almost all of it nor a crumb, and that
    /// the model is sure about.
    pub fn of_mask(path: &Path) -> Result<(Silhouette, bool)> {
        let (w, h, grey) = read_netpbm(path, "P5")?;
        if grey.len() < w * h {
            bail!("{} is cut short", path.display());
        }
        let mut inside: Vec<bool> = grey.iter().map(|&g| g > 127).collect();
        drop_specks(&mut inside, w, h);
        let share = |count: usize| count as f64 / (w * h) as f64;
        let cover = share(inside.iter().filter(|&&i| i).count());
        let unsure = share(grey.iter().filter(|g| (26..230).contains(*g)).count());
        Ok((Silhouette { w, h, inside }, (0.008..=0.72).contains(&cover) && unsure <= 0.06))
    }

    /// Kept as a bitmap (PBM), a bit a pixel.
    pub fn read(path: &Path) -> Result<Silhouette> {
        let (w, h, bits) = read_netpbm(path, "P4")?;
        let row = w.div_ceil(8);
        if bits.len() < row * h {
            bail!("{} is cut short", path.display());
        }
        let inside = (0..w * h).map(|i| bits[i / w * row + i % w / 8] & (0x80 >> (i % w % 8)) != 0).collect();
        Ok(Silhouette { w, h, inside })
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let mut bytes = format!("P4\n{} {}\n", self.w, self.h).into_bytes();
        for row in self.inside.chunks(self.w) {
            bytes.extend(row.chunks(8).map(|bits| bits.iter().enumerate().fold(0, |byte, (i, &bit)| byte | (bit as u8) << (7 - i))));
        }
        util::write_atomic(path, &bytes)
    }

    pub fn write_maps(&self, seed: u64, field: &Path, line: &Path) -> Result<()> {
        let (w, h) = (self.w, self.h);
        let outside: Vec<bool> = self.inside.iter().map(|&i| !i).collect();
        let (depth, _) = nearest(&outside, w, h); // inside the subject: how far out of it
        let (away, _) = nearest(&self.inside, w, h); // outside: how far to it

        // Inwards: 0 on the silhouette, 1 at (almost) its deepest point.
        let mut depths: Vec<f64> = depth.iter().copied().filter(|&d| d > 0.0).collect();
        if depths.is_empty() || depths.len() == w * h {
            bail!("a silhouette of nothing, or of everything");
        }
        let nearly_all = (depths.len() - 1) * 99 / 100;
        let deepest = depths.select_nth_unstable_by(nearly_all, f64::total_cmp).1.max(1.0);
        let signed = |i: usize| away[i] - depth[i];
        write_ppm(field, w, h, (0..w * h).flat_map(|i| [0, unit_byte(depth[i] / deepest), unit_byte((signed(i) + SD_RANGE) / (2.0 * SD_RANGE))]))?;

        // Outwards: the distance to the subject, roughened with noise so that
        // the front is not a perfect offset of the silhouette, then ranked.
        let mut rng = Rng(seed);
        let (coarse, fine) = (value_noise(&mut rng, w, h, w as f64 / 9.0), value_noise(&mut rng, w, h, w as f64 / 28.0));
        let push = 0.08 * w.max(h) as f64;
        let warped: Vec<f64> = (0..w * h)
            .map(|i| {
                let noise = 0.6 * coarse[i] + 0.4 * fine[i];
                away[i] * (0.75 + 0.5 * noise) + noise * push
            })
            .collect();
        let order = blur(&ranked(&warped, &outside), w, h, 4.0);

        // Along the outline: each point of a contour says how far round it is,
        // and the pixels near the outline take the nearest point's word.
        let (on_outline, along) = self.pens();
        let pen: Vec<u8> = match on_outline.iter().any(|&on| on) {
            true => {
                let (_, point) = nearest(&on_outline, w, h);
                (0..w * h).map(|i| if signed(i).abs() < SD_RANGE { along[point[i]] } else { 0 }).collect()
            }
            false => vec![0; w * h],
        };
        write_ppm(line, w, h, (0..w * h).flat_map(|i| [pen[i]].into_iter().chain(sixteen(order[i]))))
    }

    /// Which pixels are on a contour that gets a line, and where along it each
    /// is, 1..=255: every contour is drawn from its top point by two pens going
    /// round in opposite directions, which meet at the far side. 0 where the
    /// subject runs off the picture: its contour runs along the border there,
    /// and that is not an outline.
    fn pens(&self) -> (Vec<bool>, Vec<u8>) {
        let (w, h) = (self.w, self.h);
        let mut on_outline = vec![false; w * h];
        let mut along = vec![0; w * h];
        for contour in contours(&self.inside, w, h) {
            // Tiny contours — the holes between flames, the gaps in a cloud —
            // would be drawn as a spray of dots before anything else: no line.
            if contour.len() < 40 || area(&contour) < 0.0008 * (w * h) as f64 {
                continue;
            }
            // How far round each point is, walking the contour from its first.
            let mut walked = Vec::with_capacity(contour.len());
            let mut length = 0.0;
            for (i, &(x, y)) in contour.iter().enumerate() {
                walked.push(length);
                let (nx, ny) = contour[(i + 1) % contour.len()];
                length += (x.abs_diff(nx) as f64).hypot(y.abs_diff(ny) as f64);
            }
            let top = (0..contour.len()).min_by_key(|&i| contour[i].1).unwrap_or(0);
            for (i, &(x, y)) in contour.iter().enumerate() {
                let from_top = (walked[i] - walked[top]).rem_euclid(length);
                let on_border = x <= 1 || y <= 1 || x + 2 >= w || y + 2 >= h;
                on_outline[y * w + x] = true;
                along[y * w + x] = if on_border { 0 } else { (from_top.min(length - from_top) / (length / 2.0) * 255.0).clamp(1.0, 255.0) as u8 };
            }
        }
        (on_outline, along)
    }
}

/// A value in 0..=1 as a byte.
fn unit_byte(v: f64) -> u8 {
    (v * 255.0).clamp(0.0, 255.0) as u8
}

/// Specks of subject: out. They would be drawn as dots all over the screen.
fn drop_specks(inside: &mut [bool], w: usize, h: usize) {
    let least = 0.002 * (w * h) as f64;
    let mut seen = vec![false; w * h];
    let mut blob = Vec::new();
    for start in 0..w * h {
        if !inside[start] || seen[start] {
            continue;
        }
        // Everything that touches `start`, corners included.
        seen[start] = true;
        blob.clear();
        blob.push(start);
        let mut next = 0;
        while let Some(&at) = blob.get(next) {
            next += 1;
            let (x, y) = (at % w, at / w);
            for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let neighbour = ny * w + nx;
                    if inside[neighbour] && !seen[neighbour] {
                        seen[neighbour] = true;
                        blob.push(neighbour);
                    }
                }
            }
        }
        if (blob.len() as f64) < least {
            blob.iter().for_each(|&at| inside[at] = false);
        }
    }
}

/// For every pixel, how far the nearest pixel of `set` is and which one it is.
/// (Felzenszwalb and Huttenlocher's distance transform: the nearest in each
/// column first, then along each row the lower envelope of the parabolas that
/// stand on those.)
fn nearest(set: &[bool], w: usize, h: usize) -> (Vec<f64>, Vec<usize>) {
    const NONE: (f64, usize) = (1e12, 0);
    // The nearest set pixel in the same column: its squared distance, and itself.
    let mut column = vec![NONE; w * h];
    for x in 0..w {
        let mut sweep = |rows: &mut dyn Iterator<Item = usize>| {
            let mut last = None;
            for y in rows {
                if set[y * w + x] {
                    last = Some(y);
                }
                if let Some(l) = last {
                    let squared = (y.abs_diff(l) * y.abs_diff(l)) as f64;
                    if squared < column[y * w + x].0 {
                        column[y * w + x] = (squared, l * w + x);
                    }
                }
            }
        };
        sweep(&mut (0..h));
        sweep(&mut (0..h).rev());
    }
    let mut distance = vec![0.0; w * h];
    let mut which = vec![0; w * h];
    let mut roots = vec![0; w]; // the columns whose parabolas make the envelope, left to right
    let mut starts = vec![0.0; w + 1]; // where each takes over from the one before it
    for y in 0..h {
        let height = |x: usize| column[y * w + x].0;
        // where the parabola on column `q` goes under the one on column `p`
        let meet = |p: usize, q: usize| ((height(q) + (q * q) as f64) - (height(p) + (p * p) as f64)) / (2 * (q - p)) as f64;
        let mut k = 0;
        roots[0] = 0;
        starts[0] = f64::NEG_INFINITY;
        starts[1] = f64::INFINITY;
        for q in 1..w {
            let mut from = meet(roots[k], q);
            while from <= starts[k] {
                k -= 1;
                from = meet(roots[k], q);
            }
            k += 1;
            roots[k] = q;
            starts[k] = from;
            starts[k + 1] = f64::INFINITY;
        }
        k = 0;
        for x in 0..w {
            while starts[k + 1] < x as f64 {
                k += 1;
            }
            let root = roots[k];
            distance[y * w + x] = ((x.abs_diff(root) * x.abs_diff(root)) as f64 + height(root)).sqrt();
            which[y * w + x] = column[y * w + root].1;
        }
    }
    (distance, which)
}

/// The place of each pixel of `among` in the order of `values`, 0..=1: the
/// share of them that come before it. 0 for the other pixels.
fn ranked(values: &[f64], among: &[bool]) -> Vec<f64> {
    // Counted in buckets far finer than the 16 bits the answer is kept in.
    const BUCKETS: usize = 1 << 20;
    let chosen = || values.iter().zip(among).filter(|(_, &a)| a).map(|(&v, _)| v);
    let greatest = chosen().fold(f64::MIN_POSITIVE, f64::max);
    let bucket = |v: f64| (v / greatest * (BUCKETS - 1) as f64) as usize;
    let mut before = vec![0usize; BUCKETS + 1];
    chosen().for_each(|v| before[bucket(v) + 1] += 1);
    for i in 1..=BUCKETS {
        before[i] += before[i - 1];
    }
    let last = before[BUCKETS].saturating_sub(1).max(1) as f64;
    values.iter().zip(among).map(|(&v, &a)| if a { before[bucket(v)] as f64 / last } else { 0.0 }).collect()
}

/// Every border of the silhouette, its outer ones and those of its holes, as
/// the silhouette's pixels along it in order (Suzuki and Abe's border
/// following, which is OpenCV's findContours).
fn contours(inside: &[bool], w: usize, h: usize) -> Vec<Vec<(usize, usize)>> {
    // A frame of background all round, so that nothing looks outside the grid.
    // 1: silhouette no border has passed yet; other numbers: the border that did.
    let stride = w + 2;
    let mut grid = vec![0i32; stride * (h + 2)];
    for (i, _) in inside.iter().enumerate().filter(|(_, &inside)| inside) {
        grid[(i / w + 1) * stride + i % w + 1] = 1;
    }
    // The eight neighbours, clockwise from the one on the right.
    let around = [1, stride as isize + 1, stride as isize, stride as isize - 1, -1, -(stride as isize) - 1, -(stride as isize), -(stride as isize) + 1];
    let step = |from: usize, turn: usize| (from as isize + around[turn % 8]) as usize;
    let turn_to = |from: usize, to: usize| around.iter().position(|&by| from as isize + by == to as isize).unwrap_or(0);

    let mut contours = Vec::new();
    let mut number = 1;
    for start in stride + 1..stride * (h + 1) - 1 {
        // An outer border begins where background lies left of silhouette no
        // border has passed, a hole's where background lies right of silhouette.
        let background = if grid[start] == 1 && grid[start - 1] == 0 {
            start - 1
        } else if grid[start] >= 1 && grid[start + 1] == 0 {
            start + 1
        } else {
            continue;
        };
        number += 1;
        // Clockwise from that background, the first silhouette round `start`.
        let turn = turn_to(start, background);
        let Some(first) = (0..8).map(|i| step(start, turn + i)).find(|&at| grid[at] != 0) else {
            grid[start] = -number; // a lone pixel
            continue;
        };
        let mut contour = Vec::new();
        let (mut from, mut at) = (first, start);
        loop {
            // Counterclockwise from the one after `from`, the next silhouette round `at`.
            let turn = turn_to(at, from);
            let mut right_is_background = false;
            let mut next = from;
            for i in 1..=8 {
                let look = (turn + 8 - i) % 8;
                if grid[step(at, look)] != 0 {
                    next = step(at, look);
                    break;
                }
                right_is_background |= look == 0;
            }
            if right_is_background {
                grid[at] = -number;
            } else if grid[at] == 1 {
                grid[at] = number;
            }
            contour.push((at % stride - 1, at / stride - 1));
            if next == start && at == first {
                break;
            }
            (from, at) = (at, next);
        }
        contours.push(contour);
    }
    contours
}

/// The area a contour encloses.
fn area(contour: &[(usize, usize)]) -> f64 {
    let twice: i64 = (0..contour.len())
        .map(|i| {
            let ((x, y), (nx, ny)) = (contour[i], contour[(i + 1) % contour.len()]);
            x as i64 * ny as i64 - nx as i64 * y as i64
        })
        .sum();
    twice.abs() as f64 / 2.0
}
