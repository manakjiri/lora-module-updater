mod gateway;

use anyhow::{Context, Result};
use clap::Parser;
use gateway::GatewayDriver;
use gateway_host_schema::{GatewayPacket, HostPacket};

/// LoRa module OTA updater
#[derive(Parser)]
struct Args {
    /// The device path to a serialport
    port: String,

    /// Path to the firmware binary
    binary: String,

    /// The baudrate to open the port with
    #[clap(short, long, default_value = "115200")]
    baudrate: u32,
}

fn main() -> Result<()> {
    //let args = Args::parse();
    let args = Args {
        port: "/dev/ttyACM0".to_owned(),
        binary: "Cargo.toml".to_owned(),
        baudrate: 115200,
    };

    //println!("{:?}", serialport::new(&args.port, args.baudrate)
    //            .timeout(Duration::from_millis(100))
    //            .open()?.write_all(b"dasdsa")?);
    //::std::process::exit(0);

    let mut gateway =
        GatewayDriver::new(&args.port, args.baudrate).context("Failed to open port")?;

    gateway
        .write(HostPacket::PingRequest)
        .with_context(|| format!("write failed"))?;

    match gateway.read().with_context(|| format!("read failed"))? {
        GatewayPacket::PingResponse => {
            println!("pong");
        }
        _ => {
            println!("other");
        }
    }

    Ok(())
}
