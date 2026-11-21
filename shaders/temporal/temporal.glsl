// numapilot temporal pass (MIT), appended after any 2× upscaling model.
//
// The game gives no motion vectors, depth or jitter, so the motion is estimated from the frames:
// block matching on the game's frame (INPUT) against the previous one (PREV_IN), in steps of 2
// pixels then ±1 around the best. The upscaled
// frame (MAIN) is then blended with the previous output (PREV_OUT) reprojected along that motion,
// clamped to the current frame's 3×3 neighbourhood first (the FSR 2 / TAA trick against
// ghosting). Without jitter this steadies the picture (less shimmering and crawling edges in
// motion) more than it adds detail.
//
// INPUT, PREV_IN and PREV_OUT are provided by numapilot's runner (src/vkbench.rs): INPUT is the
// game's frame of this present, PREV_IN / PREV_OUT the game's frame and this pass's output of the
// previous present (zero at the start).

//!DESC temporal: luma
//!HOOK MAIN
//!BIND INPUT
//!BIND PREV_IN
//!SAVE LUMA
//!WIDTH INPUT.w
//!HEIGHT INPUT.h
// x: this frame, y: the previous one (one read per sample in the search); z, w: the same blurred
// 3×3, for the coarse steps of 2 pixels (fine texture would alias between the steps)
vec4 hook() {
    ivec2 p = ivec2(cur_pos * target_size);
    vec3 w = vec3(0.2126, 0.7152, 0.0722);
    vec2 blur = vec2(0.0);
    for (int i = 0; i < 9; i++) {
        ivec2 q = clamp(p + ivec2(i % 3 - 1, i / 3 - 1), ivec2(0), ivec2(target_size) - 1);
        blur += vec2(dot(INPUT_fetch(q).rgb, w), dot(PREV_IN_fetch(q).rgb, w));
    }
    return vec4(dot(INPUT_fetch(p).rgb, w), dot(PREV_IN_fetch(p).rgb, w), blur / 9.0);
}

//!DESC temporal: block matching
//!HOOK MAIN
//!BIND LUMA
//!SAVE MOTION
//!WIDTH LUMA.w 8 /
//!HEIGHT LUMA.h 8 /
// Coarse to fine: ±8 game pixels in steps of 2 on a blurred luma (81 candidates), then ±1 around
// the two best (18 more), instead of all 289.
const int BLOCK = 8;
vec4 hook() {
    ivec2 base = ivec2(cur_pos * target_size) * BLOCK;
    // 16 samples of the block (every other pixel)
    float now[16];
    for (int i = 0; i < 16; i++) {
        now[i] = LUMA_fetch(base + ivec2((i % 4) * 2, (i / 4) * 2)).x;
    }
    float zero = 0.0;
    for (int i = 0; i < 16; i++) {
        zero += abs(now[i] - LUMA_fetch(base + ivec2((i % 4) * 2, (i / 4) * 2)).y);
    }
    // standing still wins ties (static HUD, flat areas)
    float best = zero * 0.9;
    // coarse, on the blurred luma
    float soft[16];
    for (int i = 0; i < 16; i++) {
        soft[i] = LUMA_fetch(base + ivec2((i % 4) * 2, (i / 4) * 2)).z;
    }
    // the two best (a repeating texture can make a wrong step look a little better)
    float cbest = 1e9;
    float csecond = 1e9;
    ivec2 c = ivec2(0);
    ivec2 c2 = ivec2(0);
    for (int y = -8; y <= 8; y += 2) {
        for (int x = -8; x <= 8; x += 2) {
            float sad = 0.0;
            for (int i = 0; i < 16; i++) {
                sad += abs(soft[i] - LUMA_fetch(base + ivec2((i % 4) * 2 + x, (i / 4) * 2 + y)).w);
            }
            if (x == 0 && y == 0) {
                sad *= 0.9;
            }
            if (sad < cbest) {
                csecond = cbest;
                c2 = c;
                cbest = sad;
                c = ivec2(x, y);
            } else if (sad < csecond) {
                csecond = sad;
                c2 = ivec2(x, y);
            }
        }
    }
    // fine, on the luma itself
    ivec2 d = ivec2(0);
    for (int k = 0; k < 18; k++) {
        {
            ivec2 o = (k < 9 ? c : c2) + ivec2(k % 3 - 1, (k / 3) % 3 - 1);
            float sad = 0.0;
            for (int i = 0; i < 16; i++) {
                sad += abs(now[i] - LUMA_fetch(base + ivec2((i % 4) * 2 + o.x, (i / 4) * 2 + o.y)).y);
            }
            if (sad < best) {
                best = sad;
                d = o;
            }
        }
    }
    // d: where this block was in the previous frame, in game pixels
    return vec4(vec2(d), best / 16.0, zero / 16.0);
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
