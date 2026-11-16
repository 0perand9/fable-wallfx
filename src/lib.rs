//! The maps behind the contagion transition that `shell/Transition.qml` draws.
//!
//! Ported from `wallfx` in Diego Malagrida's pocket-dots
//! (https://github.com/diegoMalagrida/pocket-dots, GPL-3.0), where it is Python.
//!
//! A transition is a shader that decides, pixel by pixel, WHEN the new
//! wallpaper arrives. That "when" is a map, made here; the shader only reads
//! it. A handful of colonies land at random places and moments and eat
//! through the old wallpaper along a noise field: Dijkstra over that noise,
//! on a grid of crisp cells, ranked so that the covered area grows evenly.
//!
//! `wallfx.map` → `{ map, smooth }`: two PPM images (no alpha on purpose: Qt
//! premultiplies textures that have one, and would wreck the numbers). `map`
//! holds each cell's moment in 16 bits (R high, G low); `smooth` the same,
//! blurred, in G (high) and B (low), for the glow riding the front. One pair is
//! always made ahead, so a change never waits for it, and none repeats.

use fable_plugin::tokio::sync::Mutex;
use fable_plugin::tokio::task::spawn_blocking;
use fable_plugin::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// The grid: 480×270 cells, 4 px each on a 1080p screen, 8 on a 4K one.
const W: usize = 480;
const H: usize = 270;
/// Files rotate among this many names: the pair on screen, the one made
/// ahead, and the one being made, never share one.
const SLOTS: u64 = 3;

/// The pair made ahead. Whoever is making the next one holds the lock, so a
/// change that comes too soon waits for it, and names are used in turn.
static AHEAD: Mutex<Option<Value>> = Mutex::const_new(None);
static MADE: AtomicU64 = AtomicU64::new(0);

pub fn plugin() -> Plugin {
    Plugin::new()
        .run(|ctx| async move {
            let mut ahead = AHEAD.lock().await;
            *ahead = Some(make(&ctx).await?);
            Ok(())
        })
        .call(|ctx, method, _| async move {
            match method.as_str() {
                "map" => {
                    let mut ahead = AHEAD.lock().await;
                    let pair = match ahead.take() {
                        Some(pair) => pair,
                        None => make(&ctx).await?,
                    };
                    fable_plugin::tokio::spawn(async move {
                        *ahead = make(&ctx).await.inspect_err(|e| ctx.log(&format!("making a map: {e:#}"))).ok();
                    });
                    Ok(pair)
                }
                _ => bail!("unknown method {}.{method}", ctx.name()),
            }
        })
}

/// A new map pair in the plugin's cache folder.
async fn make(ctx: &Ctx) -> Result<Value> {
    let n = MADE.fetch_add(1, Ordering::SeqCst);
    let dir = ctx.cache_dir();
    let (map, smooth) = (dir.join(format!("map-{}.ppm", n % SLOTS)), dir.join(format!("smooth-{}.ppm", n % SLOTS)));
    let seed = util::now_ms() ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let (m, s) = (map.clone(), smooth.clone());
    spawn_blocking(move || write_pair(&m, &s, &contagion(seed))).await??;
    Ok(json!({ "map": map, "smooth": smooth }))
}

// ---------------------------------------------------------------- the map

/// SplitMix64: all the randomness a map needs.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// Smooth value noise at `scale` cells per lattice step.
fn value_noise(rng: &mut Rng, scale: f64) -> Vec<f64> {
    let nx = (W as f64 / scale) as usize + 3;
    let ny = (H as f64 / scale) as usize + 3;
    let lattice: Vec<f64> = (0..nx * ny).map(|_| rng.unit()).collect();
    let at = |x: usize, y: usize| lattice[y * nx + x];
    let ease = |f: f64| f * f * (3.0 - 2.0 * f);
    let mut out = Vec::with_capacity(W * H);
    for y in 0..H {
        let fy = y as f64 / scale;
        let (iy, ty) = (fy as usize, ease(fy.fract()));
        for x in 0..W {
            let fx = x as f64 / scale;
            let (ix, tx) = (fx as usize, ease(fx.fract()));
            let top = at(ix, iy) * (1.0 - tx) + at(ix + 1, iy) * tx;
            let bottom = at(ix, iy + 1) * (1.0 - tx) + at(ix + 1, iy + 1) * tx;
            out.push(top * (1.0 - ty) + bottom * ty);
        }
    }
    out
}

