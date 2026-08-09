# leddy-rasp-pi

Rust device agent for Raspberry Pi based Leddy displays. The agent consumes the
canonical `DeviceCommand` WebSocket protocol, renders frames with `leddy-lib`,
and hands them to either the CI-safe snapshot backend or a separately packaged
physical display helper.

## Runtime behavior

The agent provides:

- configurable logical matrices from 100–300 pixels wide and 5–20 pixels tall;
- brightness, top-left/top-right/bottom-left/bottom-right origin, and
  row-major/serpentine metadata;
- `show`, `clear`, `configure`, and `ping` handling with acknowledgements;
- periodic device telemetry;
- persistent display configuration with atomic replacement;
- bounded reconnect backoff from 250 ms to 30 seconds;
- a no-GPIO mode suitable for CI and development;
- optional raw frame snapshots in direct LED-chain order;
- an optional framed stdin protocol for a physical HUB75 helper process.

The default persistent configuration path is
`/var/lib/leddy/display-config.json`. A valid persisted configuration wins over
the startup environment after a power cycle.

## Environment

```text
LEDDY_DEVICE_ID=pi-development
LEDDY_DEVICE_WS_URL=ws://localhost:8080/v1/ws/devices
LEDDY_CONFIG_PATH=/var/lib/leddy/display-config.json
LEDDY_MATRIX_WIDTH=100
LEDDY_MATRIX_HEIGHT=10
LEDDY_BRIGHTNESS=96
LEDDY_SERPENTINE=true
LEDDY_PIXEL_ORIGIN=top_left
LEDDY_FRAME_INTERVAL_MS=50
LEDDY_TELEMETRY_INTERVAL_SECS=5
LEDDY_FRAME_SNAPSHOT=/tmp/leddy-frame.bin   # optional no-GPIO frame evidence
LEDDY_HUB75_HELPER=/usr/libexec/leddy/leddy-hub75-helper   # physical backend
```

If `LEDDY_HUB75_HELPER` is set, it takes precedence over the snapshot backend.
The executable is started directly with a piped stdin; no shell is involved.
The agent sends versioned `LEDDYF01` packets containing the logical row-major
frame plus width, height, brightness, origin, and serpentine metadata. This
keeps HUB75 scan/chaining/multiplexing choices out of the network/runtime agent.
A helper exit or broken stdin becomes a device-session error rather than being
reported as successful frame presentation.

When no helper is configured, `LEDDY_FRAME_SNAPSHOT` preserves the prior CI and
direct-LED evidence behavior: one byte per physical LED in Leddy device-chain
order, with non-zero logical pixels scaled to the configured brightness.

## Selected physical panel boundary

[ADR-0001](docs/hardware/ADR-0001-hub75.md) selects **HUB75 RGB matrix panels**
for the first physical sign. The reference build is five chained 64×32 panels,
forming a 320×32 physical canvas around a maximum 300×20 logical Leddy viewport.

The physical helper target is the `hzeller/rpi-rgb-led-matrix` driver family,
behind an active 3.3 V → 5 V level-shifted adapter board. Panel power comes from
an external fused 5 V supply sized from the exact purchased panel datasheet;
the Raspberry Pi does not source panel power.

The helper remains a separate package/process because HUB75 scan timing is a
hardware-specific concern and the selected upstream driver is GPL-licensed.
The Rust device agent remains MIT-licensed and contains no copied or linked
HUB75 driver code. Distribution of a combined appliance still needs normal
license/compliance review.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

The repository is a Zed package and consumes `leddy-interfaces` and `leddy-lib`
as its canonical shared contracts/rendering dependencies.
