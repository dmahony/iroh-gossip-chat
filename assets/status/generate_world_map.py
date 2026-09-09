#!/usr/bin/env python3
"""Rebuild the geographically registered Home map (requires Pillow).

Natural Earth is public domain: https://www.naturalearthdata.com/about/terms-of-use/
Source revision and checksum are pinned; no runtime download is performed by Boru.
Usage: python assets/status/generate_world_map.py [cached-source.geojson]
"""
import hashlib

import json
from pathlib import Path
import sys
import urllib.request

from PIL import Image, ImageDraw

SOURCE = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/ca96624a56bd078437bca8184e78163e5039ad19/geojson/ne_110m_admin_0_countries.geojson"
SHA256 = "6866c877d39cba9c357620878839b336d569f8c662d3cfab4cb1dbe2d39c977f"
WIDTH, HEIGHT = 1448, 724


def generate(data):
    if hashlib.sha256(data).hexdigest() != SHA256:
        raise ValueError("Natural Earth source checksum mismatch")
    # Plate carree: the complete image is lon [-180,180], lat [90,-90].
    # Supersample the neutral, transparent artwork for thin borders.
    factor = 3
    image = Image.new("RGBA", (WIDTH * factor, HEIGHT * factor))
    draw = ImageDraw.Draw(image)
    for feature in json.loads(data)["features"]:
        geometry = feature["geometry"]
        polygons = geometry["coordinates"]
        if geometry["type"] == "Polygon":
            polygons = [polygons]
        elif geometry["type"] != "MultiPolygon":
            raise ValueError(geometry["type"])
        for polygon in polygons:
            for index, ring in enumerate(polygon):
                points = [((lon + 180) / 360 * WIDTH * factor,
                           (90 - lat) / 180 * HEIGHT * factor)
                          for lon, lat in ring]
                draw.polygon(points, fill=(145, 145, 145, 115) if index == 0 else (0, 0, 0, 0))
                draw.line(points, fill=(185, 185, 185, 205), width=factor)
    return image.resize((WIDTH, HEIGHT), Image.Resampling.LANCZOS)


if __name__ == "__main__":
    data = Path(sys.argv[1]).read_bytes() if len(sys.argv) > 1 else urllib.request.urlopen(SOURCE, timeout=60).read()
    output = Path(__file__).with_name("world-map.png")
    generate(data).save(output)
    print(f"Generated {output}: {WIDTH}x{HEIGHT}, full-world equirectangular")
