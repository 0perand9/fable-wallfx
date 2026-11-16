import QtQuick
import qs.core
import qs.popups.settings

SettingsPage {
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
}
