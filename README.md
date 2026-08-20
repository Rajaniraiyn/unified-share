# Unified Share

Unified Share is an adapter-driven nearby sharing core intended to make one Linux desktop interoperate with the sharing surfaces people already have on Android, Windows, iOS, and macOS.

It is deliberately split from desktop UI integrations. The core owns capability detection, protocol adapters, transfers, trust, and a stable JSON contract. A desktop shell only renders that state and sends commands.

## Current milestone

Version 0.2 establishes the contract, includes a native Browser / QR fallback, and keeps LocalSend as an explicit working fallback. It does **not** claim unfinished protocol adapters are usable.

```bash
cargo run -- status
cargo run -- status --json
cargo run -- share --dry-run ~/Downloads/example.txt
cargo run -- share --via browser --json ~/Downloads/example.txt
cargo run -- stop TRANSFER_ID --json
```

The JSON contract starts at `schema_version: 1` so Omarchy and other clients can evolve independently from the transfer implementation.

## Adapter roadmap

| Adapter | Targets | v0.2 state |
|---|---|---|
| Quick Share | Native Android and Google's Windows Quick Share | Backend research complete; integration next |
| Browser / QR | Any modern phone or computer, without installing an app | Ready: native, expiring LAN download links |
| Bluetooth OBEX | Android, Windows, Linux, and some macOS flows | Optional slow fallback |
| AirDrop | iOS and macOS | Hardware-gated experimental adapter |
| LocalSend | Existing LocalSend devices | Ready migration fallback |

The leading Quick Share base is [RQuickShare](https://github.com/Martichou/rquickshare), whose Rust core is also used by [Packet](https://github.com/nozwock/packet). The newer [Linux port of Google's Nearby stack](https://github.com/kidfromjupiter/nearby) is promising but currently documents active Bluetooth and lifecycle bugs, so it belongs behind an experimental adapter until it settles.

Native AirDrop is not equivalent to normal LAN sharing. [OpenDrop](https://github.com/seemoo-lab/opendrop) requires an AWDL implementation such as OWL, and current end-to-end Linux work remains dependent on Wi-Fi driver behavior. Browser / QR therefore remains the safe iPhone fallback on unsupported hardware.

## Browser / QR contract

`share --via browser --json` starts an on-demand user service and returns `url`, `expires_at_unix`, and `transfer_id` fields in the normal schema-versioned result. The URL contains a 256-bit random bearer token. Only the files selected when the command starts are addressable, using fixed numeric routes rather than request-derived filesystem paths. The server binds to the LAN only for the requested lifetime (10 minutes by default; configure it with `--timeout-seconds`). Use `stop TRANSFER_ID` to close it early.

The URL is HTTP because arbitrary recipient browsers cannot trust a locally generated TLS certificate. Treat anyone who possesses the link as an authorized recipient, and use it only on a trusted LAN. The adapter currently serves regular files; folder/archive support and an in-panel QR renderer belong in later milestones.

See [docs/architecture.md](docs/architecture.md) for boundaries and milestones.

## License

GPL-3.0-only. This is compatible with the intended RQuickShare integration.
