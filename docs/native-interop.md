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

This machine currently uses Intel `iwlwifi` (`8086:51f1`). Its PHY advertises monitor mode, but that does not prove active monitor-mode frame injection or Apple-device interoperability. Neither OpenDrop nor OWL is installed, and there is no active `awdl0` link. The AirDrop route is therefore **unavailable**, not proven hardware-unsupported.

`unified-share air-drop-probe --json` now exposes these facts without mutating the network. The route may become experimental only when OpenDrop and OWL are present and an externally prepared `awdl0` interface already has link-local IPv6. This deliberately conservative gate prevents a share chooser from requesting root, converting the primary Wi-Fi interface to monitor mode, changing channels, or interrupting internet access.

Current upstream limitations matter to the product design:

- OWL requires active monitor mode with frame injection; plain `iw` monitor-mode advertisement is insufficient evidence.
- Upstream OWL does not preserve a concurrent AP connection on the same interface and describes itself as experimental.
- Stock OpenDrop has not been verified here against current iOS/macOS releases. Community patches reporting newer iOS interoperability are useful research inputs, but are not a stable backend contract for Unified Share yet.

The next safe implementation step is an explicit setup workflow for a dedicated, known-compatible Wi-Fi adapter. It must record the selected interface, verify injection and AWDL link creation under informed user control, restore network state on every exit path, and then run a real send/receive matrix before the route is offered in the normal chooser.

## UX boundary

- Omarchy's Share menu and file-manager context action are the primary entry points.
- The optional shell panel owns identity and local history, not recipient discovery or transfer routing.
- Recipient and route selection use Omarchy's native centered chooser.
- LocalSend remains a visible fallback until native receive, QR pairing, and folder delivery are independently verified.
