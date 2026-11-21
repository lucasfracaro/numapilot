// numapilot temporal pass (MIT), appended after any 2× upscaling model.
//
// The game gives no motion vectors, depth or jitter, so the motion is estimated from the frames:
// block matching on the game's frame (INPUT) against the previous one (PREV_IN). The upscaled
// frame (MAIN) is then blended with the previous output (PREV_OUT) reprojected along that motion,
// clamped to the current frame's 3×3 neighbourhood first (the FSR 2 / TAA trick against
// ghosting). Without jitter this steadies the picture (less shimmering and crawling edges in
// motion) more than it adds detail.
//
// INPUT, PREV_IN and PREV_OUT are provided by numapilot's runner (src/vkbench.rs): INPUT is the
// game's frame of this present, PREV_IN / PREV_OUT the game's frame and this pass's output of the
// previous present (zero at the start).

//!DESC temporal: block matching
//!HOOK MAIN
//!BIND INPUT
//!BIND PREV_IN
//!SAVE MOTION
//!WIDTH INPUT.w 8 /
//!HEIGHT INPUT.h 8 /
const int BLOCK = 8;
const int RANGE = 8;
float luma_now(ivec2 p) {
    return dot(INPUT_fetch(p).rgb, vec3(0.2126, 0.7152, 0.0722));
}
float luma_prev(ivec2 p) {
    return dot(PREV_IN_fetch(p).rgb, vec3(0.2126, 0.7152, 0.0722));
}
vec4 hook() {
    ivec2 base = ivec2(cur_pos * target_size) * BLOCK;
    // 16 samples of the block (every other pixel)
    float now[16];
    for (int i = 0; i < 16; i++) {
        now[i] = luma_now(base + ivec2((i % 4) * 2, (i / 4) * 2));
    }
    float zero = 0.0;
    for (int i = 0; i < 16; i++) {
        zero += abs(now[i] - luma_prev(base + ivec2((i % 4) * 2, (i / 4) * 2)));
    }
    // standing still wins ties (static HUD, flat areas)
    float best = zero * 0.9;
    vec2 d = vec2(0.0);
    for (int y = -RANGE; y <= RANGE; y++) {
        for (int x = -RANGE; x <= RANGE; x++) {
            float sad = 0.0;
            for (int i = 0; i < 16; i++) {
                ivec2 p = base + ivec2((i % 4) * 2 + x, (i / 4) * 2 + y);
                sad += abs(now[i] - luma_prev(p));
            }
            if (sad < best) {
                best = sad;
                d = vec2(float(x), float(y));
            }
        }
    }
    // d: where this block was in the previous frame, in game pixels
    return vec4(d, best / 16.0, zero / 16.0);
}

//!DESC temporal: reproject and blend
//!HOOK MAIN
//!BIND MAIN
//!BIND INPUT
//!BIND MOTION
//!BIND PREV_OUT
//!SAVE MAIN
vec4 hook() {
    ivec2 p = ivec2(cur_pos * MAIN_size);
    vec4 c = MAIN_fetch(p);
    vec4 lo = c;
    vec4 hi = c;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            vec4 n = MAIN_fetch(p + ivec2(x, y));
            lo = min(lo, n);
            hi = max(hi, n);
        }
    }
    // output pixel → game pixel → block
    float scale = MAIN_size.x / INPUT_size.x;
    vec4 m = MOTION_fetch(ivec2(vec2(p) / (8.0 * scale)));
    vec2 q = (vec2(p) + vec2(0.5) + m.xy * scale) / MAIN_size;
    // a block that matches nothing well (new content) or a history outside the picture: current
    float w = 0.2;
    if (m.z > 0.06 || q.x < 0.0 || q.y < 0.0 || q.x > 1.0 || q.y > 1.0) {
        w = 1.0;
    }
    vec4 h = clamp(PREV_OUT_tex(q), lo, hi);
    return vec4(mix(h, c, w).rgb, 1.0);
}
