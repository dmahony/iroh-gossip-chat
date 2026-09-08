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
on a background worker. No separate installation or online geolocation is needed.
`BORU_GEOIP_CITY` can optionally override it with a newer local MMDB; an invalid
override falls back to the bundled copy.

Location is approximate, may describe an ISP or VPN exit, and can be stale.
The desktop shares approximate coordinates (rounded to 0.1 degrees) and country
on the existing presence heartbeat by default so online peers can light up the
map. Set `BORU_SHARE_MAP_LOCATION=0` to opt out. Full Home location text and raw
IPs are not added to presence or persisted as user data. Public IPs come from Iroh endpoint address
discovery first. Missing IPv4/IPv6 families use bounded HTTPS lookups with
ipify and independent AWS/icanhazip fallbacks. These services see the connection
address, but receive no Boru identity or chat data. Requests use OS routing,
not HTTP proxies; VPN routing still applies. HTTPS results feed local display and coarse map geography,
never installed as QUIC endpoint addresses. Responses are limited to 64 bytes,
validated as public IPs, and redirects are disabled. Location remains offline.
Refresh occurs after address changes and every five minutes on success;
failures back off from 30 seconds to ten minutes and clear stale results.
Without a discovered public address, the card reports location unavailable
and still shows local addresses.

To update, download a new DB-IP Lite monthly edition, verify its license and
gzip integrity, replace the compressed asset, update this source URL/date/hash,
and run the `home_network_info::tests` library tests with the `gui` feature.
Keep the in-app DB-IP attribution and these license notices with distributions.
