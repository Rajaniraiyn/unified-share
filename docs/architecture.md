# Architecture

## Boundary

`unified-share` is the long-lived transfer authority. UI clients never parse logs or reach into protocol libraries. They consume versioned JSON and eventually subscribe to a local event socket.

```text
Omarchy plugin / CLI / future desktop clients
                    │
          versioned local contract
                    │
             unified-share core
                    │
    ┌───────────────┼───────────────┬──────────────┐
 Quick Share    Browser / QR    Bluetooth OBEX   AirDrop
                    │
             LocalSend fallback
```

Each adapter reports `ready`, `experimental`, `planned`, `unavailable`, or `unsupported`. Routing may only select `ready` adapters automatically.

## Milestones

1. Stable status contract and LocalSend migration adapter.
2. Embed or wrap `rqs_lib` for Quick Share discovery, consent, progress, send, and receive.
3. Add a tokenized, expiring LAN browser transfer with QR presentation and strict path handling.
4. Add a local event socket and transfer history; keep the daemon socket-activated or on-demand.
5. Add Bluetooth OBEX only when the platform exposes a reliable backend.
6. Keep AirDrop isolated behind a hardware probe and explicit opt-in.

## Security invariants

- Never auto-accept an untrusted incoming transfer.
- Never expose arbitrary filesystem paths through a browser session.
- Use unguessable, expiring session tokens for browser transfers.
- Bind privileged or hardware-disruptive adapters to explicit user actions.
- Do not silently downgrade from an authenticated route to an unauthenticated route.
- Keep received filenames inside the configured download directory after canonicalization.

