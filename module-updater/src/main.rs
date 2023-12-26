mod gateway;

use std::{path::Path, time::Duration};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use gateway::GatewayDriver;
use gateway_host_schema::*;
use ring::digest;

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
    let args = Args::parse();
    /* let args = Args {
        port: "/dev/ttyACM0".to_owned(),
        binary: "Cargo.toml".to_owned(),
        baudrate: 115200,
    }; */

    let binary_path = match Path::new(args.binary.as_str()).canonicalize() {
        Ok(path) => path,
        Err(e) => {
            return Err(anyhow!("Failed to resolve the provided binary path: {}", e));
        }
    };
    if !binary_path.is_file() {
        return Err(anyhow!("\"{}\" is not a file", binary_path.display()));
    }

    let mut gateway =
        GatewayDriver::new(&args.port, args.baudrate).context("Failed to open port")?;
    gateway.ping().context("Failed to connect to Gateway")?;

    let binary = std::fs::read(binary_path)?;
    let binary_checksum = {
        let mut c = digest::Context::new(&digest::SHA256);
        let mut ret = [0u8; 32];
        c.update(&binary);
        ret.copy_from_slice(c.finish().as_ref());
        ret
    };
    let block_size = 32;
    let index_count = {
        if binary.len() % block_size == 0 {
            binary.len() / block_size
        } else {
            binary.len() / block_size + 1
        }
    };

    println!("Initializing the peer update");
    gateway.write(HostPacket::OtaInit(OtaInitRequest {
        binary_size: binary.len() as u32,
        binary_sha256: binary_checksum,
        block_size: block_size as u16,
    }))?;
    match gateway.read_with_timeout(Duration::from_secs(5))? {
        GatewayPacket::OtaInitAck => {/* update started */}
        _ => {
            return Err(anyhow!("failed to initialize the OTA update"));
        }
    }

    

    Ok(())
}
