#!/usr/bin/env python3
"""Distil the lensfun database into the table photo-engine bundles.

    python3 tools/lensfun_extract.py path/to/lensfun/data/db

writes crates/photo-engine/data/lensfun.tsv.z: the cameras (for crop factor and mount)
and the rectilinear lenses with their distortion, TCA and vignetting calibrations, one
record per line, zlib-compressed. The lensfun database is CC BY-SA 3.0; the notice
travels with the data (crates/photo-engine/data/LENSFUN-NOTICE.md).

Records, tab-separated:
    C  maker  model  mount  crop
    L  maker  model  mounts(;)  crop  aspect      then that lens's calibrations:
    D  focal  model  k...                   ptlens a b c | poly3 k1 | poly5 k1 k2
    T  focal  model  k...                   linear kr kb | poly3 vr cr br vb cb bb
    V  focal  aperture  k1  k2  k3          pa, at the farthest calibrated distance
"""

import os
import sys
import xml.etree.ElementTree as ET
import zlib


def text(el, tag):
    # The untranslated name: lensfun adds lang="…" variants beside it.
    for child in el.findall(tag):
        if "lang" not in child.attrib:
            return (child.text or "").strip()
    return ""


def num(v):
    return ("%.6g" % float(v)) if v is not None else "0"


def main(db):
    out = []
    for name in sorted(os.listdir(db)):
        if not name.endswith(".xml"):
            continue
        root = ET.parse(os.path.join(db, name)).getroot()
        for cam in root.findall("camera"):
            maker, model = text(cam, "maker"), text(cam, "model")
            crop = cam.findtext("cropfactor")
            if maker and model and crop:
                out.append("\t".join(["C", maker, model, text(cam, "mount"), num(crop)]))
        for lens in root.findall("lens"):
            kind = (lens.findtext("type") or "rectilinear").strip()
            if kind != "rectilinear":
                continue
            maker, model = text(lens, "maker"), text(lens, "model")
            crop = lens.findtext("cropfactor")
            calib = lens.find("calibration")
            if not model or crop is None or calib is None:
                continue
            aspect = (lens.findtext("aspect-ratio") or "3:2").strip()
            if ":" in aspect:
                a, b = aspect.split(":")
                aspect = "%.6g" % (float(a) / float(b))
            mounts = ";".join((m.text or "").strip() for m in lens.findall("mount"))
            rows = []
            for d in calib.findall("distortion"):
                m = d.get("model")
                keys = {"ptlens": "abc", "poly3": ["k1"], "poly5": ["k1", "k2"]}.get(m)
                if keys is None:
                    continue
                rows.append(["D", num(d.get("focal")), m] + [num(d.get(k)) for k in keys])
            for t in calib.findall("tca"):
                m = t.get("model")
                if m == "linear":
                    ks = ["kr", "kb"]
                elif m == "poly3":
                    ks = ["vr", "cr", "br", "vb", "cb", "bb"]
                else:
                    continue
                defaults = {"vr": "1", "vb": "1"}
                rows.append(
                    ["T", num(t.get("focal")), m]
                    + [num(t.get(k, defaults.get(k, "0"))) for k in ks]
                )
            far = {}
            for v in calib.findall("vignetting"):
                if v.get("model") != "pa":
                    continue
                key = (float(v.get("focal")), float(v.get("aperture")))
                dist = float(v.get("distance", "1000"))
                if key not in far or dist > far[key][0]:
                    far[key] = (dist, v)
            for (focal, aperture), (_, v) in sorted(far.items()):
                rows.append(
                    ["V", num(focal), num(aperture)] + [num(v.get(k)) for k in ("k1", "k2", "k3")]
                )
            if not rows:
                continue
            out.append("\t".join(["L", maker, model, mounts, num(crop), aspect]))
            out.extend("\t".join(r) for r in rows)
    data = ("\n".join(out) + "\n").encode("utf-8")
    here = os.path.dirname(os.path.abspath(__file__))
    dest = os.path.join(here, "..", "crates", "photo-engine", "data", "lensfun.tsv.z")
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with open(dest, "wb") as f:
        f.write(zlib.compress(data, 9))
    print("%d records, %d bytes, %d compressed" % (len(out), len(data), os.path.getsize(dest)))


if __name__ == "__main__":
    main(sys.argv[1])
