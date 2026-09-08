# Bundled offline GeoIP data

IP Geolocation by [DB-IP](https://db-ip.com/).

`dbip-city-lite.mmdb.gz` is the unmodified DB-IP IP to City Lite database,
August 2026 edition, downloaded from:
https://download.db-ip.com/free/dbip-city-lite-2026-08.mmdb.gz

License: Creative Commons Attribution 4.0 International (CC BY 4.0).
https://creativecommons.org/licenses/by/4.0/
Legal code: https://creativecommons.org/licenses/by/4.0/legalcode

SHA-256: `2b53203ec36a975051a8189dc1207d624e3bc302fbee47648928476533be69d1`

The desktop binary embeds this compressed database and decompresses it once
on a background worker. No separate installation or online lookup is needed.
`BORU_GEOIP_CITY` can optionally override it with a newer local MMDB; an invalid
override falls back to the bundled copy.

Location is approximate, may describe an ISP or VPN exit, and can be stale.
Only the local Home card receives these details; they are not added to peer
presence or persisted as user data. Public IPs come from Iroh endpoint address
discovery, not a hosted IP lookup. Without a discovered public address, the
card reports location unavailable and still shows local addresses.

To update, download a new DB-IP Lite monthly edition, verify its license and
gzip integrity, replace the compressed asset, update this source URL/date/hash,
and run the `home_network_info::tests` library tests with the `gui` feature.
Keep the in-app DB-IP attribution and these license notices with distributions.
