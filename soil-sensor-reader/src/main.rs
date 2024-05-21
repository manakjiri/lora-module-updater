mod gateway;
mod weather;

use anyhow::{Context, Result};
use chrono::prelude::*;
use clap::Parser;
use gateway::GatewayDriver;
use gateway_host_schema::*;
use serde::{Deserialize, Serialize};
use serde_json;
use std::fs::OpenOptions;
use std::{fs::File, io::Write, path::Path};
use std::{thread::sleep, time::Duration};
use weather::Weather;

/// LoRa module OTA updater
#[derive(Parser)]
struct Args {
    /// The device path to a serialport
    port: String,

    /// The node address
    destination_address: usize,

    /// OpenWeather version 2.5 token
    weather_token: String,

    /// The baudrate to open the port with
    #[clap(short, long, default_value = "115200")]
    baudrate: u32,
}

#[derive(Serialize, Deserialize)]
struct Config {
    latitude: f64,
    longitude: f64,
    sensor_cal_low: [u16; 4],
    sensor_cal_high: [u16; 4],
    moisture_threshold: f64,
    precipitation_threshold: f64,
    day_start_hour: u32,
    day_end_hour: u32,
}

struct WateringResult {
    watering: bool,
    moisture: f64,
}

fn figure_out_watering(config: &Config, moisture: [u16; 4], pop: f64) -> WateringResult {
    let moisture = moisture
        .iter()
        .zip(
            config
                .sensor_cal_low
                .iter()
                .zip(config.sensor_cal_high.iter()),
        )
        .map(|(m, (low, high))| ((*m).clamp(*low, *high) - *low) as f64 / (*high - *low) as f64)
        .collect::<Vec<f64>>();

    let moisture = moisture.iter().fold(0.0, |acc, m| acc + m) / moisture.len() as f64;
    let hours = Local::now().hour();

    WateringResult {
        watering: moisture < (config.moisture_threshold / 100.0)
            && pop < (config.precipitation_threshold / 100.0)
            && (hours >= config.day_start_hour && hours < config.day_end_hour),
        moisture,
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config: Config = serde_json::from_reader(
        OpenOptions::new()
            .read(true)
            .open(Path::new("config.json"))
            .context("Failed to open config file")?,
    )
    .context("Failed to parse config file")?;
    let mut weather = Weather::new(config.latitude, config.longitude, args.weather_token);

    let mut gateway =
        GatewayDriver::new(&args.port, args.baudrate).context("Failed to open port")?;
    gateway.ping().context("Failed to connect to Gateway")?;

    let output_path = Path::new("sensor_log.csv");
    let mut output_path = match output_path.exists() {
        true => OpenOptions::new()
            .append(true)
            .open(output_path)
            .context("Failed to open output file")?,
        false => {
            let mut f = File::create(output_path).context("Failed to create output file")?;
            f.write_all("time,zone1,zone2,zone3,zone4,moisture,pop,water\n".as_bytes())?;
            f
        }
    };

    loop {
        gateway.write(HostPacket::SoilSensor(SoilSensorRequest {
            destination_address: args.destination_address,
        }))?;
        match gateway.read_with_timeout(Duration::from_secs(1)) {
            Ok(resp) => match resp {
                GatewayPacket::SoilSensorMoisture(s) => {
                    println!("{:?}", s);
                    let pop = weather.get_precipitation_probability()?;
                    let watering = figure_out_watering(&config, s, pop);
                    output_path.write_all(
                        format!(
                            "{},{},{},{},{},{},{},{}\n",
                            Local::now().format("%y-%m-%d %H:%M.%S"),
                            s[0],
                            s[1],
                            s[2],
                            s[3],
                            (watering.moisture * 100.0).round() as u16,
                            (pop * 100.0).round() as u16,
                            watering.watering as u8
                        )
                        .as_bytes(),
                    )?;
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
