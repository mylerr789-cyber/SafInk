"""Önce | sonra karşılaştırması + temizlenen en büyük 3 bölgenin yakın çekimi.
Kullanım: python tools/compare.py orijinal.png temiz.png çıktı_öneki
Çıktılar: <önek>_side.jpg, <önek>_zoom.jpg (docs/test-results/ git'e girmez)."""
import sys

import numpy as np
from PIL import Image
from scipy import ndimage

a = Image.open(sys.argv[1]).convert("RGB")
b = Image.open(sys.argv[2]).convert("RGB")
out = sys.argv[3]
assert a.size == b.size, (a.size, b.size)
w, h = a.size
s = min(1.0, 1400 / h, 900 / (2 * w))
side = Image.new("RGB", (int(w * s) * 2 + 8, int(h * s)), (255, 0, 255))
side.paste(a.resize((int(w * s), int(h * s)), Image.LANCZOS), (0, 0))
side.paste(b.resize((int(w * s), int(h * s)), Image.LANCZOS), (int(w * s) + 8, 0))
side.save(out + "_side.jpg", quality=90)

d = (np.abs(np.asarray(a, dtype=np.int16) - np.asarray(b, dtype=np.int16)).sum(2) > 24)
d = ndimage.binary_dilation(d, iterations=25)
lab, n = ndimage.label(d)
objs = ndimage.find_objects(lab)
sizes = sorted(((ndimage.sum(d, lab, i + 1), i) for i in range(n)), reverse=True)[:3]
tiles = []
for _, i in sizes:
    ys, xs = objs[i]
    cy, cx = (ys.start + ys.stop) // 2, (xs.start + xs.stop) // 2
    r = max(ys.stop - ys.start, xs.stop - xs.start, 220) // 2 + 20
    box = (max(0, cx - r), max(0, cy - r), min(w, cx + r), min(h, cy + r))
    pa, pb = a.crop(box), b.crop(box)
    k = 420 / max(pa.size)
    sz = (int(pa.width * k), int(pa.height * k))
    t = Image.new("RGB", (sz[0] * 2 + 6, sz[1]), (255, 0, 255))
    t.paste(pa.resize(sz, Image.LANCZOS), (0, 0))
    t.paste(pb.resize(sz, Image.LANCZOS), (sz[0] + 6, 0))
    tiles.append(t)
if tiles:
    H = sum(t.height for t in tiles) + 6 * (len(tiles) - 1)
    z = Image.new("RGB", (max(t.width for t in tiles), H), (255, 0, 255))
    y = 0
    for t in tiles:
        z.paste(t, (0, y))
        y += t.height + 6
    z.save(out + "_zoom.jpg", quality=90)
print("changed pixels:", int(d.sum()), "regions:", n)
