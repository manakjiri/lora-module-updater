mod gateway;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use gateway::GatewayDriver;
use gateway_host_schema::*;
use std::{thread::sleep, time::Duration};
use std::{fs::File, io::Write, path::Path};
use chrono::prelude::*;

/// LoRa module OTA updater
#[derive(Parser)]
struct Args {
    /// The device path to a serialport
    port: String,

    /// The node address
    destination_address: usize,

    /// The baudrate to open the port with
    #[clap(short, long, default_value = "115200")]
    baudrate: u32,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut gateway =
        GatewayDriver::new(&args.port, args.baudrate).context("Failed to open port")?;
    gateway.ping().context("Failed to connect to Gateway")?;

    let mut output_path = File::create(Path::new("moisture_log.csv")).context("Failed to create output file")?;
    output_path.write_all("time,zone1,zone2,zone3,zone4\n".as_bytes())?;
    
    loop {
        gateway.write(HostPacket::SoilSensor(SoilSensorRequest{ destination_address: args.destination_address }))?;
        match gateway.read_with_timeout(Duration::from_secs(1)) {
            Ok(resp) => match resp {
                GatewayPacket::SoilSensorMoisture(s) => {
                    println!("{:?}", s);
                    output_path.write_all(format!("{},{},{},{},{}\n", Local::now().format("%y-%m-%d %H:%M.%S"), s[0], s[1], s[2], s[3]).as_bytes())?;
                }
                p => {
                    eprintln!("Unexpected response: {:?}", p);
                }
            },
            Err(_) => {
                eprintln!("Response timeout");
            }
        }

        sleep(Duration::from_secs(15));
    }
}
