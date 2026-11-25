#!/usr/bin/env python3
"""Small local server for numapilot's CPU AI models (AI server tab).

One process serves one model, chosen with --kind:
  indra-ocr       text from photos and PDFs      POST /ocr         (file body)  -> {"text": ...}
  parakeet-redux  speech to text                  POST /transcribe  (audio body) -> {"text": ...}
  embeddinggemma  text embeddings                 POST /v1/embeddings (OpenAI-style JSON)
  pocket-tts      text to speech                  POST /tts {"text", "voice"} -> audio/wav
GET / shows a page to try it. Binds to 127.0.0.1 unless --host says otherwise.
`--download` only fetches the model (used by the install step) and exits.
"""

import argparse
import io
import json
import os
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ARGS = None
MODEL = None
LOCK = threading.Lock()


def log(msg):
    print(time.strftime("%H:%M:%S"), msg, flush=True)


def torch_threads(n):
    if n <= 0:
        return
    try:
        import torch

        torch.set_num_threads(n)
        torch.set_num_interop_threads(max(1, min(n, 4)))
    except Exception as e:  # torch absent or already initialised
        log(f"torch threads not set: {e}")


# ------------------------------------------------------------------ models


def load(kind, threads):
    if kind == "indra-ocr":
        import indra_ocr

        return indra_ocr.load(threads=threads) if threads > 0 else indra_ocr.load()
    if kind == "parakeet-redux":
        import moondream as md

        ctx = md.photon("moondream/parakeet-redux", device="cpu")
        return ctx.__enter__()
    if kind == "embeddinggemma":
        torch_threads(threads)
        import torch
        from sentence_transformers import SentenceTransformer

        return SentenceTransformer(
            os.environ.get("NUMAPILOT_EMBED_MODEL", "google/embeddinggemma-2"),
            device="cpu",
            model_kwargs={"torch_dtype": torch.float32},
        )
    if kind == "pocket-tts":
        torch_threads(threads)
        from pocket_tts import TTSModel

        lang = os.environ.get("NUMAPILOT_TTS_LANGUAGE", "")
        try:
            return TTSModel.load_model(language=lang) if lang else TTSModel.load_model()
        except TypeError:
            return TTSModel.load_model()
    raise SystemExit(f"unknown kind: {kind}")


def ocr(body, name):
    suffix = os.path.splitext(name or "")[1] or ".png"
    with tempfile.NamedTemporaryFile(suffix=suffix) as f:
        f.write(body)
        f.flush()
        parts = []
        for _, page in MODEL.convert(f.name):
            parts.append(page.markdown)
    return {"text": "\n\n".join(parts)}


def transcribe(body, name):
    suffix = os.path.splitext(name or "")[1] or ".wav"
    with tempfile.NamedTemporaryFile(suffix=suffix) as f:
        f.write(body)
        f.flush()
        r = MODEL.transcribe(audio=f.name)
    return {"text": r.get("text", "") if isinstance(r, dict) else str(r)}


def embed(req):
    texts = req.get("input", "")
    if isinstance(texts, str):
        texts = [texts]
    prompt = req.get("prompt_name") or None
    vecs = MODEL.encode(texts, prompt_name=prompt) if prompt else MODEL.encode(texts)
    dims = req.get("dimensions")
    data = []
    for i, v in enumerate(vecs):
        v = [float(x) for x in v]
        if dims:
            v = v[: int(dims)]
            n = sum(x * x for x in v) ** 0.5 or 1.0
            v = [x / n for x in v]
        data.append({"object": "embedding", "index": i, "embedding": v})
    return {"object": "list", "data": data, "model": "embeddinggemma"}


def tts(req):
    import scipy.io.wavfile

    voice = req.get("voice") or os.environ.get("NUMAPILOT_TTS_VOICE", "rafael")
    state = MODEL.get_state_for_audio_prompt(voice)
    audio = MODEL.generate_audio(state, req.get("text", ""))
    buf = io.BytesIO()
    scipy.io.wavfile.write(buf, MODEL.sample_rate, audio.numpy())
    return buf.getvalue()


# ------------------------------------------------------------------ page

