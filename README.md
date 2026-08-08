# leddy-rasp-pi

Rust device agent for Raspberry Pi based Leddy displays. The agent consumes the
canonical `DeviceCommand` WebSocket protocol, renders frames with `leddy-lib`,
and applies the configured physical pixel order before handing frames to the
hardware boundary.

## Runtime behavior

The agent now provides the software foundation required before attaching a
physical panel:

- configurable matrices from 100–300 pixels wide and 5–20 pixels tall;
- brightness, top-left/top-right/bottom-left/bottom-right origin, and
  row-major/serpentine ordering through the shared renderer;
- `show`, `clear`, `configure`, and `ping` handling with acknowledgements;
- periodic device telemetry;
- persistent display configuration with atomic replacement;
- bounded reconnect backoff from 250 ms to 30 seconds;
- a no-GPIO mode suitable for CI and development;
- optional raw frame snapshots in physical LED-chain order.

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
```

When `LEDDY_FRAME_SNAPSHOT` is set, each presented frame is written as one byte
per physical LED in device-chain order. A non-zero logical renderer pixel is
scaled to the configured brightness value. Without the variable, frames are
rendered and measured but no GPIO is touched.

## Physical panel boundary

Direct GPIO/panel output is intentionally not guessed in this repository yet.
The electrical and driver implementation depends materially on the selected
panel technology:

- addressable 5 V pixels such as WS2812-class strips need a timing-specific
  peripheral/driver, 3.3 V → 5 V level shifting, a shared ground, and power
  injection sized for the actual pixel current;
- HUB75 matrix panels use a very different parallel scan interface and power
  architecture;
- discrete single-color lamps require their own row/column or driver-IC design.

For a 300×20 board there can be 6,000 LEDs. Power must therefore be designed
from the chosen LED/panel datasheet before connecting the Pi; the Pi must not
source matrix power from its GPIO header. Use fused external supplies, adequate
wire gauge and injection points, a common signal reference, and a shutdown path
that blanks the display before power removal.

The next hardware PR should implement a concrete driver behind the existing
physical-frame boundary only after the panel family, voltage, data interface,
and maximum current budget are fixed.

## Development

```sh
cargo run
cargo test
```

The repository is a Zed package and consumes `leddy-interfaces` and `leddy-lib`
as its canonical shared contracts/rendering dependencies.
