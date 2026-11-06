"""Teste rápido: o PyTorch enxerga as MI50 e faz conta em cada uma?"""
import sys
import time

import torch

print(f"torch {torch.__version__}  HIP {torch.version.hip}")
if torch.version.hip is None:
    print("ERRO: este torch não é build ROCm")
    sys.exit(1)
n = torch.cuda.device_count()
if n == 0 or not torch.cuda.is_available():
    print("ERRO: nenhuma GPU visível (veja /dev/kfd, /dev/dri e os grupos render/video)")
    sys.exit(1)
bad = 0
for i in range(n):
    p = torch.cuda.get_device_properties(i)
    arch = getattr(p, "gcnArchName", "?")
    try:
        a = torch.randn(4096, 4096, device=f"cuda:{i}", dtype=torch.float16)
        torch.cuda.synchronize(i)
        t = time.time()
        for _ in range(10):
            b = a @ a
        torch.cuda.synchronize(i)
        tflops = 10 * 2 * 4096**3 / (time.time() - t) / 1e12
        ok = torch.isfinite(b).all().item()
        print(f"GPU {i}: {p.name} ({arch}) {p.total_memory / 2**30:.0f} GiB  fp16 ~{tflops:.1f} TFLOPS  {'OK' if ok else 'ERRO'}")
        bad += not ok
    except Exception as e:  # noqa: BLE001
        print(f"GPU {i}: {p.name} ({arch}) ERRO: {e}")
        bad += 1
sys.exit(1 if bad else 0)
