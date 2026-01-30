use axum::{Router, body::Bytes, extract::{Query, State}, http::StatusCode, routing::{get, post}};
use serde::Deserialize;

#[derive(Deserialize)]
struct PrintParams {
    #[serde(default)]
    raw: bool,
}

#[derive(Deserialize)]
struct WeatherParams {
    #[serde(default = "default_city")]
    city: String,
}

fn default_city() -> String { "Berlin".to_string() }

#[derive(Deserialize)]
struct GeocodingResponse {
    results: Option<Vec<GeocodingResult>>,
}

#[derive(Deserialize)]
struct GeocodingResult {
    name: String,
    latitude: f64,
    longitude: f64,
}

#[derive(Deserialize)]
struct WeatherResponse {
    daily: DailyWeather,
}

#[derive(Deserialize)]
struct DailyWeather {
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
    apparent_temperature_max: Vec<f64>,
    apparent_temperature_min: Vec<f64>,
    precipitation_probability_max: Vec<u8>,
    weather_code: Vec<u8>,
    sunrise: Vec<String>,
    sunset: Vec<String>,
    uv_index_max: Vec<f64>,
    wind_speed_10m_max: Vec<f64>,
    wind_gusts_10m_max: Vec<f64>,
}
use escpos::{driver, printer::Printer, printer_options::PrinterOptions, utils::Protocol};
use std::{env, time::Duration};

type UsbPrinter = Printer<driver::UsbDriver>;

const CHARS_PER_LINE: usize = 48;

fn write_chunk(printer: &mut Option<UsbPrinter>, chunk: &str) {
    if let Some(printer) = printer {
        if let Err(e) = printer.write(chunk) {
            eprintln!("Failed to write chunk: {:?}", e);
        }
    } else {
        print!("{}", chunk);
    }
}

fn create_printer() -> Option<UsbPrinter> {
    eprintln!("Attempting to open USB printer (vendor=0x04b8, product=0x0e28)...");
    let driver = match driver::UsbDriver::open(0x04b8, 0x0e28, Some(Duration::from_secs(2)), None) {
        Ok(d) => {
            eprintln!("USB driver opened successfully");
            d
        }
        Err(e) => {
            eprintln!("Failed to open USB driver: {:?}", e);
            return None;
        }
    };

    let mut printer = Printer::new(
        driver,
        Protocol::default(),
        Some(PrinterOptions::new(
            Some(escpos::utils::PageCode::PC437),
            None,
            CHARS_PER_LINE as u8,
        )),
    );

    if let Err(e) = printer.init() {
        eprintln!("Failed to initialize printer: {:?}", e);
        return None;
    }
    eprintln!("Printer initialized successfully");

    Some(printer)
}

#[tokio::main]
async fn main() {
    let printer = create_printer();
    let app = Router::new()
        .route("/", post(print))
        .route("/weather", get(weather))
        .with_state(printer);

    let listener = tokio::net::TcpListener::bind(format!(
        "0.0.0.0:{}",
        env::var("PORT").unwrap_or("3000".to_owned())
    ))
    .await
    .expect("failed to bind port");

    axum::serve(listener, app)
        .await
        .expect("failed to start server")
}

async fn print(
    State(mut printer): State<Option<UsbPrinter>>,
    Query(params): Query<PrintParams>,
    body: Bytes,
) -> Result<(), StatusCode> {
    let str = std::str::from_utf8(&body).or(Err(StatusCode::UNPROCESSABLE_ENTITY))?;
    eprintln!("Received print request: {} bytes (raw={})", body.len(), params.raw);
    eprintln!("Content: {:?}", str);

    if let None = printer {
        eprintln!("No printer connected, outputting to stdout");
        println!("{}", "-".repeat(CHARS_PER_LINE))
    }

    if params.raw {
        for line in str.lines() {
            write_chunk(&mut printer, line);
            write_chunk(&mut printer, "\n");
        }
    } else {
        for line in str.lines() {
            let mut chars_written = 0;
            let mut extra_char = 0;

            for chunk in line.split_ascii_whitespace() {
                if chunk.len() > CHARS_PER_LINE {
                    eprintln!("Chunk too long ({} chars): {:?}", chunk.len(), chunk);
                    return Err(StatusCode::UNPROCESSABLE_ENTITY);
                }

                if chars_written + chunk.len() + extra_char > CHARS_PER_LINE {
                    write_chunk(&mut printer, "\n");
                    chars_written = 0;
                    extra_char = 0;
                }

                if extra_char == 1 {
                    write_chunk(&mut printer, " ");
                }

                write_chunk(&mut printer, chunk);

                chars_written += extra_char + chunk.len();
                extra_char = 1;
            }

            write_chunk(&mut printer, "\n");
        }
    }

    if let Some(ref mut printer) = printer {
        eprintln!("Flushing print buffer...");
        if let Err(e) = printer.print_cut() {
            eprintln!("Failed to print: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        eprintln!("Print successful");
    } else {
        println!("{}", "-".repeat(CHARS_PER_LINE))
    }

    Ok(())
}

fn weather_code_to_description(code: u8) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Foggy",
        51 | 53 | 55 => "Drizzle",
        61 | 63 | 65 => "Rain",
        66 | 67 => "Freezing rain",
        71 | 73 | 75 => "Snow",
        77 => "Snow grains",
        80 | 81 | 82 => "Rain showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown",
    }
}

