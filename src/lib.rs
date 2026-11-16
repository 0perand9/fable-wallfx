//! The maps behind the wallpaper transitions that `shell/Transition.qml` draws.
//!
//! Ported from `wallfx` in Diego Malagrida's pocket-dots
//! (https://github.com/diegoMalagrida/pocket-dots, GPL-3.0), where it is Python.
//!
//! A transition is a shader that decides, pixel by pixel, WHEN the new
//! wallpaper arrives. That "when" is a map, made here; the shader only reads
//! it. Two kinds:
//!
//!   contagion  any wallpaper: colonies land at random places and eat through
//!              the old one (contagion.rs);
//!   outline    a wallpaper with a subject: its outline is drawn, it fills in,
//!              and the rest spreads out of it (outline.rs). The subject is
//!              cut out of every wallpaper ahead of time (cutout.rs).
//!
//! `wallfx.map {to}` → `{ kind, field, line }`: the kind of transition the
//! wallpaper `to` gets, and its two maps (PPM images). A contagion pair is
//! always made ahead, so a change never waits for it, and none repeats.

mod contagion;
mod cutout;
mod field;
mod outline;

use fable_plugin::tokio::sync::Mutex;
use fable_plugin::tokio::task::spawn_blocking;
use fable_plugin::*;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// The files of a kind rotate among this many names: the pair on screen, the
/// one made ahead, and the one being made, never share one.
const SLOTS: u64 = 3;
static CONTAGIONS: AtomicU64 = AtomicU64::new(0);
static OUTLINES: AtomicU64 = AtomicU64::new(0);

/// The contagion pair made ahead. Whoever is making the next one holds the
/// lock, so a change that comes too soon waits for it.
static AHEAD: Mutex<Option<Value>> = Mutex::const_new(None);

pub fn plugin() -> Plugin {
    Plugin::new()
        .run(|ctx| async move {
            let mut ahead = AHEAD.lock().await;
            *ahead = Some(contagion(&ctx).await?);
            drop(ahead);
            cutout::run(ctx).await
        })
        .call(|ctx, method, p| async move {
            match method.as_str() {
                "map" => map(ctx, p.str_or("to", "")).await,
                _ => bail!("unknown method {}.{method}", ctx.name()),
            }
        })
}

/// The maps for a change to the wallpaper `to`.
async fn map(ctx: Ctx, to: &str) -> Result<Value> {
    if let Some(silhouette) = cutout::subject(&ctx, Path::new(to)) {
        return make(&ctx, "outline", &OUTLINES, move |seed, field, line| silhouette.write_maps(seed, field, line)).await;
    }
    let mut ahead = AHEAD.lock().await;
    let pair = match ahead.take() {
        Some(pair) => pair,
        None => contagion(&ctx).await?,
    };
    tokio::spawn(async move {
        *ahead = contagion(&ctx).await.inspect_err(|e| ctx.log(&format!("making a map: {e:#}"))).ok();
    });
    Ok(pair)
}

/// A new contagion pair.
async fn contagion(ctx: &Ctx) -> Result<Value> {
    make(ctx, "contagion", &CONTAGIONS, contagion::write_maps).await
}

/// Has `write` make the next pair of maps of a `kind` (from a seed, to a field
/// and a line file in the plugin's cache folder), the `made`-th of them.
async fn make(ctx: &Ctx, kind: &str, made: &AtomicU64, write: impl FnOnce(u64, &Path, &Path) -> Result<()> + Send + 'static) -> Result<Value> {
    let n = made.fetch_add(1, Ordering::SeqCst);
    let file = |map: &str| ctx.cache_dir().join(format!("{kind}-{map}-{}.ppm", n % SLOTS));
    let (field, line) = (file("field"), file("line"));
    let seed = util::now_ms() ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let (f, l) = (field.clone(), line.clone());
    spawn_blocking(move || write(seed, &f, &l)).await??;
    Ok(json!({ "kind": kind, "field": field, "line": line }))
}
