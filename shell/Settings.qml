import QtQuick
import qs.core
import qs.popups.settings

SettingsPage {
    id: page

    // How far the daemon is with cutting the subjects out of the wallpapers.
    readonly property var cutouts: Daemon.topic("wallfx", {}).cutouts ?? ({ state: "off", left: 0, error: "" })

    Section {
        ActionRow {
            readonly property bool chosen: Daemon.setting("wallpaper.transition", "") === "wallfx"

            label: "Contagion"
            hint: chosen ? "The wallpaper transition now. Change it under Appearance → Transition." : "Make it the wallpaper transition (Appearance → Transition). Videos keep a plain fade."
            button: chosen ? "In use" : "Use it"
            icon: Icons.wallpaper
            keywords: "wallpaper animation transition"
            onTriggered: Daemon.setSetting("wallpaper.transition", "wallfx")
        }

        NumberRow {
            path: "wallfx.duration"
            label: "Length"
            from: 0.6
            to: 5
            step: 0.1
            decimals: 1
            factor: 1000
            unit: "s"
            keywords: "duration speed time"
        }

        NumberRow {
            path: "wallfx.glow"
            label: "Glow"
            hint: "How brightly the front burns, in the new wallpaper's most vivid colour."
            from: 0
            to: 1
            step: 0.1
            decimals: 1
            keywords: "light ember bright"
        }
    }

    Section {
        title: "Outline"

        ToggleRow {
            path: "wallfx.outline"
            label: "Outline the subject"
            hint: {
                const what = "A wallpaper with a clear subject draws its outline first, fills it in and spreads out of it.";
                switch (page.cutouts.state) {
                case "working": return what + " Looking at your wallpapers: " + page.cutouts.left + " to go.";
                case "done": return what + " Every wallpaper has been looked at.";
                case "failed": return what + " Not now: " + page.cutouts.error + ".";
                default: return what;
                }
            }
            keywords: "character silhouette cut-out perfil"
        }

        TextRow {
            path: "wallfx.python"
            label: "Python"
            hint: "The Python that cuts the subjects out. It needs rembg (the plugin's README says how)."
            placeholder: "python3"
            keywords: "rembg birefnet model"
        }
    }
}
