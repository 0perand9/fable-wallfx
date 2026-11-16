"""The cut-out behind wallfx's outline transition (src/cutout.rs runs it).

Reads `image<TAB>mask` lines. For each, the subject of `image` is written to
`mask` as a grey picture (PGM, at most 1280 px on its long side), and the
answer is `ok`, or `unreadable: why` for a file that is not a picture. Anything
else that goes wrong ends the script, with the reason on stderr.

The model (BiRefNet, through rembg) stays loaded between lines; rembg downloads
it the first time (~220 MB, to $U2NET_HOME). It runs on the graphics card if
this onnxruntime can use one, and otherwise on the processor, on as many
threads as $OMP_NUM_THREADS says. (Not through OpenVINO, which rembg would pick:
on a processor it is no faster, and it takes every core.)

From `_cutout` in Diego Malagrida's pocket-dots
(https://github.com/diegoMalagrida/pocket-dots, home/.local/bin/wallfx, GPL-3.0).
"""
import os
import sys

import onnxruntime
from PIL import Image
from rembg import new_session, remove

LONG_SIDE = 1280
PROVIDERS = [p for p in ["CUDAExecutionProvider"] if p in onnxruntime.get_available_providers()] + ["CPUExecutionProvider"]

session = None
for line in sys.stdin:
    image, mask = line.rstrip("\n").split("\t")
    try:
        picture = Image.open(image).convert("RGB")
    except Exception as error:
        print("unreadable:", " ".join(str(error).split()), flush=True)
        continue
    picture.thumbnail((LONG_SIDE, LONG_SIDE))
    session = session or new_session("birefnet-general-lite", providers=PROVIDERS)
    cut = remove(picture, session=session, only_mask=True).convert("L")
    cut.save(mask + ".tmp", "PPM")
    os.replace(mask + ".tmp", mask)
    print("ok", flush=True)
