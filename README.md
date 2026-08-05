# leddy-rasp-pi

Raspberry Pi edge agent. It connects to the Leddy API over WebSockets, receives
canonical device commands, renders messages through `leddy-lib`, and exposes a
hardware-driver boundary. The bootstrap implementation prints framebuffer
previews to the terminal so protocol and scrolling behavior can be developed
without a physical matrix.

```sh
LEDDY_DEVICE_ID=pi-lab-1 LEDDY_DEVICE_WS_URL=ws://localhost:8080/v1/ws/devices cargo run
```
