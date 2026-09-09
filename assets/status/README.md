# Home network map

`world-map.png` is a full-world equirectangular (plate carrée) raster:

- 1448 × 724 pixels; longitude −180° to +180°, latitude +90° to −90°.
- No border, crop, or geographic padding. Both the image and the lights must
  use the same centred contain-fit rectangle.
- Neutral transparent land with country outlines. Made with Natural Earth
  1:110m Admin 0 Countries (generalized world-map geography, not street detail).
- Public domain: https://www.naturalearthdata.com/about/terms-of-use/
- Source revision and SHA-256 are pinned in `generate_world_map.py`.

Rebuild with Python and Pillow:

```sh
python assets/status/generate_world_map.py
# Or use the checksum-verified cached GeoJSON without network access:
python assets/status/generate_world_map.py /path/to/ne_110m_admin_0_countries.geojson
```

The generator is development-only. Boru embeds the resulting PNG and never
contacts Natural Earth at runtime. Keep PNG decoding separate from the SVG
lights to avoid the Windows embedded-raster SVG parser stack issue.

The production renderer and regression tests are in
`src/bin/boru/status_card.rs`. Tests check geographic land/ocean fixtures against
the actual raster, contain-fit alignment across viewport shapes, and coincident
lights without geographic displacement. Coincident peer halos alpha-composite
at the same coordinates; they are not moved apart.

This fixes artwork registration, not the accuracy of IP geolocation. Shared
coordinates remain coarse and may identify an ISP/VPN exit rather than the
physical device. The legacy `world-map.svg` is only a separate debug fixture;
it is not the production geography.
