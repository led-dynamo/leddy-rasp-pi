# Leddy HUB75 helper

This directory is the separately scoped physical-display helper selected by
`docs/hardware/ADR-0001-hub75.md`.

**License:** GPL-2.0-or-later. This helper is intentionally separated from the
MIT-licensed Rust `leddy-rasp-pi` agent because it links to the GPL-licensed
`hzeller/rpi-rgb-led-matrix` implementation.

Upstream driver: https://github.com/hzeller/rpi-rgb-led-matrix

## Protocol

The Rust agent starts this executable when `LEDDY_HUB75_HELPER` is configured
and writes `LEDDYF01` packets to stdin.

Each packet is:

```text
8 bytes  magic = "LEDDYF01"
1 byte   protocol version = 1
1 byte   origin: 0=top-left, 1=top-right, 2=bottom-left, 3=bottom-right
1 byte   serpentine: 0 or 1
1 byte   brightness: 0..255
2 bytes  logical width, little endian
2 bytes  logical height, little endian
4 bytes  payload length, little endian
N bytes  row-major grayscale pixels
```

The helper validates the 100–300 × 5–20 product geometry, centers the logical
viewport inside the configured physical HUB75 canvas, applies the Leddy
origin/serpentine mapping at the physical boundary, scales intensity by the
configured brightness, clears all pixels outside the logical viewport, and
swaps frames on vertical sync.

A clean EOF or SIGINT/SIGTERM blanks the matrix before exit. Malformed input is
a non-zero helper failure, which the Rust agent treats as a device-session
error.

## Reference hardware geometry

Defaults match ADR-0001:

```text
LEDDY_HUB75_ROWS=32
LEDDY_HUB75_COLS=64
LEDDY_HUB75_CHAIN=5
LEDDY_HUB75_PARALLEL=1
physical canvas = 320 × 32
```

These values configure panel geometry only. Panel-specific scan/multiplexing,
RGB sequence, row-address type, hardware mapping, GPIO slowdown, and adapter
settings must be validated against the exact panel revision before physical
acceptance. Do not hide those installation parameters in unrelated application
code.

## Host protocol test

The parser/mapping layer has no Raspberry Pi or upstream-driver dependency:

```sh
make -C hardware/hub75-helper test
```

CI compiles it with C++20 plus `-Wall -Wextra -Werror -pedantic`.

## Raspberry Pi build

Build the upstream library separately on the target Pi, preserving its license
and source obligations. Then point `RGB_MATRIX_DIR` at that checkout/build:

```sh
git clone https://github.com/hzeller/rpi-rgb-led-matrix /opt/rpi-rgb-led-matrix
make -C /opt/rpi-rgb-led-matrix/lib
make -C hardware/hub75-helper \
  RGB_MATRIX_DIR=/opt/rpi-rgb-led-matrix \
  leddy-hub75-helper
```

Install the resulting executable somewhere such as:

```text
/usr/libexec/leddy/leddy-hub75-helper
```

Then configure the Rust agent:

```text
LEDDY_HUB75_HELPER=/usr/libexec/leddy/leddy-hub75-helper
```

The physical build and upstream source revision must be recorded with the
hardware acceptance evidence. Production packaging should pin a reviewed
upstream commit rather than cloning an unpinned branch at install time.

## Electrical hold point

Building this executable is not authorization to connect a matrix. Follow the
ADR and DEN-2961 acceptance gate first:

- exact 64×32 panel model/revision and scan type recorded;
- active 3.3V→5V buffered adapter selected;
- externally regulated 5V panel supply sized from the actual panel datasheet;
- separately fused panel branches with wire/connector ratings checked;
- common ground and short keyed HUB75 ribbons;
- measured idle, representative, and worst-case current;
- boot/clear/shutdown black-frame behavior verified;
- 30-minute 300×20 scrolling soak, reconnect/replay, emergency clear, and
  power-cycle recovery captured as evidence.

The Raspberry Pi must not source panel power from GPIO or its normal logic
supply path.
