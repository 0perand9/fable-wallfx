import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.core
import qs.widgets

// The transitions between two still wallpapers, in a shader (transition.frag)
// with the maps of the plugin's daemon side (`wallfx.map`):
//
//   contagion  colonies landing at random places eat through the old
//              wallpaper in crisp cells, each one burning for an instant in
//              the new wallpaper's most vivid colour;
//   outline    if the new wallpaper has a subject: a thin quiet line draws its
//              outline, it fills in, and the rest spreads out of it with the
//              same cells.
//
// From Diego Malagrida's pocket-dots (https://github.com/diegoMalagrida/pocket-dots,
// home/.config/quickshell/WallpaperFx.qml, GPL-3.0).
//
// It draws in a layer ABOVE the wallpaper daemon and below the windows, only
// for the length of a change. The daemon hands each change over (the
// `wallpaper.transition` event, docs/PROTOCOL.md), and the order is what makes
// it seamless:
//   1. load both images and the maps, off the main thread;
//   2. start the transition and, at the same time, fade in over the wallpaper.
//      At t≈0 the transition IS the old wallpaper, so the fade is between two
//      renders of the same image and nobody sees it;
//   3. once covered, tell the daemon (`wallpaper.covered`): it switches the
//      wallpaper underneath, instantly;
//   4. the transition ends on the new image, which the wallpaper now shows too
//      (`wallpaper.swapped`);
//   5. fade out, and the textures are released.
// Whatever goes wrong, this steps aside: the daemon changes the wallpaper on
// its own when it is not told the screen is covered.
Scope {
    id: root

    readonly property int fadeMs: 130
    property int change: 0          // the daemon's id for the change being drawn
    property string from: ""
    property string to: ""
    property var outputs: []
    property string kind: "contagion"
    property string fieldPath: ""
    property string linePath: ""
    // idle → loading → run (→ held, until the wallpaper has switched) → out → idle
    property string phase: "idle"
    property real t: 0
    property real fade: 0
    property bool swapped: false
    // Taken from the new wallpaper when the change starts: the contagion's
    // burning edge and the outline's line.
    property color ember: "orange"
    property color ink: "white"
    property real seed: 0
    property int readyCount: 0
    readonly property bool active: phase !== "idle"
    readonly property var targets: Quickshell.screens.filter(screen => outputs.length === 0 || outputs.includes(screen.name))

    // A wallpaper's colour can be dark; the burning edge has to light up.
    function lifted(c) {
        return Qt.hsla(c.hslHue, Math.max(c.hslSaturation, 0.55), Math.min(Math.max(c.hslLightness, 0.62), 0.82), 1);
    }

    // The line is quiet: almost white, with a little of the wallpaper in it.
    function pale(c) {
        return Qt.hsla(Math.max(c.hslHue, 0), Math.min(c.hslSaturation, 0.3), 0.9, 1);
    }

    // The new wallpaper's colour, from the library (the palette follows the
    // change, too late to be used here).
    function colorOf(path) {
        const item = (Daemon.wallpaper.items ?? []).find(i => i.path === path);
        return item && item.color ? Qt.color(item.color) : Theme.accent;
    }

    function start(data) {
        finish();
        change = data.id;
        outputs = data.outputs ?? [];
        if (targets.length === 0)
            return;
        phase = "loading";
        watchdog.restart();
        const color = colorOf(data.to);
        ember = lifted(color);
        ink = pale(color);
        seed = Math.random() * 100;
        // The pictures load while the maps are made.
        from = data.from;
        to = data.to;
        const id = change;
        Daemon.call("wallfx.map", { to: data.to }, (maps, error) => {
            if (id !== change || phase !== "loading")
                return;
            if (!maps) {
                console.warn("wallfx: no map:", error);
                finish();
                return;
            }
            kind = maps.kind;
            fieldPath = maps.field;
            linePath = maps.line;
        });
    }

    function screenReady() {
        if (phase !== "loading" || ++readyCount < targets.length)
            return;
        watchdog.stop();
        phase = "run";
        // The outline has more to tell: a ninth longer (pocket-dots' 2 s to 1.8 s).
        run.duration = Math.max(300, Number(Daemon.setting("wallfx.duration", 1800))) * (kind === "outline" ? 10 / 9 : 1);
        run.restart();
        fadeIn.restart();
    }

    // Uncover only when the transition has ended AND the wallpaper shows the new one.
    function maybeFinish() {
        if (run.running || !(phase === "run" || phase === "held"))
            return;
        if (!swapped) {
            phase = "held";
            watchdog.restart();
            return;
        }
        phase = "out";
        fadeOut.restart();
    }

    function finish() {
        fadeIn.stop();
        run.stop();
        fadeOut.stop();
        watchdog.stop();
        settle.stop();
        phase = "idle";
        fade = 0;
        t = 0;
        swapped = false;
        readyCount = 0;
        from = to = fieldPath = linePath = "";
    }

    // While this is the chosen transition (Settings → Appearance), the daemon
    // hands wallpaper changes over.
    Want {
        topic: "wallpaper.transition"
        active: Daemon.setting("wallpaper.transition", "") === "wallfx"
    }

    Connections {
        target: Daemon

        function onEvent(name, data) {
            if (name === "wallpaper.transition") {
                root.start(data);
            } else if (name === "wallpaper.swapped" && data.id === root.change && root.active) {
                if (!data.ok)
                    root.finish();
                else
                    settle.restart();
            }
        }
    }

    NumberAnimation {
        id: fadeIn

        target: root
        property: "fade"
        from: 0
        to: 1
        duration: root.fadeMs
        easing.type: Easing.OutQuad
        onFinished: Daemon.send("wallpaper.covered", { id: root.change })
    }

    // `t` is TIME, so it runs linear; the shader eases its own stretch.
    NumberAnimation {
        id: run

        target: root
        property: "t"
        from: 0
        to: 1
        easing.type: Easing.Linear
        onFinished: root.maybeFinish()
    }

    NumberAnimation {
        id: fadeOut

        target: root
        property: "fade"
        from: 1
        to: 0
        duration: root.fadeMs
        easing.type: Easing.OutQuad
        onFinished: root.finish()
    }

    // The wallpaper has the new image; give it two frames to present it before
    // anything can uncover it.
    Timer {
        id: settle

        interval: 40
        onTriggered: {
            root.swapped = true;
            root.maybeFinish();
        }
    }

    // Two ways to get stuck, one way out: the images never load (the daemon
    // then changes the wallpaper on its own), or the wallpaper never switches
    // (the layer steps aside instead of staying on top for good).
    Timer {
        id: watchdog

        interval: 2400
        onTriggered: root.finish()
    }

    Variants {
        model: root.active ? root.targets : []

        PanelWindow {
            id: win

            required property var modelData

            screen: modelData
            color: "transparent"
            exclusionMode: ExclusionMode.Ignore
            anchors {
                top: true
                bottom: true
                left: true
                right: true
            }
            mask: Region {} // clicks go through to the desktop
            WlrLayershell.layer: WlrLayer.Bottom
            WlrLayershell.namespace: "fable-wallfx"

            readonly property bool loaded: [oldImg, newImg, fieldImg, lineImg].every(image => image.status === Image.Ready)
            onLoadedChanged: {
                if (loaded)
                    root.screenReady();
            }

            // Decoded at the size of the screen (covering it), not at the size
            // of the file: a 6 MB wallpaper does not need 24 MB of texture.
            Image {
                id: oldImg

                visible: false
                asynchronous: true
                cache: false
                source: root.from ? "file://" + root.from : ""
                sourceSize: Qt.size(win.width, win.height)
                fillMode: Image.PreserveAspectCrop
            }

            Image {
                id: newImg

                visible: false
                asynchronous: true
                cache: false
                source: root.to ? "file://" + root.to : ""
                sourceSize: Qt.size(win.width, win.height)
                fillMode: Image.PreserveAspectCrop
            }

            // The maps hold numbers, not colours: contagion's are read texel
            // by texel (no filtering), the outline's are interpolated on purpose.
            Image {
                id: fieldImg

                visible: false
                asynchronous: true
                cache: false
                smooth: root.kind === "outline"
                source: root.fieldPath ? "file://" + root.fieldPath : ""
            }

            Image {
                id: lineImg

                visible: false
                asynchronous: true
                cache: false
                source: root.linePath ? "file://" + root.linePath : ""
            }

            // The wallpaper daemon's "crop": scale to cover the screen, centred.
            function crop(img) {
                const iw = Math.max(img.implicitWidth, 1), ih = Math.max(img.implicitHeight, 1);
                const s = Math.max(win.width / iw, win.height / ih);
                const w = win.width / (iw * s), h = win.height / (ih * s);
                return Qt.vector4d((1 - w) / 2, (1 - h) / 2, w, h);
            }

            ShaderEffect {
                anchors.fill: parent
                visible: win.loaded
                opacity: root.fade

                property var oldTex: oldImg
                property var newTex: newImg
                property var fieldTex: fieldImg
                property var lineTex: lineImg
                property real t: root.t
                property real mode: root.kind === "outline" ? 1 : 0
                property real sw: win.width
                property real sh: win.height
                property real fw: Math.max(fieldImg.implicitWidth, 1)
                property real fh: Math.max(fieldImg.implicitHeight, 1)
                property real lw: Math.max(lineImg.implicitWidth, 1)
                property real lh: Math.max(lineImg.implicitHeight, 1)
                // the outline's cells are the contagion's, 480 across the screen
                property real cell: win.width / 480
                property real glow: Number(Daemon.setting("wallfx.glow", 0.9))
                property real lineW: 1.5 * Theme.scale
                property real sdRange: 16
                property real seed: root.seed
                property vector4d cropOld: win.crop(oldImg)
                property vector4d cropNew: win.crop(newImg)
                property color ember: root.ember
                property color ink: root.ink

                fragmentShader: Qt.resolvedUrl("transition.frag.qsb")
            }
        }
    }
}
