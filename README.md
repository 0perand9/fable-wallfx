# wallfx

A plugin for [Fableland](https://github.com/0perand9/fableland), compiled into
its daemon and shell. Fableland's docs/PLUGINS.md explains plugins.

Wallpaper transitions drawn by the shell instead of awww, two of them:

- **Outline**, for a wallpaper with a subject: a thin quiet line draws the
  subject's outline, the subject fills in from it, and the rest of the picture
  spreads out of it in crisp pixel cells.
- **Contagion**, for any other: a handful of colonies land at random places and
  moments and eat through the old wallpaper in the same cells, each one burning
  for an instant in the new wallpaper's most vivid colour, with a thin glow
  riding the front. No two are the same.

## Credit

The transitions are **Diego Malagrida's**, from his rice
[diegoMalagrida/pocket-dots](https://github.com/diegoMalagrida/pocket-dots):
its *contagio* and *perfil* modes, in `home/.config/quickshell/WallpaperFx.qml`,
`shaders/wallfx.frag` and `home/.local/bin/wallfx`. Here the shader and the
overlay are adapted to Fableland, and the maps are made in Rust instead of
Python (numpy and OpenCV); only the cut-out itself still runs his way, through
rembg (`src/cutout.py`).

pocket-dots is GPL-3.0, and so is this plugin: see [LICENSE](LICENSE).

### What was changed

- **Maps** (2026-11-16): `wallfx`'s Python is Rust in `src/`: the contagion
  map, and the outline's distance fields, contours and order. They are written
  as PPM instead of PNG, and an outline's are made at the moment of the change
  from the subject's kept silhouette, not stored per wallpaper.
- **Cut-outs** (2026-11-16): a wallpaper is never cut out while it waits to be
  shown. The daemon looks at the whole library ahead, at low priority; a
  wallpaper it has not got to yet changes with contagion. The list of
  wallpapers to leave alone is the setting `wallfx.plain`, not a file.
- **Hand-over**: Fableland's daemon switches awww, not the shell; the shell
  covers the screen and tells it (`Transition.qml`).
- **Colours**: the burning edge and the line take the new wallpaper's colour
  from Fableland's library, not from a pywal palette.
- **Cells**: the outline's cells are as big as the contagion's on any screen.

## Installing

With Fableland's Home Manager module:

```nix
# flake.nix
inputs.fable-wallfx = { url = "github:0perand9/fable-wallfx"; flake = false; };

# home configuration
programs.fableland.plugins = [ inputs.fable-wallfx ];
# for outlines: a Python that has rembg
programs.fableland.settings.wallfx.python = "${pkgs.python3.withPackages (p: [ p.rembg ])}/bin/python3";
```

It needs awww (or swww) as the wallpaper daemon, which Fableland uses anyway.

Outlines need a subject cut out of the picture, which a model does: BiRefNet,
through Python's [rembg](https://github.com/danielgatis/rembg). Name a Python
that has it in `wallfx.python` (above; elsewhere, `pip install "rembg[cpu]"`
into a virtual environment and name its `bin/python`). rembg downloads the
model the first time, about 220 MB, into the plugin's cache. Without it every
wallpaper gets contagion, and Settings says why.

To work on it, clone it into a Fableland checkout as `plugins/wallfx` and run
`plugins/assemble`; then `dev/daemon build && dev/daemon restart` and `dev/window`.

## How it works

Fableland hands every change between two still wallpapers to a transition the
shell has loaded (the `wallpaper.transition` event in Fableland's
docs/PROTOCOL.md). `shell/Transition.qml` loads both images and two maps,
covers the screen with a picture of the old wallpaper, and tells the daemon,
which switches awww underneath at once; the animation then ends on the new one
and steps aside. If anything fails, the daemon changes the wallpaper on its own.

The maps say when each pixel flips; `shell/transition.frag` only reads them.

- **Contagion** (`src/contagion.rs`): Dijkstra over a noise field from 6–12
  colonies, ranked so that the covered area grows evenly. The next map is
  always made ahead.
- **Outline** (`src/outline.rs`): from the subject's silhouette, how deep
  inside it each pixel is, how far from its outline, where along the outline
  (so that the line is drawn like a pen stroke, by two pens from the top of
  each contour), and in what order the outside arrives.
- **Cut-outs** (`src/cutout.rs`, `src/cutout.py`): while outlines are on, the
  daemon runs the model over the stills of the library it has not seen, one at
  a time, on a quarter of the processors at the lowest priority, and keeps each
  silhouette (about 100 KB) in the plugin's cache. A subject counts when it
  takes between 0.8 % and 72 % of the picture and the model is sure of it.

## Using it

Choose **Contagion** under Settings → Appearance → Transition (or press *Use
it* on the plugin's own Settings page): the plugin adds it to that list
through its plugin.json `choices`. Any other choice puts awww's transitions back.

## Settings

| setting | default | |
| --- | --- | --- |
| `wallfx.duration` | `1800` | length of a contagion, in milliseconds; an outline takes a ninth longer |
| `wallfx.glow` | `0.9` | how brightly the contagion's front burns, 0–1 |
| `wallfx.outline` | `true` | outline the subject of a wallpaper that has one |
| `wallfx.python` | `python3` | the Python that has rembg, for the cut-outs |
| `wallfx.plain` | `[]` | file names of wallpapers that get contagion although a subject was found: the model finds *something* in almost any picture, and an outlined bridge reads as a mistake |

`wallpaper.transition` is `"wallfx"` while this is the chosen transition.
Videos, and a change the shell could not cover in time, get a plain fade.

## Methods and topic

- `wallfx.map {to}` → `{ kind, field, line }`: the kind of transition the
  wallpaper `to` gets (`outline` or `contagion`) and its two maps (PPM).
- Topic `wallfx`: `{ cutouts: { state, left, error } }`: how far the cut-outs
  are; `state` is `off`, `working`, `done` or `failed`.
