# Unified Share

Unified Share is an adapter-driven nearby sharing core intended to make one Linux desktop interoperate with the sharing surfaces people already have on Android, Windows, iOS, and macOS.

It is deliberately split from desktop UI integrations. The core owns capability detection, protocol adapters, transfers, trust, and a stable JSON contract. A desktop shell only renders that state and sends commands.

## Current milestone

Version 0.1 establishes the contract and keeps LocalSend as an explicit working fallback. It does **not** claim unfinished protocol adapters are usable.

```bash
cargo run -- status
cargo run -- status --json
cargo run -- share --dry-run ~/Downloads/example.txt
```

The JSON contract starts at `schema_version: 1` so Omarchy and other clients can evolve independently from the transfer implementation.

## Adapter roadmap

| Adapter | Targets | v0.1 state |
|---|---|---|
| Quick Share | Native Android and Google's Windows Quick Share | Backend research complete; integration next |
| Browser / QR | Any modern phone or computer, without installing an app | Planned universal fallback |
| Bluetooth OBEX | Android, Windows, Linux, and some macOS flows | Optional slow fallback |
| AirDrop | iOS and macOS | Hardware-gated experimental adapter |
| LocalSend | Existing LocalSend devices | Ready migration fallback |

The leading Quick Share base is [RQuickShare](https://github.com/Martichou/rquickshare), whose Rust core is also used by [Packet](https://github.com/nozwock/packet). The newer [Linux port of Google's Nearby stack](https://github.com/kidfromjupiter/nearby) is promising but currently documents active Bluetooth and lifecycle bugs, so it belongs behind an experimental adapter until it settles.

Native AirDrop is not equivalent to normal LAN sharing. [OpenDrop](https://github.com/seemoo-lab/opendrop) requires an AWDL implementation such as OWL, and current end-to-end Linux work remains dependent on Wi-Fi driver behavior. Browser / QR therefore remains the safe iPhone fallback on unsupported hardware.

See [docs/architecture.md](docs/architecture.md) for boundaries and milestones.

## License

GPL-3.0-only. This is compatible with the intended RQuickShare integration.

