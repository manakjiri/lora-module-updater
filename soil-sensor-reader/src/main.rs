mod gateway;
mod weather;

use anyhow::{Context, Result};
use chrono::prelude::*;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use gateway::GatewayDriver;
use gateway_host_schema::*;
use http::{Uri, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json;
use std::fs::OpenOptions;
use std::{fs::File, io::Write, path::Path};
use std::{thread::sleep, time::Duration};
use tokio_websockets::{ClientBuilder, Message};
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
    zones: [u16; 4],
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

    let total = moisture.iter().fold(0.0, |acc, m| acc + m) / moisture.len() as f64;
    let hours = Local::now().hour();

    WateringResult {
        watering: total < (config.moisture_threshold / 100.0)
            && pop < (config.precipitation_threshold / 100.0)
            && (hours >= config.day_start_hour && hours < config.day_end_hour),
        moisture: total,
        zones: moisture.iter().map(|m| (*m * 100.0) as u16).collect::<Vec<u16>>().try_into().unwrap_or_default(),
    }
}

async fn transmit_readings(readings: &[u16; 4]) -> Result<()> {
    let uri = Uri::from_static("wss://new-horizons.lumias.cz:8765");
    let (mut client, _) = ClientBuilder::from_uri(uri)
        .add_header(HeaderName::from_static("watering-sensor-client"), HeaderValue::from_str("true")?)
        .connect()
        .await?;
    client.send(Message::text(serde_json::json!({
        "zones": readings,
    }).to_string())).await.context("Failed to send ws message")?;
    client.close().await.context("Failed to close ws connection")?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
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
                    //let pop = weather.get_precipitation_probability()?;
                    let pop = 0.0;
                    let watering = figure_out_watering(&config, s, pop);
                    if let Err(e) = transmit_readings(&watering.zones).await {
                        eprintln!("Failed to transmit readings: {:?}", e);
                    }
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

        tokio::time::sleep(Duration::from_secs(15)).await;
    }
}
