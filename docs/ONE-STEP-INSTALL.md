# One-step install

One `.X1C` file and one flasher run, with no reconnect. The bootloader writes
the unpacker and the main firmware. The unpacker then takes the upper region
from the flasher over USB HID.

Tested on a KWS-X1 from a Linux host.

## Build

Use the current stable Rust toolchain:

```sh
rustup target add thumbv7em-none-eabihf
cargo build --release -p firmware --target thumbv7em-none-eabihf --locked
```

## Pack and audit

```sh
ELF=target/thumbv7em-none-eabihf/release
mkdir -p build
cargo run --release -p firmware-pack --locked -- pack --stage usb \
  --firmware "$ELF/firmware" --unpacker "$ELF/unpacker" \
  --out build/firmware.X1C > build/package.json
hash=$(sha256sum build/firmware.X1C | cut -d ' ' -f 1)
cargo run --release -p firmware-pack --locked -- audit --stage usb \
  --image build/firmware.X1C --confirm-sha256 "$hash" \
  --firmware "$ELF/firmware" --unpacker "$ELF/unpacker" > build/audit.json
cargo run --release -p flasher --locked -- \
  --dry-run --image build/firmware.X1C --confirm-sha256 "$hash"
```

Reference: with Rust 1.100.0, the commit that added this file gives
`build/firmware.X1C` SHA-256
`414e8629f66c9cce95cfb8897679d1d214985353f798557a2885d66a813908cb`, with
`unpacker_bytes` 26036 and `main_packed_bytes` 133632 in `build/package.json`.

## Flash

On Linux, allow hidraw access to the bootloader (`1105:000a`) and the
installer (`16c0:05df`) in `/etc/udev/rules.d/70-kws-x1.rules`, then run
`sudo udevadm control --reload`:

```
KERNEL=="hidraw*", SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1105", ATTRS{idProduct}=="000a", TAG+="uaccess"
KERNEL=="hidraw*", SUBSYSTEM=="hidraw", ATTRS{idVendor}=="16c0", ATTRS{idProduct}=="05df", TAG+="uaccess"
```

Hold **Back + OK** while connecting USB, then:

```sh
cargo run --release -p flasher --locked -- \
  --image build/firmware.X1C --confirm-sha256 "$hash"
```

The flasher output ends with:

```text
Bootloader accepted the image; reboot command sent
Waiting for the meter's installer; keep the cable connected
Upper region sector written bytes=8192 total=122616
...
Transfer complete: the meter verified the upper region
```

The flasher stops there and warns that installation is not complete. The
meter then writes the main firmware and restarts into the measurement screen.
Keep it connected until then.

If the meter already holds this build's upper region, the flasher prints
`Transfer skipped: the meter already holds this build's upper region` and
sends nothing.

## Recovery

- **Flasher stopped, or power lost, after the bootloader step.** Reconnect
  without holding any button and run the same flash command. It prints
  `The meter is already waiting in the installer; resuming`.
- **Power lost during the bootloader step.** Hold **Back + OK** while
  connecting USB and run the same flash command.
