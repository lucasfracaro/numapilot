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

## Milestones

Each one ends with a measurement that decides whether the next one is worth doing.

| Step | What | Done when |
|---|---|---|
| M0 | `numapilot present-bench`: per-frame time of SRVGGNetCompact (`realesrgan-ncnn-vulkan`, 2×) and RIFE v4 (`rife-ncnn-vulkan`) on the output card, copy between the cards calculated from the PCIe links, budget of one real frame. Plus the gamescope split already in `main` (`run --out-gpu`) with `-F fsr`. | There are numbers for the budget table on the MI50. |
| M1 | `numapilot-present` helper: reads gamescope's PipeWire node (dmabuf), runs FSR 1 + block matching in Vulkan compute, shows it full screen on card B. | 60 FPS of base shows at 120 Hz without visible stutter for 10 minutes. |
| M2 | Models in the helper: Anime4K (GLSL → compute) and SRVGGNetCompact (ncnn) for upscaling, RIFE v4 lite for generation; side by side with the same captures. | One model of each kind fits the goal and beats FSR 1 / block matching by eye. |
| M3 | The chain inside gamescope's composition path (patch, no PipeWire, no extra window). | Total delay drops against M1 and it also works in session mode. |
| M4 | numapilot: the models become `ready` in `present::UPSCALERS`/`ENGINES`, one click turns it on, the panel shows real × shown FPS and time per stage. | One click turns everything on. |

A cheaper side path for M2's upscaling: Anime4K ported to a ReShade `.fx` and loaded with
gamescope's `--reshade-effect`, with no helper at all. Worth trying once M0 says the card has
room.

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