PAGE = """<!doctype html><html><head><meta charset="utf-8"><title>%(title)s</title>
<style>body{font-family:sans-serif;max-width:820px;margin:2em auto;padding:0 1em;background:#16181d;color:#ddd}
textarea,input{width:100%%;background:#22252c;color:#ddd;border:1px solid #444;padding:.5em}
button{margin:.6em 0;padding:.5em 1.2em}pre{white-space:pre-wrap;background:#22252c;padding:1em}</style>
</head><body><h2>%(title)s</h2><p>%(about)s</p>%(form)s<pre id="out"></pre>
<script>
const out=document.getElementById('out');
async function send(path,body,headers){out.textContent='...';const t=Date.now();
 const r=await fetch(path,{method:'POST',body,headers});
 if(!r.ok){out.textContent=await r.text();return null}return [r,(Date.now()-t)/1000]}
async function file(path){const f=document.getElementById('f').files[0];if(!f)return;
 const x=await send(path,f,{'X-Filename':f.name});if(!x)return;const j=await x[0].json();
 out.textContent=j.text+'\\n\\n('+x[1].toFixed(1)+' s)'}
async function emb(){const a=document.getElementById('a').value,b=document.getElementById('b').value;
 const x=await send('/v1/embeddings',JSON.stringify({input:[a,b]}),{'Content-Type':'application/json'});if(!x)return;
 const d=(await x[0].json()).data;const u=d[0].embedding,v=d[1].embedding;let s=0,nu=0,nv=0;
 for(let i=0;i<u.length;i++){s+=u[i]*v[i];nu+=u[i]*u[i];nv+=v[i]*v[i]}
 out.textContent='similarity: '+(s/Math.sqrt(nu*nv)).toFixed(4)+'  ('+u.length+' dims, '+x[1].toFixed(2)+' s)'}
async function speak(){const x=await send('/tts',JSON.stringify({text:document.getElementById('t').value,
 voice:document.getElementById('v').value}),{'Content-Type':'application/json'});if(!x)return;
 const a=document.getElementById('au');a.src=URL.createObjectURL(await x[0].blob());a.play();
 out.textContent='('+x[1].toFixed(1)+' s)'}
</script></body></html>"""

FORMS = {
    "indra-ocr": (
        "Indra OCR",
        "Text from a photo, scan or PDF (API: POST /ocr with the file as body).",
        '<input type="file" id="f"><button onclick="file(\'/ocr\')">Read text</button>',
    ),
    "parakeet-redux": (
        "Parakeet Redux",
        "Speech to text (API: POST /transcribe with the audio file as body).",
        '<input type="file" id="f" accept="audio/*,video/*"><button onclick="file(\'/transcribe\')">Transcribe</button>',
    ),
    "embeddinggemma": (
        "EmbeddingGemma 2",
        "Embeddings (OpenAI-compatible API: POST /v1/embeddings). Compare two texts:",
        '<textarea id="a" rows="3"></textarea><textarea id="b" rows="3"></textarea><button onclick="emb()">Compare</button>',
    ),
    "pocket-tts": (
        "Pocket TTS",
        'Text to speech (API: POST /tts with {"text", "voice"}, returns WAV).',
        '<textarea id="t" rows="4">Olá! Este é o servidor de voz do numapilot.</textarea>'
        '<input id="v" value="rafael"><button onclick="speak()">Speak</button><audio id="au" controls></audio>',
    ),
}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *a):
        log(fmt % a)

    def reply(self, code, body, ctype="application/json"):
        if isinstance(body, (dict, list)):
            body = json.dumps(body).encode()
        elif isinstance(body, str):
            body = body.encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/health":
            return self.reply(200, {"status": "ok", "kind": ARGS.kind})
        title, about, form = FORMS[ARGS.kind]
        self.reply(200, PAGE % {"title": title, "about": about, "form": form}, "text/html; charset=utf-8")

    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(n)
        name = self.headers.get("X-Filename", "")
        try:
            with LOCK:
                if self.path == "/ocr" and ARGS.kind == "indra-ocr":
                    return self.reply(200, ocr(body, name))
                if self.path == "/transcribe" and ARGS.kind == "parakeet-redux":
                    return self.reply(200, transcribe(body, name))
                if self.path in ("/v1/embeddings", "/embed") and ARGS.kind == "embeddinggemma":
                    return self.reply(200, embed(json.loads(body or b"{}")))
                if self.path == "/tts" and ARGS.kind == "pocket-tts":
                    return self.reply(200, tts(json.loads(body or b"{}")), "audio/wav")
            self.reply(404, {"error": "unknown endpoint for " + ARGS.kind})
        except Exception as e:
            log(f"error: {e!r}")
            self.reply(500, {"error": str(e)})


def main():
    global ARGS, MODEL
    p = argparse.ArgumentParser()
    p.add_argument("--kind", required=True, choices=sorted(FORMS))
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=8090)
    p.add_argument("--threads", type=int, default=0)
    p.add_argument("--download", action="store_true")
    ARGS = p.parse_args()
    t = time.time()
    MODEL = load(ARGS.kind, ARGS.threads)
    log(f"{ARGS.kind} loaded in {time.time() - t:.1f} s (threads {ARGS.threads or 'auto'})")
    if ARGS.download:
        return
    srv = ThreadingHTTPServer((ARGS.host, ARGS.port), Handler)
    log(f"listening on http://{ARGS.host}:{ARGS.port}")
    srv.serve_forever()


if __name__ == "__main__":
    sys.exit(main())
