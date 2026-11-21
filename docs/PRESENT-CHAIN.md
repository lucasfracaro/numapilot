# Render on one card, improve on the other

The long path of the presentation block (`Profile.present`, `src/present.rs`): the game renders
on one card, its finished frames go to a second card, which upscales them with a small neural
network, generates the in-between frames and shows the result through its own video output. It
is the "capture card" idea behind DLSS-style frame generation done on Linux, with a second GPU in
place of the capture card. The reference machine is the server: game on MI50 `gfx906-0`
(06:00), chain on MI50 `gfx906-1` (09:00), both with display outputs, same vendor:device
(1002:66a1), so every card choice goes by PCI address.

## Chain

```
game (card A) ──present──▶ gamescope (card B, --prefer-vk-device + DRI_PRIME)
                                 │ image of the real frame, copied over PCIe (~0.7 ms at 1080p)
                                 ▼
                      upscale once per real frame        (FSR 1 → Anime4K → SRVGGNetCompact)
                                 │
                      generate (multiplier − 1) frames   (block matching → RIFE v4 lite)
                                 │
                      compose and present on card B's output
```

- Runtime: ncnn on Vulkan (MIT) on the same Vulkan device as the compositor, not ROCm: no
  copies between APIs and no dependency on the gfx906 ROCm stack. ROCm only to train or adjust
  models outside the game.
- Generation runs on the upscaled frames, so the model cost does not double with 2×.
- Generation turns off under the base FPS of the profile (`Present.min_fps`); the real × shown
  measurement (`present::measure`) already works this way.

## Models: video only

The chain sees only the finished image (no motion vectors, depth or jitter from the engine, which
DLSS/FSR 2+ need), so the models are video models:

| Use | Model | Notes |
|---|---|---|
| Upscaling, light | Anime4K (GLSL shaders) | real time by design; can sharpen edges too much on realistic textures |
| Upscaling, light | FSRCNN / ESPCN | tiny CNNs; quality a little above FSR 1. In use: ArtCNN C4F16/C4F32 (ESPCN-style, MIT, shipped) and FSRCNNX x2 8/16 (LGPL, downloaded on first use) |
| Upscaling | SRVGGNetCompact: `realesr-animevideov3` (x2/x3/x4) | made for video, ~0.6 M parameters; the `present-bench` default |
| Upscaling | SRVGGNetCompact: `realesr-general-x4v3` | realistic content, x4 only (2× = run at x4 on a smaller input, or train a x2) |
| Frame generation | RIFE v4 (lite) | learned optical flow, any time fraction (2×, 3×) |
| Frame generation, cheap | block matching (Longshot) | more artifacts; prototype |
| Out | BasicVSR++ and other recurrent video SR | too heavy for real time on the MI50 |

### Temporal upscaling with made-up inputs

