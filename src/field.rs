//! Numbers on a grid, which is all a map is: the randomness, the noise and the
//! blur the maps are made of, and the pictures they are written as.
//!
//! The pictures are PPM, without alpha on purpose: Qt premultiplies textures
//! that have one, and would wreck the numbers.

use fable_plugin::*;
use std::path::Path;

/// SplitMix64: all the randomness a map needs.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// Smooth value noise over a `w` × `h` grid, at `scale` cells per lattice step.
pub fn value_noise(rng: &mut Rng, w: usize, h: usize, scale: f64) -> Vec<f64> {
    let nx = (w as f64 / scale) as usize + 3;
    let ny = (h as f64 / scale) as usize + 3;
    let lattice: Vec<f64> = (0..nx * ny).map(|_| rng.unit()).collect();
    let ease = |f: f64| f * f * (3.0 - 2.0 * f);
    let across: Vec<(usize, f64)> = (0..w).map(|x| x as f64 / scale).map(|f| (f as usize, ease(f.fract()))).collect();
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        let fy = y as f64 / scale;
        let (top, bottom) = lattice[fy as usize * nx..].split_at(nx);
        let ty = ease(fy.fract());
        for &(ix, tx) in &across {
            let above = top[ix] * (1.0 - tx) + top[ix + 1] * tx;
            let below = bottom[ix] * (1.0 - tx) + bottom[ix + 1] * tx;
            out.push(above * (1.0 - ty) + below * ty);
        }
    }
    out
}

/// Separable Gaussian, edges extended.
pub fn blur(v: &[f64], w: usize, h: usize, sigma: f64) -> Vec<f64> {
    let r = (3.0 * sigma) as usize + 1;
    let mut kernel: Vec<f64> = (0..=2 * r).map(|i| (-((i as f64 - r as f64).powi(2)) / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f64 = kernel.iter().sum();
    kernel.iter_mut().for_each(|k| *k /= sum);
    // Every line of `n` values one way across the grid; `at` is where a line's value lives.
    let along = |src: &[f64], lines: usize, n: usize, at: &dyn Fn(usize, usize) -> usize| {
        let mut out = vec![0.0; w * h];
        let mut line = vec![0.0; n + 2 * r];
        for l in 0..lines {
            for (i, value) in line.iter_mut().enumerate() {
                *value = src[at(l, i.saturating_sub(r).min(n - 1))];
            }
            for i in 0..n {
                out[at(l, i)] = kernel.iter().zip(&line[i..]).map(|(k, p)| k * p).sum();
            }
        }
        out
    };
    let rows = along(v, h, w, &|y, x| y * w + x);
    along(&rows, w, h, &|x, y| y * w + x)
}

/// A value in 0..=1 as 16 bits, high byte first.
pub fn sixteen(v: f64) -> [u8; 2] {
    ((v * 65535.0 + 0.5).clamp(0.0, 65535.0) as u16).to_be_bytes()
}

pub fn write_ppm(path: &Path, w: usize, h: usize, rgb: impl Iterator<Item = u8>) -> Result<()> {
    let mut bytes = format!("P6\n{w} {h}\n255\n").into_bytes();
    bytes.reserve(w * h * 3);
    bytes.extend(rgb);
    util::write_atomic(path, &bytes)
}

/// A Netpbm picture's size and bytes: a grey one (`P5`) or a bitmap (`P4`).
pub fn read_netpbm(path: &Path, magic: &str) -> Result<(usize, usize, Vec<u8>)> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    // The header is words apart by white space (the bitmap's has no greatest value), then one more space.
    let words = if magic == "P4" { 3 } else { 4 };
    let mut header = Vec::new();
    let mut at = 0;
    while header.len() < words {
        let start = at + bytes[at..].iter().position(|b| !b.is_ascii_whitespace()).context("a short Netpbm header")?;
        at = start + bytes[start..].iter().position(|b| b.is_ascii_whitespace()).context("a short Netpbm header")?;
        header.push(std::str::from_utf8(&bytes[start..at])?);
    }
    if header[0] != magic {
        bail!("{} is {}, not {magic}", path.display(), header[0]);
    }
    Ok((header[1].parse()?, header[2].parse()?, bytes[at + 1..].to_vec()))
}
