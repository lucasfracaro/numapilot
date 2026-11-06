"""Gera constraints que travam o PyTorch da imagem base (build gfx906/ROCm)."""
import importlib.metadata as m

for pkg in ["torch", "torchvision", "torchaudio", "triton", "pytorch-triton-rocm"]:
    try:
        print(f"{pkg}=={m.version(pkg)}")
    except m.PackageNotFoundError:
        pass
