use anyhow::{Context, Result};
use gateway_host_schema::{self, GatewayPacket, HostPacket};
use postcard;
use serialport::SerialPort;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GatewayError {
    #[error("A timeout was exceeded when receiving data from the Gateway: {0}")]
    ReadTimeout(std::io::Error),
    #[error("Gateway or host sent too much data")]
    Overflow,
    #[error("Serialization or deserialization of data failed: {0}")]
    SerDe(postcard::Error),
    #[error("Invalid response given by the gateway")]
    InvalidResponse,
}

pub struct GatewayDriver {
    port: Box<dyn SerialPort>,
    timeout: Duration,
}

impl GatewayDriver {
    pub fn new(path: &str, baudrate: u32) -> Result<GatewayDriver> {
        Ok(GatewayDriver {
            port: serialport::new(path, baudrate)
                .timeout(Duration::from_millis(100))
                .open()?,
            timeout: Duration::from_millis(100),
        })
    }

    pub fn write(&mut self, packet: HostPacket) -> Result<()> {
        let mut buffer = [0u8; 256];
        let to_encode = postcard::to_slice(&packet, &mut buffer).map_err(GatewayError::SerDe)?;
        let mut encoded = [0u8; 256];

        let max_val = 254;
        let mut i = 0;
        let mut j = 0;
        let mut next_add = false;
        while i < to_encode.len() {
            if j >= encoded.len() {
                return Err(GatewayError::Overflow.into());
            }
            if to_encode[i] == max_val {
                next_add = true;
                i += 1;
                continue;
            }
            encoded[j] = if next_add {
                to_encode[i] + max_val
            } else {
                to_encode[i]
            };
            j += 1;
            i += 1;
            next_add = false;
        }
        encoded[j] = 0xff; // terminator
        j += 1;

        println!("TX: {:X?}", &encoded[..j-1]);
        self.port
            .write_all(&encoded[..j])
            .with_context(|| format!("failed to send {:0X?}", &encoded[..j]))?;
        Ok(())
    }

    pub fn read_with_timeout(&mut self, timeout: Duration) -> Result<GatewayPacket> {
        let start = Instant::now();

        let mut buffer = [0u8; 256];
        let max_val = 254;
        let mut j = 0;
        let mut next_add = false;

        loop {
            let mut recv = [0u8; 1];
            match self.port.read_exact(&mut recv) {
                Err(e) => {
                    if start + timeout < Instant::now() {
                        return Err(GatewayError::ReadTimeout(e).into());
                    }
                }
                Ok(_) => {
                    let to_encode = recv[0];
                    if to_encode == 0xFF {
                        break;
                    }
                    if j >= buffer.len() {
                        return Err(GatewayError::Overflow.into());
                    }
                    if to_encode == max_val {
                        next_add = true;
                        continue;
                    }
                    buffer[j] = if next_add {
                        to_encode + max_val
                    } else {
                        to_encode
                    };
                    j += 1;
                    next_add = false;
                }
            }
        }
        println!("RX: {:0X?}", &buffer[..j]);
        Ok(postcard::from_bytes::<GatewayPacket>(&buffer[..j]).map_err(GatewayError::SerDe)?)
    }

    pub fn read(&mut self) -> Result<GatewayPacket> {
        self.read_with_timeout(self.timeout)
    }

    pub fn ping(&mut self) -> Result<Duration> {
        let start = Instant::now();
        self.write(HostPacket::PingRequest)
            .with_context(|| format!("write failed"))?;

        match self.read().with_context(|| format!("read failed"))? {
            GatewayPacket::PingResponse => Ok(Instant::now() - start),
            _resp => Err(GatewayError::InvalidResponse.into()),
        }
    }
}
