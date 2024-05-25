use anyhow::{anyhow, Context, Result};
use reqwest;
use serde_json;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
struct WeatherData {
    precipitation_probability: f64,
    timestamp: Instant,
}

pub struct Weather {
    latitude: f64,
    longitude: f64,
    weather_token: String,
    data: Option<WeatherData>,
}

impl Weather {
    pub fn new(latitude: f64, longitude: f64, weather_token: String) -> Self {
        Self {
            latitude,
            longitude,
            weather_token,
            data: None,
        }
    }

    fn fetch_forecast(&self) -> Result<WeatherData, anyhow::Error> {
        let url = format!(
            "https://api.openweathermap.org/data/2.5/onecall?lat={}&lon={}&lang=en&units=metric&exclude=minutely,daily&appid={}",
            self.latitude, self.longitude, self.weather_token
        );
        let response = reqwest::blocking::get(&url)?.json::<serde_json::Value>()?;

        let mut pop = 0.0;
        for i in 0..6 {
            let p = response["hourly"][i]["pop"]
                .as_f64()
                .ok_or(anyhow!("pop not found in response"))?;
            if p > pop {
                pop = p;
            }
        }

        Ok(WeatherData {
            precipitation_probability: pop,
            timestamp: Instant::now(),
        })
    }

    pub fn get_precipitation_probability(&mut self) -> Result<f64, anyhow::Error> {
        if let Some(data) = &self.data {
            if data.timestamp.elapsed().as_secs() < 60 * 15 {
                return Ok(data.precipitation_probability);
            }
        }
        let data = self.fetch_forecast()?;
        self.data = Some(data);
        Ok(data.precipitation_probability)
    }
}