"DLSS5 For All" (ReShade add-ons on Windows) feeds the NVIDIA DLSS with motion vectors and depth
estimated from the image alone (DLSS5-Feeder, LumeniteFX). DLSS itself does not run on AMD, but
the idea fits this chain: the optical flow computed for frame generation (block matching or
RIFE's flow) is the motion vector field for free, and a temporal upscaler that runs on any Vulkan
card can take it: FSR 2 / FSR 3.1 upscaler (FidelityFX SDK, MIT, compute shaders, works on
gfx906). Depth can be left flat or estimated cheaply; FSR 2 uses it mainly for disocclusion.
Expected: better detail than a spatial model at a lower cost than SRVGG, with ghosting where the
estimated vectors are wrong (HUD, transparencies). It goes in M2 as a third upscaling candidate;
code from the Windows add-ons is only a reference until their licenses are checked (they are
ReShade/DirectX add-ons, so little of it would be reused anyway).

## M0 result (server, 2026-11-20)

`present-bench --gpu gfx906-1 --game-gpu gfx906-0`, 720p → 1440p, 2×:

| Stage | Time | Goal |
|---|---|---|
| Copy between the cards (calculated) | 0.62 ms | ~0.7 ms |
| SRVGGNetCompact `realesr-animevideov3` 2× | 323 ms | ≤ 6 ms |
| RIFE v4.6 at 1440p | 598 ms | ≤ 4 ms |
| Total | 924 ms | 16.7 ms |

The card was at 88–99 % during the runs, so it is the models, not the PNG files: both are about
two orders of magnitude too slow for real time on the MI50. The copy is not a problem.
Conclusions:

- Frame generation stays with **lsfg-vk on the output card** (`lsfg-out`, already in `main`:
  the layer loaded only in gamescope with the SDL backend generates on gamescope's card). A
  learned interpolator of our own is out until there is one ~100× lighter than RIFE.
- Our own work goes into **upscaling with tiny models run directly as Vulkan compute** on the
  output card, with no files and no ncnn: Anime4K CNN x2 (S/M/L, a few thousand to ~100 k
  weights, MIT), FSRCNN, and FSR 2 fed with motion estimated at the game's resolution.
- It runs where lsfg-vk already proved the path works: **a Vulkan layer loaded only in
  gamescope**. The layer gives gamescope a swapchain at the game's resolution and presents into
  the real one at the display's resolution, running the upscaler in between (GPU timestamps per
  stage).

## Milestones

Each one ends with a measurement that decides whether the next one is worth doing.

| Step | What | Done when |
|---|---|---|
| M0 ✓ | `numapilot present-bench` (ncnn models, copy) | Done: ncnn SRVGG/RIFE are ~100× over the budget; the copy fits. |
| M0b | `present-bench --anime4k`: Anime4K CNN x2 (S/M/L) through mpv's Vulkan renderer on the output card, ms per frame at 720p → 1440p and 1080p → 4K. | There is one Anime4K size under 6 ms on the MI50. |
| M1 | `numapilot-upscale` Vulkan layer in gamescope (SDL backend, like `lsfg-out`): smaller swapchain for gamescope, compute upscale into the real one, first bilinear/FSR 1 (EASU) as the baseline, with GPU timestamps. | A game shows through the layer at 60 FPS with ≤ 1 ms of overhead; works together with `lsfg-out`. |
| M2 | Anime4K CNN x2 ported from its GLSL hooks to the layer's compute shaders; then FSRCNN. | It fits the 6 ms and beats FSR 1 by eye. |
| M3 | FSR 2 in the layer with motion estimated by block matching at the game's resolution (and flat depth). | Better than M2 in motion, ghosting acceptable. |
| M4 | numapilot: the models become `ready` in `present::UPSCALERS`, one click turns it on, the panel shows real × shown FPS and time per stage. | One click turns everything on. |

## Budget (1080p → 1440p, 60 FPS of base, 2×)

| Stage | Runs per real frame | Goal |
|---|---|---|
| Copy between the cards | 1 | ~0.7 ms (calculated) |
| Upscaling | 1 | ≤ 6 ms |
| Generation (flow + synthesis) | multiplier − 1 | ≤ 4 ms |
| Composition and presentation | multiplier | ≤ 1 ms each |
| Total | | ≤ 16.7 ms (1000 / base FPS) |

## M0 on the server

```
# AUR: realesrgan-ncnn-vulkan, rife-ncnn-vulkan (and ffmpeg)
numapilot present-bench --gpu gfx906-1 --game-gpu gfx906-0                 # 720p → 1440p
numapilot present-bench --gpu gfx906-1 --game-gpu gfx906-0 --size 1920x1080 # 1080p → 4K
numapilot present-bench --gpu gfx906-1 --pictures ~/Pictures/skyrim         # game captures
numapilot present-bench --show
numapilot present-bench --gpu gfx906-1 --compute                            # M0b, 720p → 1440p
numapilot present-bench --gpu gfx906-1 --compute --size 1920x1080           # 1080p → 4K
```

The tools read and write PNGs on the processor, so their times are an upper bound of the GPU
work. Results are kept per card in `~/.local/state/numapilot/present-bench.json`.

## M0b result (server, 2026-11-20)

`present-bench --gpu gfx906-1 --compute`, median of 50 runs, GPU time (card confirmed by fdinfo):

| Model | 720p → 1440p | 1080p → 4K |
|---|---|---|
| bilinear 2× | 0.20 ms | 0.41 ms |
| Anime4K CNN x2 S | 0.47 ms | 0.93 ms |
| Anime4K CNN x2 M | 1.07 ms | 2.31 ms |
| Anime4K CNN x2 L | 1.26 ms | 2.97 ms |

All within the 6 ms goal. Slowest passes: M's 4x1x1x56 convolution, the depth-to-space of S and L.

## M1: the layer (`layer/`)

`VK_LAYER_NUMAPILOT_upscale`, an implicit layer turned on by `ENABLE_NUMAPILOT_UPSCALE=1`, acting
only in gamescope (`NUMAPILOT_UPSCALE_PROCS`, default `gamescope,gamescope-wl`):

- gamescope always composes at its output size (`g_nOutputWidth`, from -W/-H and then the
  window's size; the swapchain's `imageExtent` is that size, whatever the surface reports). The
  first version told gamescope the window was half its size; gamescope ignored it and composed at
  the full size into the half-size image, so only the top-left quarter showed (server test,
  2026-11-20).
- So gamescope runs with `-S integer -F nearest`: the game is drawn as blocks of k×k identical
  pixels (k = floor of the smaller ratio, centered, like `calc_scale_factor_scaler`). The layer
  gives gamescope swapchain images of its own (same size); at present it copies the composed
  image to a buffer, reads one texel per block (the game's frame back at its size,
  `NUMAPILOT_UPSCALE_SRC=WxH` from the profile's game resolution, else half the window), runs
  unpack → model passes → pack as compute (`numapilot::vkbench`), and copies the 2× result
  centered into the real image, black around it when the window is larger than 2×.
- When the window is less than 2× the game, it passes through (gamescope's nearest picture).

Model: `NUMAPILOT_UPSCALE_MODEL` (anime4k-s, anime4k-m, anime4k-l, artcnn-c4f16, artcnn-c4f32,
fsrcnnx, fsrcnnx-16, bilinear, or a path to an mpv user shader). Debug messages with
`NUMAPILOT_UPSCALE_DEBUG=1`. 32-bit swapchain formats (8-bit BGRA/RGBA and 10-bit A2B10G10R10/A2R10G10B10, the one
gamescope's SDL backend picks on the server); anything else, or any
error while building the chain, passes through untouched.

In the profile: Adjust → Presentation → upscaling model; `present::plan` adds `--backend sdl -S
integer -F nearest` to gamescope and the variables to gamescope's environment. Set the
gamescope block to game resolution = half the output (e.g. 1280×720 → 2560×1440).

Tested so far with vkcube on lavapipe under Xvfb (320×180 → 640×360, all models). To test by hand:

```
ENABLE_NUMAPILOT_UPSCALE=1 NUMAPILOT_UPSCALE_MODEL=anime4k-l NUMAPILOT_UPSCALE_SRC=1280x720 \
  NUMAPILOT_UPSCALE_DEBUG=1 gamescope --backend sdl -S integer -F nearest \
  -w 1280 -h 720 -W 2560 -H 1440 -- vkcube
```

Not tested yet: together with `lsfg-out` (both layers wrap gamescope's swapchain; their order is
up to the loader), HDR colour spaces (PQ, scRGB) and 16-bit float swapchains (passed through), and the cost per frame on the MI50
inside a game.


## M2: luma models (ArtCNN, FSRCNNX)

ArtCNN and FSRCNNX double only the luma, like mpv uses them. The runner adds a pass that turns
the RGB picture into luma (BT.709 weights on the encoded values) before the model and one that
adds the change in luma onto a bilinear RGB picture after it. ArtCNN's passes are mpv compute
hooks (`//!COMPUTE`, a shared-memory tile per workgroup, `imageStore`): they are rewritten for
the storage buffers (`texelFetch` reads zero outside the picture, the padding the networks were
trained with). Two naga quirks are worked around in `vkbench`: multi-dimensional arrays come out
with their dimensions reversed (the shared tiles are flattened by hand) and `barrier()` is read as
a barrier on every scope, subgroups included (it becomes a workgroup barrier).

FSRCNNX is LGPL 3 and numapilot is MIT, so it is not shipped: `vkbench::fetch` downloads it from
the FSRCNN-TensorFlow releases into `~/.local/share/numapilot/shaders` the first time `run`,
`present-bench --compute` or a profile asks for it.

Checked on lavapipe: every pass compiles; on a smooth picture ArtCNN C4F16 stays within 0.02 of
bilinear on average, and a real picture looks right with every model. To measure on the server:
`numapilot present-bench --gpu gfx906-1 --compute`.

## M3: temporal pass with estimated motion

`shaders/temporal/temporal.glsl` (numapilot's own, MIT) is an option on top of any model
(Adjust → Presentation → "Temporal", `NUMAPILOT_UPSCALE_TEMPORAL=1` for the layer). It is not the
FidelityFX FSR 2 code but the same idea reduced to what the picture alone gives:

1. **Block matching** on the game's frame (INPUT) against the previous one (PREV_IN): 8×8 blocks,
   16 luma samples each, ±8 game pixels; standing still wins ties (HUD, flat areas). Coarse to
   fine: steps of 2 pixels on a 3×3-blurred luma (81 candidates), then ±1 around the two best on
   the luma itself (18), instead of all 289. The full search cost 3.7 ms at 720p and 8.8 ms at
   1080p on the MI50 (first server run).
2. **Reproject and blend**: the previous output (PREV_OUT) is read where the block was, clamped
   to the min/max of the current 3×3 neighbourhood (the FSR 2 / TAA guard against ghosting) and
   mixed 80/20 with the current frame. A block that matches nothing well (new content) or a
   history outside the picture takes the current frame only.

The runner keeps INPUT and MAIN of each present as PREV_IN / PREV_OUT for the next one (black at
the start); the layer copies them at the end of each present.

**Without jitter there is little new detail to gather**: the game renders the same pixel centres
every frame, so the pass mostly steadies the picture (less shimmering and crawling edges in
motion, softer noise) rather than adding resolution like the real FSR 2 with a jittered game.
Flat depth is implied (no disocclusion test beyond the block match). Checked on lavapipe: the
block matching finds a known (3, 2) shift, a still picture stays within 0.02 of the model alone,
vkcube runs through the layer with it. Measured on the second MI50 (server, 2026-11-21, `present-bench --compute`, median of 50):

| | 720p → 1440p | 1080p → 4K |
|---|---|---|
| bilinear + temporal | 1.81 ms | 4.47 ms |
| anime4k-m + temporal | 2.72 ms | 6.46 ms |
| block matching alone | 1.24 ms | 3.18 ms |

So at 720p → 1440p it fits with Anime4K M; at 1080p → 4K only with the lighter models. In gamescope
(vkcube, 960x510 → 1920x1020) the picture was whole, with no trails or blocky tears.

## Frame generation in the layer (engine "blockmatch")

The "DLSS 5 through a capture card" idea: the output card makes the in-between frames from the
finished picture alone, like Lossless Scaling, with the motion the temporal pass already
estimates. `shaders/temporal/fg.glsl` (MIT) makes the frame halfway between the previous output
and this one: each output pixel looks up the block motion (twice, so it uses the motion of where
the block is halfway), samples the previous output half a step forward and this one half a step
back, and blends them. Where the motion is unreliable (a block that matched nothing well, or the
two samples disagree: something appeared or was uncovered) it shows this frame there, which reads
as a small judder instead of a smear. Checked in the runner: halfway through a 2-pixel move the
generated frame is the picture moved by 1.

The layer (`NUMAPILOT_UPSCALE_FG=2`, set by the profile with engine "blockmatch", 2×, and one of
the layer's models) asks the real swapchain for one more image. At each present of gamescope it
acquires that extra image (waiting 4 ms at most; without one, that frame goes without a
generated one), writes the generated frame there and the real one into gamescope's image in the
same submit, and presents the generated one first. It needs a FIFO (vsync) swapchain, where each
present takes its own vblank: with another present mode the layer only upscales. Latency: the
real frame is shown one vblank later than without frame generation, as with any interpolation.
Tested on lavapipe with vkcube (Xvfb): every present after the first shows a generated frame,
synchronization validation clean.