fn format_time(iso: &str) -> &str {
    // ISO format: "2024-01-15T07:30" -> "07:30"
    iso.split('T').nth(1).unwrap_or(iso)
}

async fn weather(
    State(mut printer): State<Option<UsbPrinter>>,
    Query(params): Query<WeatherParams>,
) -> Result<(), StatusCode> {
    eprintln!("Weather request for city={}", params.city);

    let geo_url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1",
        urlencoding::encode(&params.city)
    );

    let geo_response = reqwest::get(&geo_url)
        .await
        .map_err(|e| {
            eprintln!("Failed to geocode city: {:?}", e);
            StatusCode::BAD_GATEWAY
        })?
        .json::<GeocodingResponse>()
        .await
        .map_err(|e| {
            eprintln!("Failed to parse geocoding response: {:?}", e);
            StatusCode::BAD_GATEWAY
        })?;

    let location = geo_response
        .results
        .and_then(|r| r.into_iter().next())
        .ok_or_else(|| {
            eprintln!("City not found: {}", params.city);
            StatusCode::NOT_FOUND
        })?;

    eprintln!("Resolved {} to lat={}, lon={}", location.name, location.latitude, location.longitude);

    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&daily=temperature_2m_max,temperature_2m_min,apparent_temperature_max,apparent_temperature_min,precipitation_probability_max,weather_code,sunrise,sunset,uv_index_max,wind_speed_10m_max,wind_gusts_10m_max&temperature_unit=fahrenheit&wind_speed_unit=mph&timezone=auto&forecast_days=1",
        location.latitude, location.longitude
    );

    let response = reqwest::get(&url)
        .await
        .map_err(|e| {
            eprintln!("Failed to fetch weather: {:?}", e);
            StatusCode::BAD_GATEWAY
        })?
        .json::<WeatherResponse>()
        .await
        .map_err(|e| {
            eprintln!("Failed to parse weather response: {:?}", e);
            StatusCode::BAD_GATEWAY
        })?;

    let daily = &response.daily;
    let forecast = format!(
        "{}\n\n{}\n\nHigh: {:.0}F  Low: {:.0}F\nFeels: {:.0}F / {:.0}F\nPrecip: {}%\nUV Index: {:.0}\nWind: {:.0} mph (gusts {:.0})\n\nSunrise: {}\nSunset: {}\n",
        location.name,
        weather_code_to_description(daily.weather_code[0]),
        daily.temperature_2m_max[0],
        daily.temperature_2m_min[0],
        daily.apparent_temperature_max[0],
        daily.apparent_temperature_min[0],
        daily.precipitation_probability_max[0],
        daily.uv_index_max[0],
        daily.wind_speed_10m_max[0],
        daily.wind_gusts_10m_max[0],
        format_time(&daily.sunrise[0]),
        format_time(&daily.sunset[0])
    );

    if printer.is_none() {
        eprintln!("No printer connected, outputting to stdout");
        println!("{}", "-".repeat(CHARS_PER_LINE));
    }

    for line in forecast.lines() {
        write_chunk(&mut printer, line);
        write_chunk(&mut printer, "\n");
    }

    if let Some(ref mut printer) = printer {
        eprintln!("Flushing print buffer...");
        if let Err(e) = printer.print_cut() {
            eprintln!("Failed to print: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        eprintln!("Print successful");
    } else {
        println!("{}", "-".repeat(CHARS_PER_LINE));
    }

    Ok(())
}