/// Each cell's moment, 0..=65535, ranked by area: at 30 % of the transition,
/// 30 % of the screen is new. 6–12 colonies, each eating along a noise field
/// with per-cell grain, which is what makes the fronts ragged and full of
/// tendrils.
fn contagion(seed: u64) -> Vec<u16> {
    let mut rng = Rng(seed);
    let octaves = [(0.52, 18.0, 65.0), (0.30, 7.0, 20.0), (0.18, 2.0, 6.0)];
    let mut noise = vec![0.0; W * H];
    for (weight, lo, hi) in octaves {
        let scale = rng.range(lo, hi);
        for (n, v) in noise.iter_mut().zip(value_noise(&mut rng, scale)) {
            *n += weight * v;
        }
    }
    let (steep, grain) = (rng.range(5.0, 9.0), rng.range(0.15, 0.65));
    let cost: Vec<f64> = noise.iter().map(|n| 0.12 + (steep * (n - 0.5)).exp() + grain * rng.unit()).collect();

    let colonies = 6 + (rng.next() % 7) as usize;
    let latest = ((W * H) as f64).sqrt() * 0.12;
    let seeds: Vec<(usize, f64)> = (0..colonies)
        .map(|_| {
            let x = (rng.range(0.06, 0.94) * W as f64) as usize;
            let y = (rng.range(0.08, 0.92) * H as f64) as usize;
            (y * W + x, rng.range(0.0, latest))
        })
        .collect();
    ranked(&arrival(&cost, &seeds))
}

/// Dijkstra over the cost grid, eight neighbours, from (cell, start time) seeds.
fn arrival(cost: &[f64], seeds: &[(usize, f64)]) -> Vec<f64> {
    use std::f64::consts::SQRT_2;
    const STEPS: [(isize, isize, f64); 8] =
        [(-1, 0, 1.0), (1, 0, 1.0), (0, -1, 1.0), (0, 1, 1.0), (-1, -1, SQRT_2), (1, -1, SQRT_2), (-1, 1, SQRT_2), (1, 1, SQRT_2)];
    let mut time = vec![f64::INFINITY; W * H];
    // Times are never negative, so their bits order like the numbers.
    let mut heap = BinaryHeap::new();
    for &(i, t) in seeds {
        if t < time[i] {
            time[i] = t;
            heap.push(Reverse((t.to_bits(), i)));
        }
    }
    while let Some(Reverse((bits, i))) = heap.pop() {
        let t = f64::from_bits(bits);
        if t > time[i] {
            continue;
        }
        let (x, y) = ((i % W) as isize, (i / W) as isize);
        for (dx, dy, d) in STEPS {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= W as isize || ny >= H as isize {
                continue;
            }
            let j = ny as usize * W + nx as usize;
            let candidate = t + (cost[i] + cost[j]) * 0.5 * d;
            if candidate < time[j] {
                time[j] = candidate;
                heap.push(Reverse((candidate.to_bits(), j)));
            }
        }
    }
    time
}

fn ranked(time: &[f64]) -> Vec<u16> {
    let mut order: Vec<usize> = (0..time.len()).collect();
    order.sort_by(|&a, &b| time[a].total_cmp(&time[b]));
    let mut rank = vec![0u16; time.len()];
    let last = (time.len() - 1) as f64;
    for (r, &i) in order.iter().enumerate() {
        rank[i] = (r as f64 / last * 65535.0) as u16;
    }
    rank
}

/// Separable Gaussian, edges extended.
fn blur(v: &[f64], sigma: f64) -> Vec<f64> {
    let r = (3.0 * sigma) as isize + 1;
    let mut kernel: Vec<f64> = (-r..=r).map(|x| (-(x * x) as f64 / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f64 = kernel.iter().sum();
    kernel.iter_mut().for_each(|k| *k /= sum);
    let pass = |src: &[f64], horizontal: bool| -> Vec<f64> {
        let mut out = vec![0.0; W * H];
        for y in 0..H as isize {
            for x in 0..W as isize {
                out[y as usize * W + x as usize] = (-r..=r)
                    .zip(&kernel)
                    .map(|(o, k)| {
                        let (sx, sy) = if horizontal { ((x + o).clamp(0, W as isize - 1), y) } else { (x, (y + o).clamp(0, H as isize - 1)) };
                        k * src[sy as usize * W + sx as usize]
                    })
                    .sum();
            }
        }
        out
    };
    pass(&pass(v, true), false)
}

fn write_pair(map: &Path, smooth: &Path, moments: &[u16]) -> Result<()> {
    let blurred = blur(&moments.iter().map(|&m| m as f64 / 65535.0).collect::<Vec<_>>(), 2.5);
    let smooth16 = blurred.iter().map(|v| (v * 65535.0 + 0.5).clamp(0.0, 65535.0) as u16);
    write_ppm(map, moments.iter().flat_map(|&m| [(m >> 8) as u8, m as u8, 0]))?;
    write_ppm(smooth, smooth16.flat_map(|m| [0, (m >> 8) as u8, m as u8]))
}

fn write_ppm(path: &Path, rgb: impl Iterator<Item = u8>) -> Result<()> {
    let mut bytes = format!("P6\n{W} {H}\n255\n").into_bytes();
    bytes.extend(rgb);
    util::write_atomic(path, &bytes)
}
