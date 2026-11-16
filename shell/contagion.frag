// contagion.frag — the wallpaper transition (Transition.qml).
//
// From Diego Malagrida's pocket-dots (https://github.com/diegoMalagrida/pocket-dots,
// home/.config/quickshell/shaders/wallfx.frag, GPL-3.0): its contagion mode.
//
// Nothing here knows WHEN a pixel changes: the maps made by the plugin's
// daemon side (src/lib.rs) say so, and this only reads them. `t` runs from 0
// to 1 in time.
//
// A handful of colonies land at random places and moments and eat through the
// old wallpaper in crisp cells. Each cell that has just flipped burns for an
// instant in the new wallpaper's most vivid colour, a thin glow rides the
// front, and now and then a spark jumps ahead of it. Every extra effect is
// scaled by sin(πt) or a cell's own short life, so it is exactly zero at both
// ends, and the camera ends where the wallpaper daemon draws: the hand-over
// cannot show.
#version 440

layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;

// qt_Matrix + qt_Opacity first (Qt's vertex shader demands it), then floats
// only, then the vec4s: a vec2 among the floats breaks the std140 layout.
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    float t;          // 0..1, linear in time
    float fw;         // mapTex, texels
    float fh;
    float sw;         // smoothTex, texels
    float sh;
    float glow;       // 0..1, strength of the light
    float seed;       // a different one every change: the sparks never repeat
    vec4 cropOld;     // image uv = xy + screen uv * zw  (cover the screen, centred)
    vec4 cropNew;
    vec4 ember;       // the new wallpaper's most vivid colour: the burning edge
};
layout(binding = 1) uniform sampler2D oldTex;
layout(binding = 2) uniform sampler2D newTex;
layout(binding = 3) uniform sampler2D mapTex;
layout(binding = 4) uniform sampler2D smoothTex;

const float PI = 3.14159265;

// The phases reveal AREA, and an area at a steady rate already brakes by
// itself, so the curve only softens the ends.
float scene(float x) { x = clamp(x, 0.0, 1.0); return x * x * (3.0 - 2.0 * x); }
float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7)) + seed * 13.17) * 43758.5453); }

vec3 oldAt(vec2 uv) { return texture(oldTex, cropOld.xy + uv * cropOld.zw).rgb; }
vec3 newAt(vec2 uv, float zoom) { return texture(newTex, cropNew.xy + (0.5 + (uv - 0.5) / zoom) * cropNew.zw).rgb; }

// 16-bit surface in smoothTex's G (high) and B (low), interpolated by hand:
// the sampler's bilinear filter would blend the two bytes separately.
float s16(vec2 texel) {
    texel = clamp(texel, vec2(0.0), vec2(sw, sh) - 1.0);
    vec4 f = texture(smoothTex, (texel + 0.5) / vec2(sw, sh));
    return (floor(f.g * 255.0 + 0.5) * 256.0 + floor(f.b * 255.0 + 0.5)) / 65535.0;
}
float surface(vec2 uv) {
    vec2 p = uv * vec2(sw, sh) - 0.5;
    vec2 i = floor(p), f = p - i;
    return mix(mix(s16(i), s16(i + vec2(1.0, 0.0)), f.x),
               mix(s16(i + vec2(0.0, 1.0)), s16(i + vec2(1.0, 1.0)), f.x), f.y);
}

// A cell's moment: mapTex R (high) and G (low), read texel by texel.
float arrivalAt(vec2 c) {
    c = clamp(c, vec2(0.0), vec2(fw, fh) - 1.0);
    vec4 f = texture(mapTex, (c + 0.5) / vec2(fw, fh));
    return (floor(f.r * 255.0 + 0.5) * 256.0 + floor(f.g * 255.0 + 0.5)) / 65535.0;
}

void main() {
    vec2 uv = qt_TexCoord0;
    const float LIFE = 0.05;                // how long a flipped cell burns, in area
    float energy = sin(PI * t);
    float T = scene(t) * (1.0 + LIFE + 0.01) - 0.01;
    float s = surface(uv);
    vec2 cell = floor(uv * vec2(fw, fh));
    float a = arrivalAt(cell);
    float flick = hash(cell);
    vec3 hot = mix(ember.rgb, vec3(1.0), 0.18);
    vec3 c;
    if (a > T) {
        c = oldAt(uv);
        // a spark: a lone cell just ahead of the front lights up for a moment
        float ahead = a - T;
        if (flick > 0.985 && ahead < 0.02)
            c = mix(c, hot, (1.0 - ahead / 0.02) * 0.8 * glow * step(0.0, T));
    } else {
        c = newAt(uv, 1.0 + 0.025 * (1.0 - scene(t)));   // settles from a touch closer
        bool edge = max(max(arrivalAt(cell + vec2(1.0, 0.0)), arrivalAt(cell - vec2(1.0, 0.0))),
                        max(arrivalAt(cell + vec2(0.0, 1.0)), arrivalAt(cell - vec2(0.0, 1.0)))) > T;
        float life = clamp((T - a) / LIFE, 0.0, 1.0);
        float burn = (1.0 - life) * (1.0 - life) * (edge ? 1.0 : 0.55);
        c = mix(c, hot, burn * (0.55 + 0.45 * flick) * 0.85 * glow);
    }
    // the thin glow riding the front, on both sides
    c += ember.rgb * 0.22 * glow * energy * exp(-abs(s - T) / 0.006);
    fragColor = vec4(clamp(c, 0.0, 1.0), 1.0) * qt_Opacity;
}
