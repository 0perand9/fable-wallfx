import QtQuick
import qs.core
import qs.popups.settings

SettingsPage {
    Section {
        ChoiceRow {
            path: "wallpaper.transition"
            label: "Draw transitions"
            hint: "Between two still wallpapers. Videos keep the plain change."
            options: [{ value: "random", label: "On" }, { value: "none", label: "None" }]
            keywords: "wallpaper animation contagion"
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
