//! The contagion maps, for any wallpaper: a handful of colonies land at random
//! places and moments and eat through the old wallpaper along a noise field.
//! It is Dijkstra over that noise, on a grid of crisp cells, ranked so that
//! the covered area grows evenly. It does not depend on the picture.
//!
//! Ported from `contagion_map` in Diego Malagrida's pocket-dots
//! (https://github.com/diegoMalagrida/pocket-dots, home/.local/bin/wallfx, GPL-3.0).
//!
//! `field` holds each cell's moment in 16 bits (R high, G low); `line` the
//! same, blurred, in G (high) and B (low), for the glow riding the front.

use crate::field::{blur, sixteen, value_noise, write_ppm, Rng};
use fable_plugin::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::Path;

/// The grid: 480×270 cells, 4 px each on a 1080p screen, 8 on a 4K one.
const W: usize = 480;
const H: usize = 270;

pub fn write_maps(seed: u64, field: &Path, line: &Path) -> Result<()> {
    let moments = moments(seed);
    let smooth = blur(&moments.iter().map(|&m| m as f64 / 65535.0).collect::<Vec<_>>(), W, H, 2.5);
    write_ppm(field, W, H, moments.iter().flat_map(|m| m.to_be_bytes().into_iter().chain([0])))?;
    write_ppm(line, W, H, smooth.iter().flat_map(|&v| [0].into_iter().chain(sixteen(v))))
}

/// Each cell's moment, 0..=65535, ranked by area: at 30 % of the transition,
/// 30 % of the screen is new. 6–12 colonies, each eating along a noise field
/// with per-cell grain, which is what makes the fronts ragged and full of
/// tendrils.
fn moments(seed: u64) -> Vec<u16> {
    let mut rng = Rng(seed);
    let octaves = [(0.52, 18.0, 65.0), (0.30, 7.0, 20.0), (0.18, 2.0, 6.0)];
    let mut noise = vec![0.0; W * H];
    for (weight, lo, hi) in octaves {
        let scale = rng.range(lo, hi);
        for (n, v) in noise.iter_mut().zip(value_noise(&mut rng, W, H, scale)) {
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
