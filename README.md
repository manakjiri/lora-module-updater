# Module Updater

## Dev

build binary: `cargo objcopy --release --bin b -- -O binary b.bin`

module-updater: `RUST_BACKTRACE=1 cargo run -- /dev/ttyACM0 ../lora-module-fw/external/embassy/examples/boot/application/stm32wl/b.bin`
