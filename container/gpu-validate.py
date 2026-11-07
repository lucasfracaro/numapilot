"""Validação de uma versão nova (roda dentro da imagem candidata, antes de trocar).

Confere em cada MI50: resultado de multiplicação de matrizes e convolução igual ao da CPU,
memória grande preenchida e lida sem erro, desempenho fp16; e cópia entre as placas.
A última linha é `RESULT {json}` para o gfx906-rocm comparar com a versão anterior.
"""

import json
import sys
import time

res = {"ok": True, "gpus": [], "errors": []}


def fail(msg):
    res["ok"] = False
    res["errors"].append(msg)
    print("ERRO:", msg)


try:
    import torch
    import torch.nn.functional as F
except Exception as e:  # noqa: BLE001
    fail(f"não consegui importar o PyTorch: {e}")
    print("RESULT " + json.dumps(res))
    sys.exit(1)

import os

stack = os.environ.get("VALIDATE_STACK", "gfx906")
res["torch"] = torch.__version__
res["hip"] = torch.version.hip
res["stack"] = stack
if stack == "vulkan":
    # GPU sem ROCm/CUDA: o laboratório roda o PyTorch na CPU (a GPU fica para o llama.cpp/Vulkan).
    a = torch.randn(512, 512)
    ok = torch.allclose(a @ torch.eye(512), a)
    res["ok"] = bool(ok)
    print("PyTorch na CPU:", "OK" if ok else "ERRO")
    print("RESULT " + json.dumps(res))
    sys.exit(0 if ok else 1)
if stack in ("gfx906", "rocm") and torch.version.hip is None:
    fail("este PyTorch não é build ROCm")
if stack == "cuda" and torch.version.cuda is None:
    fail("este PyTorch não é build CUDA")
n = torch.cuda.device_count() if torch.cuda.is_available() else 0
if n == 0:
    fail("nenhuma GPU visível")

g = torch.Generator().manual_seed(1234)
a = torch.randn(1024, 1024, generator=g)
b = torch.randn(1024, 1024, generator=g)
ref = (a.double() @ b.double())
x = torch.randn(4, 16, 64, 64, generator=g)
w = torch.randn(32, 16, 3, 3, generator=g)
conv_ref = F.conv2d(x.double(), w.double(), padding=1)

for i in range(n):
    d = f"cuda:{i}"
    p = torch.cuda.get_device_properties(i)
    info = {"index": i, "name": p.name, "arch": getattr(p, "gcnArchName", "?"), "ok": True}
    try:
        # matmul fp32 x referência fp64 na CPU
        c = (a.to(d) @ b.to(d)).double().cpu()
        info["matmul_err"] = ((c - ref).abs().max() / ref.abs().max()).item()
        if not info["matmul_err"] < 1e-3:
            info["ok"] = False
            fail(f"GPU {i}: matmul diferente da CPU (erro {info['matmul_err']:.2e})")
        # convolução (MIOpen)
        cv = F.conv2d(x.to(d), w.to(d), padding=1).double().cpu()
        info["conv_err"] = ((cv - conv_ref).abs().max() / conv_ref.abs().max()).item()
        if not info["conv_err"] < 1e-3:
            info["ok"] = False
            fail(f"GPU {i}: convolução diferente da CPU (erro {info['conv_err']:.2e})")
        # memória: metade da livre, preenchida e conferida
        free, total = torch.cuda.mem_get_info(i)
        nel = int(free * 0.5 / 4)
        big = torch.empty(nel, dtype=torch.float32, device=d)
        big.fill_(float(i + 1))
        good = bool((big == float(i + 1)).all().item())
        info["mem_test_gb"] = round(nel * 4 / 2**30, 1)
        del big
        torch.cuda.empty_cache()
        if not good:
            info["ok"] = False
            fail(f"GPU {i}: erro ao ler a memória")
        # desempenho fp16
        m = torch.randn(4096, 4096, device=d, dtype=torch.float16)
        torch.cuda.synchronize(i)
        t = time.time()
        for _ in range(20):
            r = m @ m
        torch.cuda.synchronize(i)
        info["tflops_fp16"] = round(20 * 2 * 4096**3 / (time.time() - t) / 1e12, 2)
        if not torch.isfinite(r).all().item():
            info["ok"] = False
            fail(f"GPU {i}: resultado fp16 inválido")
    except Exception as e:  # noqa: BLE001
        info["ok"] = False
        fail(f"GPU {i}: {e}")
    res["gpus"].append(info)
    print(f"GPU {i} {info['name']} ({info['arch']}): {'OK' if info['ok'] else 'ERRO'} "
          f"fp16 {info.get('tflops_fp16', '?')} TFLOPS, memória {info.get('mem_test_gb', '?')} GB testada")

if n >= 2:
    try:
        t0 = torch.randn(64 * 2**20, device="cuda:0")
        torch.cuda.synchronize(0)
        t = time.time()
        t1 = t0.to("cuda:1")
        torch.cuda.synchronize(1)
        dt = time.time() - t
        res["copy_gbs"] = round(t0.numel() * 4 / dt / 1e9, 1)
        if not torch.equal(t0.cpu(), t1.cpu()):
            fail("cópia entre as GPUs com dados diferentes")
        print(f"cópia GPU0 → GPU1: {res['copy_gbs']} GB/s")
    except Exception as e:  # noqa: BLE001
        fail(f"cópia entre as GPUs: {e}")

print("RESULT " + json.dumps(res))
sys.exit(0 if res["ok"] else 1)
