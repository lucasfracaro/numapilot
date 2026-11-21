// numapilot frame generation pass (MIT), appended after the temporal pass.
//
// Makes the frame halfway between the previous output (PREV_OUT) and this one (MAIN), moving
// each block halfway along the motion the temporal pass estimated (MOTION: where the block was
// in the previous frame, in game pixels). Like the frame generation of Lossless Scaling it only
// sees the finished picture: where the motion is unreliable (a block that matched nothing well,
// or the two halves disagree: something appeared or was uncovered) it shows this frame there
// instead of making something up, which reads as a small judder rather than a smear.

//!DESC frame generation: halfway frame
//!HOOK MAIN
//!BIND MAIN
//!BIND INPUT
//!BIND MOTION
//!BIND PREV_OUT
//!SAVE FG
vec4 hook() {
    vec2 p = floor(cur_pos * MAIN_size) + vec2(0.5);
    float scale = MAIN_size.x / INPUT_size.x;
    // the motion of the block this pixel is in halfway, not now: look it up twice
    vec4 m = MOTION_fetch(ivec2(p / (8.0 * scale)));
    m = MOTION_fetch(ivec2((p - 0.5 * m.xy * scale) / (8.0 * scale)));
    vec4 now = MAIN_fetch(ivec2(p));
    if (m.z > 0.06) {
        return vec4(now.rgb, 1.0);
    }
    vec2 h = 0.5 * m.xy * scale;
    vec4 a = PREV_OUT_tex((p + h) / MAIN_size);
    vec4 b = MAIN_tex((p - h) / MAIN_size);
    if (dot(abs(a.rgb - b.rgb), vec3(1.0)) > 0.5) {
        return vec4(b.rgb, 1.0);
    }
    return vec4(mix(a.rgb, b.rgb, 0.5), 1.0);
}
