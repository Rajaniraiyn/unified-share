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

Each adapter reports `ready`, `experimental`, or `unavailable`. Routing may only select `ready` adapters automatically. Hardware not yet proven compatible is `unavailable`, not labelled unsupported from an incomplete probe.

## Milestones

1. Stable status contract and LocalSend migration adapter.
2. Embed pinned `rqs_lib` for bounded Quick Share discovery and outbound transfers. Discovery and send are now wired; shell consent/progress UX, inbound consent, and device interoperability tests remain.
3. Add a tokenized, expiring LAN browser transfer with strict path handling. The core now provides the URL contract; desktop clients own QR presentation.
4. Add private on-device transfer history and expose it through the native Omarchy Share provider. A future live event socket remains socket-activated or on-demand.
5. Add Bluetooth OBEX only when the platform exposes a reliable backend.
6. Keep AirDrop isolated behind a hardware probe and explicit opt-in.

## Security invariants

- Never auto-accept an untrusted incoming transfer.
- Never expose arbitrary filesystem paths through a browser session.
- Use unguessable, expiring session tokens for browser transfers.
- Bind privileged or hardware-disruptive adapters to explicit user actions.
- Do not silently downgrade from an authenticated route to an unauthenticated route.
- Keep received filenames inside the configured download directory after canonicalization.
- Store history without source paths or filenames and cap it to 200 local entries.
