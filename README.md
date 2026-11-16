# wallfx

A plugin for [Fableland](https://github.com/0perand9/fableland), compiled into
its daemon and shell. Fableland's docs/PLUGINS.md explains plugins.

Wallpaper transitions drawn by the shell instead of awww: a handful of colonies
land at random places and moments and eat through the old wallpaper in crisp
pixel cells, each one burning for an instant in the new wallpaper's most vivid
colour, with a thin glow riding the front. No two are the same.

## Credit

The transition is **Diego Malagrida's**, from his rice
[diegoMalagrida/pocket-dots](https://github.com/diegoMalagrida/pocket-dots):
its *contagio* mode, in `home/.config/quickshell/WallpaperFx.qml`,
`shaders/wallfx.frag` and `home/.local/bin/wallfx`. Here the shader and the
overlay are adapted to Fableland, and the maps are made in Rust instead of
Python. His other mode, *perfil* (a character's outline drawn first, cut out
with the BiRefNet model), is not ported.

pocket-dots is GPL-3.0, and so is this plugin: see [LICENSE](LICENSE).

## Installing

With Fableland's Home Manager module:

```nix
# flake.nix
inputs.fable-wallfx = { url = "github:0perand9/fable-wallfx"; flake = false; };

# home configuration
programs.fableland.plugins = [ inputs.fable-wallfx ];
```

It needs awww (or swww) as the wallpaper daemon, which Fableland uses anyway.

To work on it, clone it into a Fableland checkout as `plugins/wallfx` and run
`plugins/assemble`; then `dev/daemon build && dev/daemon restart` and `dev/window`.

## How it works

Fableland hands every change between two still wallpapers to a transition the
shell has loaded (the `wallpaper.transition` event in Fableland's
docs/PROTOCOL.md). `shell/Transition.qml` loads both images and a map, covers
the screen with a picture of the old wallpaper, and tells the daemon, which
switches awww underneath at once; the animation then ends on the new one and
steps aside. If anything fails, the daemon changes the wallpaper on its own.

The map says when each cell flips: Dijkstra over a noise field from 6–12
colonies, ranked so that the covered area grows evenly (`src/lib.rs`). The next
map is always made ahead. `shell/contagion.frag` only reads it.

## Using it

Choose **Contagion** under Settings → Appearance → Transition (or press *Use
it* on the plugin's own Settings page): the plugin adds it to that list
through its plugin.json `choices`. Any other choice puts awww's transitions back.

## Settings

| setting | default | |
| --- | --- | --- |
| `wallfx.duration` | `1800` | length of a transition, in milliseconds |
| `wallfx.glow` | `0.9` | how brightly the front burns, 0–1 |

`wallpaper.transition` is `"wallfx"` while Contagion is chosen. Videos, and a
change the shell could not cover in time, get a plain fade.

## Methods

- `wallfx.map` → `{ map, smooth }`: the next pair of map images (PPM).
