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
| Upscaling, light | FSRCNN / ESPCN | tiny CNNs; quality a little above FSR 1 |
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
```

The tools read and write PNGs on the processor, so their times are an upper bound of the GPU
work. Results are kept per card in `~/.local/state/numapilot/present-bench.json`.
