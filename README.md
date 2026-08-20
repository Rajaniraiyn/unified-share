# Unified Share

Unified Share is an adapter-driven nearby sharing core intended to make one Linux desktop interoperate with the sharing surfaces people already have on Android, Windows, iOS, and macOS.

It is deliberately split from desktop UI integrations. The core owns capability detection, protocol adapters, transfers, trust, and a stable JSON contract. A desktop shell only renders that state and sends commands.

## Current milestone

Version 0.4 adds a private local transfer history and configurable Quick Share device identity to the native Browser / QR fallback and on-demand Quick Share engine. LocalSend remains an explicit working fallback. It does **not** claim device combinations that have not passed a live interoperability test are stable.

```bash
cargo run -- status
cargo run -- status --json
cargo run -- share --dry-run ~/Downloads/example.txt
cargo run -- share --via browser --json ~/Downloads/example.txt
cargo run -- stop TRANSFER_ID --json
cargo run -- discover --timeout-seconds 8 --json
cargo run -- share --via quick-share --device DEVICE_ID --device-name "Pixel" --json ~/Downloads/example.txt
cargo run -- history --json
cargo run -- config --device-name "My Omarchy"
cargo run -- air-drop-probe --json
```

The JSON contract starts at `schema_version: 1` so Omarchy and other clients can evolve independently from the transfer implementation.

## Adapter roadmap

| Adapter | Targets | v0.4 state |
|---|---|---|
| Quick Share | Native Android and Google's Windows Quick Share | Experimental native discovery and outbound transfer |
| Browser / QR | Any modern phone or computer, without installing an app | Ready: native, expiring LAN download links |
| Bluetooth OBEX | Android, Windows, Linux, and some macOS flows | Optional slow fallback |
| AirDrop | iOS and macOS | Non-mutating capability probe; transfer remains unavailable by default |
| LocalSend | Existing LocalSend devices | Ready migration fallback |

Quick Share uses the GPL-3.0 `rqs_lib` engine from the [Packet-maintained RQuickShare fork](https://github.com/nozwock/rquickshare), pinned through [our public integration fork](https://github.com/Rajaniraiyn/rquickshare). We retain upstream history and attribution instead of copying Packet's GTK interface. Our fork vendors `protoc` for reproducible builds, accepts Android's short 17-byte mDNS records, and reports outbound connection failures to clients.

`discover` runs BLE-assisted mDNS discovery only for the requested bounded window, then shuts the engine down. A Quick Share send likewise exists only for the consent/transfer lifetime. The recipient must be visible in Quick Share and on the same Wi-Fi LAN. The confirmation code is written to stderr as soon as the protocol exposes it. Live Android and Windows device testing is still required before this route becomes stable or automatic.

The newer [Linux port of Google's Nearby stack](https://github.com/kidfromjupiter/nearby) is promising but currently documents active Bluetooth and lifecycle bugs, so it remains a research alternative rather than a second protocol engine.

Native AirDrop is not equivalent to normal LAN sharing. [OpenDrop](https://github.com/seemoo-lab/opendrop) requires an AWDL implementation such as [OWL](https://github.com/seemoo-lab/owl), active monitor mode, and working frame injection. Upstream OWL also takes exclusive control of its Wi-Fi interface rather than preserving a concurrent AP connection. Browser / QR therefore remains the safe iPhone fallback unless an AWDL link has been deliberately prepared outside Unified Share.

`air-drop-probe` is a read-only capability report intended for desktop integrations and troubleshooting. It reports the Wi-Fi interface and driver, advertised monitor mode, installed backends, active connection risk, and whether an existing `awdl0` link has link-local IPv6. It never creates a monitor interface, changes channels, starts OWL, requests root, or disconnects Wi-Fi. Advertised monitor mode is deliberately not treated as proof of active frame injection.

## Browser / QR contract

`share --via browser --json` starts an on-demand user service and returns `url`, `expires_at_unix`, and `transfer_id` fields in the normal schema-versioned result. The URL contains a 256-bit random bearer token. Only the files selected when the command starts are addressable, using fixed numeric routes rather than request-derived filesystem paths. The server binds to the LAN only for the requested lifetime (10 minutes by default; configure it with `--timeout-seconds`). Use `stop TRANSFER_ID` to close it early.

The URL is HTTP because arbitrary recipient browsers cannot trust a locally generated TLS certificate. Treat anyone who possesses the link as an authorized recipient, and use it only on a trusted LAN. The adapter currently serves regular files; folder/archive support and an in-panel QR renderer belong in later milestones.

See [docs/architecture.md](docs/architecture.md) for boundaries and milestones, and [docs/native-interop.md](docs/native-interop.md) for the Quick Share trust/QR and hardware-gated AirDrop plan.

Transfer history is capped at 200 entries under the user's XDG state directory. It records route, recipient label, item count, outcome, and message—but never source paths or filenames. `config --device-name` stores the human-friendly name advertised by the Quick Share engine.

## License

GPL-3.0-only. This is compatible with the intended RQuickShare integration.
