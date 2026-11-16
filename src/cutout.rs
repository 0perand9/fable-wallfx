//! Which wallpapers have a subject to outline: the cut-outs, made ahead.
//!
//! A model cuts the subject out of a picture (BiRefNet, through Python's
//! rembg: cutout.py). That takes seconds, and a wallpaper must change at
//! once, so it is never done at the moment of a change: every still of the
//! library is looked at beforehand, one at a time and at low priority, and a
//! wallpaper that was not looked at yet changes with contagion and goes to
//! the front of the queue.
//!
//! What the model found is kept by the picture's path, size and date: a note
//! (`<key>.json`) and, when the subject is clear, its silhouette (`<key>.pbm`).
//!
//! The Python that has rembg is the setting `python`. Without it there are no
//! cut-outs, and every wallpaper gets contagion.

use crate::outline::Silhouette;
use fable_plugin::tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use fable_plugin::tokio::process::Command;
use fable_plugin::tokio::sync::Notify;
use fable_plugin::tokio::task::spawn_blocking;
use fable_plugin::tokio::time::sleep;
use fable_plugin::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

/// Wallpapers that were shown before the model had looked at them: next in line.
static ASKED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static WAKE: Notify = Notify::const_new();

/// Outlines are drawn at all: the setting is on, and wallfx is the chosen transition.
fn wanted(ctx: &Ctx) -> bool {
    ctx.setting("outline") == true && ctx.host().setting("wallpaper.transition") == ctx.name()
}

/// The silhouette of `image`'s subject, if it has a clear one and is to be
/// outlined. A wallpaper the model has not looked at is queued for it.
pub fn subject(ctx: &Ctx, image: &Path) -> Option<Silhouette> {
    let plain = ctx.setting("plain");
    let name = image.file_name()?.to_str()?;
    if !wanted(ctx) || plain.as_array().is_some_and(|names| names.iter().any(|n| n == name)) {
        return None;
    }
    let place = place_of(ctx, image)?;
    if !place.with_extension("json").exists() {
        ASKED.lock().unwrap().push(image.to_path_buf());
        WAKE.notify_one();
    }
    Silhouette::read(&place.with_extension("pbm")).ok()
}

/// Where what the model found in `image` is kept, less the extension. A
/// picture that changes on disk is another picture.
fn place_of(ctx: &Ctx, image: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(image).ok()?;
    let date = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let identity = format!("{}|{}|{date}", std::fs::canonicalize(image).ok()?.display(), meta.len());
    // FNV-1a
    let hash = identity.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3));
    Some(util::ensure_dir(ctx.cache_dir().join("cutouts")).join(format!("{hash:016x}")))
}

/// The stills the model has yet to look at: those asked for first, then the library's.
fn waiting(ctx: &Ctx) -> Vec<PathBuf> {
    let library = ctx.topic("wallpaper").unwrap_or_default();
    let stills = library["items"].as_array().into_iter().flatten().filter(|item| item["video"] != true).filter_map(|item| item["path"].as_str()).map(PathBuf::from);
    let mut waiting: Vec<PathBuf> = std::mem::take(&mut *ASKED.lock().unwrap());
    waiting.extend(stills);
    let mut seen = HashSet::new();
    waiting.retain(|image| seen.insert(image.clone()) && place_of(ctx, image).is_some_and(|place| !place.with_extension("json").exists()));
    waiting
}

/// Says how far the cut-outs are, in the plugin's topic: `{ cutouts: { state,
/// left, error } }`, `state` being `off`, `working`, `done` or `failed`.
fn report(ctx: &Ctx, state: &str, left: usize, error: &str) {
    ctx.publish(json!({ "cutouts": { "state": state, "left": left, "error": error } }));
}

/// Looks at the library, for the life of the daemon.
pub async fn run(ctx: Ctx) -> Result<()> {
    let mut settings = ctx.setting_changes();
    // The Python that could not do it, why, and when. It is tried again once
    // the setting names another, outlines are switched off and on, or an hour
    // has passed (the model may just not have downloaded).
    let mut trouble: Option<(String, String, Instant)> = None;
    loop {
        let python = ctx.setting_str("python");
        if !wanted(&ctx) {
            trouble = None;
            report(&ctx, "off", 0, "");
        } else if trouble.as_ref().is_none_or(|(with, _, at)| *with != python || at.elapsed() > Duration::from_secs(3600)) {
            let images = waiting(&ctx);
            match images.is_empty() {
                true => report(&ctx, "done", 0, ""),
                false => match cut(&ctx, &python, images).await {
                    Ok(()) => {
                        trouble = None;
                        continue;
                    }
                    Err(e) => {
                        let error = format!("{e:#}");
                        if trouble.as_ref().is_none_or(|(_, before, _)| *before != error) {
                            ctx.log(&format!("no cut-outs: {error}"));
                        }
                        report(&ctx, "failed", 0, &error);
                        trouble = Some((python, error, Instant::now()));
                    }
                },
            }
        }
        // Until a setting changes, a wallpaper asks, or the library may have grown.
        tokio::select! {
            _ = settings.changed() => {}
            _ = WAKE.notified() => {}
            _ = sleep(Duration::from_secs(60)) => {}
        }
    }
}

/// Runs the model over `images` and keeps what it finds, for as long as outlines are wanted.
async fn cut(ctx: &Ctx, python: &str, images: Vec<PathBuf>) -> Result<()> {
    let dir = ctx.cache_dir();
    let (mask, log) = (dir.join("mask.pgm"), dir.join("cutout.log"));
    // A quarter of the processors, at the lowest priority: it must not be felt.
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get() / 4).clamp(1, 4);
    let mut model = Command::new("nice")
        .args(["-n", "19", python, "-c", include_str!("cutout.py")])
        .env("U2NET_HOME", &dir)
        .env("OMP_NUM_THREADS", threads.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(&log)?)
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("starting {python}"))?;
    let mut questions = model.stdin.take().context("no way to ask the model")?;
    let mut answers = BufReader::new(model.stdout.take().context("no way to hear the model")?).lines();
    for (done, image) in images.iter().enumerate() {
        if !wanted(ctx) {
            break;
        }
        report(ctx, "working", images.len() - done, "");
        let Some(place) = place_of(ctx, image) else { continue };
        questions.write_all(format!("{}\t{}\n", image.display(), mask.display()).as_bytes()).await?;
        // Libraries may chatter on the same pipe: only the script's own answers count.
        let answer = loop {
            let line = answers.next_line().await?.with_context(|| died(&log))?;
            if line == "ok" || line.starts_with("unreadable:") {
                break line;
            }
        };
        let (image, mask) = (image.clone(), mask.clone());
        spawn_blocking(move || keep(&image, &place, &answer, &mask)).await??;
    }
    Ok(())
}

/// What the model answered about `image`, written down at `place`.
fn keep(image: &Path, place: &Path, answer: &str, mask: &Path) -> Result<()> {
    let subject = answer == "ok" && {
        let (silhouette, clear) = Silhouette::of_mask(mask)?;
        if clear {
            silhouette.write(&place.with_extension("pbm"))?;
        }
        clear
    };
    util::write_json(&place.with_extension("json"), &json!({ "image": image, "subject": subject, "answer": answer }))
}

/// Why the model stopped: the last thing it said on its way out.
fn died(log: &Path) -> String {
    let said = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
    format!("the model stopped: {}", said.rsplit(['\n', '\r']).map(str::trim).find(|line| !line.is_empty()).unwrap_or("without a word"))
}
