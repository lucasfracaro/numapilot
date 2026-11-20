# ArtCNN (upscaling, luma)

`ArtCNN_C4F16.glsl` and `ArtCNN_C4F32.glsl` from ArtCNN (https://github.com/Artoriuz/ArtCNN,
`GLSL/`), MIT License, Copyright (c) 2024 Joao Chrisostomo, Kacper Michajłow (the notice is at
the top of each file). Small ESPCN-style networks (4 convolutions and a depth-to-space, ~12 k
and ~48 k weights) that double the luma. They are mpv compute shaders; `src/vkbench.rs` runs
them unchanged, turning the RGB picture into luma before and adding the doubled luma back onto a
bilinear RGB picture after.
