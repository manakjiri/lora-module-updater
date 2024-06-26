mod gateway;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use gateway::GatewayDriver;
use gateway_host_schema::*;
use ring::digest;
use std::{fs::File, io::Write, path::Path, thread::sleep, time::{Duration, Instant}};

/// LoRa module OTA updater
#[derive(Parser)]
struct Args {
    /// The device path to a serialport
    port: String,

    /// The node address
    destination_address: usize,

    /// Path to the firmware binary
    binary: String,

    /// The baudrate to open the port with
    #[clap(short, long, default_value = "115200")]
    baudrate: u32,

    /// Diagnostic file output path
    #[clap(long, default_value=None)]
    debug_file: Option<String>
}

const INIT_TIMEOUT: Duration = Duration::from_secs(30);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);

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

    let mut debug_path = match args.debug_file {
        Some(path) => Some(File::create(Path::new(path.as_str()))?),
        None => None
    };

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
    let block_size = 64;
    let index_count = {
        if binary.len() % block_size == 0 {
            binary.len() / block_size
        } else {
            binary.len() / block_size + 1
        }
    };

    gateway.write(HostPacket::OtaGetStatus)?;
    match gateway.read_with_timeout(RESPONSE_TIMEOUT)? {
        GatewayPacket::OtaStatus(s) => {
            if s.in_progress {
                eprintln!("Aborting previously started update");
                gateway.write(HostPacket::OtaAbortRequest)?;
                match gateway.read_with_timeout(INIT_TIMEOUT)? {
                    GatewayPacket::OtaAbortAck => {}
                    p => {
                        return Err(anyhow!("failed to abort the OTA update: {:?}", p));
                    }
                }
            }
        }
        p => {
            return Err(anyhow!("failed to initialize the OTA update: {:?}", p));
        }
    }

    eprintln!("Initializing the peer update with {} blocks of size {}, {}B total", index_count, block_size, binary.len());
    gateway.write(HostPacket::OtaInit(OtaInitRequest {
        destination_address: args.destination_address,
        binary_size: binary.len() as u32,
        binary_sha256: binary_checksum,
        block_size: block_size as u16,
        block_count: index_count as u16,
    }))?;
    match gateway.read_with_timeout(INIT_TIMEOUT)? {
        GatewayPacket::OtaInitAck => { /* update started */ }
        p => {
            return Err(anyhow!("failed to initialize the OTA update: {:?}", p));
        }
    }

    let mut indexes_to_transmit: Vec<u16> = Vec::with_capacity(index_count);
    let mut highest_index: u16 = 0;
    let mut last_acked_index: u16 = 0;
    let mut transmitted_count = 0;
    let update_start_time = Instant::now();

    if let Some(f) = debug_path.as_mut() {
        f.write_all("time,txed,acked\n".as_bytes())?;
    }

    loop {
        if indexes_to_transmit.is_empty() && highest_index == index_count as u16 {
            eprintln!("Requesting ota done status");
            gateway.write(HostPacket::OtaDoneRequest)?;
        } else {
            let i = match indexes_to_transmit.pop() {
                Some(i) => i as usize,
                None => {
                    let tmp = highest_index;
                    if last_acked_index + 12 >= highest_index {
                        highest_index += 1;
                    } else {
                        eprint!("not advancing further, last acked {}, highest {}", last_acked_index, highest_index);
                    }
                    tmp as usize
                }
            };
            let begin = i * block_size;
            let end = {
                if (i + 1) * block_size >= binary.len() {
                    binary.len() - 1
                } else {
                    (i + 1) * block_size
                }
            };
            eprintln!("Transmitting block {}", i);
            transmitted_count += 1;
            gateway.write(HostPacket::OtaData(OtaData {
                index: i as u16,
                data: binary[begin..end].iter().cloned().collect(),
            }))?;
        }

        match gateway.read_with_timeout(RESPONSE_TIMEOUT) {
            Ok(packet) => match packet {
                GatewayPacket::OtaStatus(status) => {
                    for na in status.not_acked {
                        if !indexes_to_transmit.contains(&na) {
                            eprintln!(
                                "Scheduling {} to retransmit along with {:?}",
                                na, indexes_to_transmit
                            );
                            indexes_to_transmit.push(na);
                        }
                    }
                    last_acked_index = status.last_acked;
                    sleep(Duration::from_millis(150));
                }
                GatewayPacket::OtaDoneAck => {
                    println!("done");
                    break;
                }
                resp => {
                    eprintln!("Unexpected response from gateway during OTA: {:?}", resp);
                }
            },
            Err(e) => {
                eprintln!("Error during read: {}", e);
            }
        }

        if let Some(f) = debug_path.as_mut() {
            f.write_all(format!("{},{},{}\n", update_start_time.elapsed().as_secs(), transmitted_count, last_acked_index).as_bytes())?;
        }
    }

    if let Some(f) = debug_path.as_mut() {
        f.write_all(format!("{},{},{}\n", update_start_time.elapsed().as_secs(), transmitted_count, index_count).as_bytes())?;
    }

    Ok(())
}
