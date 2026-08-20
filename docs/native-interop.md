# Native interoperability plan

## Quick Share trust modes

Stock Quick Share has three materially different paths:

1. **Everyone / temporary visibility** exposes a normal named endpoint. This is the path implemented today.
2. **Contacts / your devices** relies on public-certificate metadata and Google-issued account trust. Random or locally self-signed values cannot safely impersonate that trust, so Unified Share will not offer a fake bypass.
3. **Quick Share QR pairing** uses an ephemeral P-256 key, a `quickshare.google/qrcode` link, QR-derived discovery tokens, and a signature over the UKEY2 authentication string. This is the account-free path to implement next. It is a native Quick Share QR—not the Browser / QR fallback.

The QR work stays inside the Quick Share adapter and must not be surfaced as ready until the displayed key, discovery match, handshake signature, consent, and final safe-disconnect have passed an Android interoperability test.

## Device identity

`unified-share config --device-name NAME` controls the human-friendly endpoint name. The value is stored privately under the XDG config directory and is limited to 32 visible characters. Discovered recipient names remain peer-provided; address-shaped mDNS fallbacks must be labelled as fallback identities rather than presented as a configured device name.

## AirDrop

AirDrop requires Apple Wireless Direct Link (AWDL), not ordinary LAN transport. OpenDrop supplies the upper protocol and OWL supplies an experimental Linux AWDL link, but OWL requires a Wi-Fi card with usable active monitor mode and frame injection.

This machine currently uses Intel `iwlwifi` (`8086:51f1`). No end-to-end result has been established for that card, so the AirDrop adapter remains hardware-gated and unsupported. Unified Share must not reconfigure the primary Wi-Fi interface, require root, or interrupt internet access merely because the user opened a share chooser. A future implementation needs an explicit hardware probe and opt-in transaction with guaranteed network restoration.

## UX boundary

- Omarchy's Share menu and file-manager context action are the primary entry points.
- The optional shell panel owns identity and local history, not recipient discovery or transfer routing.
- Recipient and route selection use Omarchy's native centered chooser.
- LocalSend remains a visible fallback until native receive, QR pairing, and folder delivery are independently verified.
