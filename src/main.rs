use axum::{Router, body::Bytes, extract::{Query, State}, http::StatusCode, routing::{get, post}};
use serde::Deserialize;

#[derive(Deserialize)]
struct PrintParams {
    #[serde(default)]
    raw: bool,
}

const BERLIN_LAT: f64 = 52.52;
const BERLIN_LON: f64 = 13.405;

#[derive(Deserialize)]
struct WeatherResponse {
    daily: DailyWeather,
    hourly: HourlyWeather,
}

#[derive(Deserialize)]
struct DailyWeather {
    time: Vec<String>,
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

#[derive(Deserialize)]
struct HourlyWeather {
    temperature_2m: Vec<f64>,
}
use escpos::{driver, printer::Printer, printer_options::PrinterOptions, utils::{JustifyMode, Protocol}};
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

fn parse_hour(iso: &str) -> f64 {
    // "2024-01-15T07:30" -> 7.5
    let time = iso.split('T').nth(1).unwrap_or("12:00");
    let parts: Vec<&str> = time.split(':').collect();
    let hour: f64 = parts[0].parse().unwrap_or(12.0);
    let min: f64 = parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0.0);
    hour + min / 60.0
}

fn moon_phase(date: &str) -> (&'static str, &'static str) {
    // Simple moon phase calculation based on known new moon (Jan 6, 2000)
    let parts: Vec<i32> = date.split('-').filter_map(|s| s.parse().ok()).collect();
    if parts.len() < 3 {
        return ("?", "Unknown");
    }
    let (year, month, day) = (parts[0], parts[1], parts[2]);

    // Days since known new moon (Jan 6, 2000)
    let a = (14 - month) / 12;
    let y = year + 4800 - a;
    let m = month + 12 * a - 3;
    let jd = day + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32045;
    let days_since = (jd - 2451550) as f64; // Jan 6, 2000 = JD 2451550
    let phase = ((days_since % 29.53) + 29.53) % 29.53;

    match phase as u8 {
        0..=1 => ("@", "New Moon"),
        2..=6 => (")", "Waxing Crescent"),
        7..=8 => ("D", "First Quarter"),
        9..=13 => ("D", "Waxing Gibbous"),
        14..=16 => ("O", "Full Moon"),
        17..=21 => ("C", "Waning Gibbous"),
        22..=23 => ("C", "Last Quarter"),
        _ => ("(", "Waning Crescent"),
    }
}

fn render_daylight_bar(sunrise: f64, sunset: f64) -> String {
    let width = CHARS_PER_LINE;
    let mut bar = String::new();

    for col in 0..width {
        let hour = (col as f64 / width as f64) * 24.0;
        let sr_col = (sunrise / 24.0 * width as f64) as usize;
        let ss_col = (sunset / 24.0 * width as f64) as usize;

        let ch = if col == sr_col {
            '>'
        } else if col == ss_col {
            '<'
        } else if hour > sunrise && hour < sunset {
            '='
        } else {
            '-'
        };
        bar.push(ch);
    }

    format!(
        "0           6           12          18        24\n\
         {}\n\
         ^night      ^morn       ^noon       ^eve      ^\n",
        bar
    )
}

fn render_hourly_temps(temps: &[f64]) -> String {
    let mut output = String::new();

    // Show temps for key hours (every 3 hours)
    output.push_str("  Hour:  ");
    for h in (0..24).step_by(3) {
        output.push_str(&format!("{:>4}", h));
    }
    output.push('\n');
    output.push_str("  Temp:  ");
    for h in (0..24).step_by(3) {
        if h < temps.len() {
            output.push_str(&format!("{:>3.0}F", temps[h]));
        }
    }
    output.push('\n');

    output
}

async fn weather(
    State(mut printer): State<Option<UsbPrinter>>,
) -> Result<(), StatusCode> {
    eprintln!("Weather request for Berlin");

    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&daily=temperature_2m_max,temperature_2m_min,apparent_temperature_max,apparent_temperature_min,precipitation_probability_max,weather_code,sunrise,sunset,uv_index_max,wind_speed_10m_max,wind_gusts_10m_max&hourly=temperature_2m&temperature_unit=fahrenheit&wind_speed_unit=mph&timezone=auto&forecast_days=1",
        BERLIN_LAT, BERLIN_LON
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
    let hourly = &response.hourly;
    let desc = weather_code_to_description(daily.weather_code[0]);
    let border = "~".repeat(CHARS_PER_LINE);
    let divider = "-".repeat(CHARS_PER_LINE);

    let sunrise_hour = parse_hour(&daily.sunrise[0]);
    let sunset_hour = parse_hour(&daily.sunset[0]);
    let (moon_symbol, moon_name) = moon_phase(&daily.time[0]);

    let daylight_bar = render_daylight_bar(sunrise_hour, sunset_hour);
    let hourly_temps = render_hourly_temps(&hourly.temperature_2m);

    if printer.is_none() {
        eprintln!("No printer connected, outputting to stdout");
    }

    // Helper macros for printing
    macro_rules! writeln_left {
        ($($arg:tt)*) => {
            if let Some(ref mut p) = printer {
                let _ = p.justify(JustifyMode::LEFT);
                let _ = p.writeln(&format!($($arg)*));
            } else {
                println!($($arg)*);
            }
        };
    }

    macro_rules! writeln_center {
        ($($arg:tt)*) => {
            if let Some(ref mut p) = printer {
                let _ = p.justify(JustifyMode::CENTER);
                let _ = p.writeln(&format!($($arg)*));
            } else {
                println!($($arg)*);
            }
        };
    }

    // Header
    writeln_left!("{}", border);
    writeln_center!("* * * BERLIN * * *");
    writeln_center!("{}", &daily.time[0]);
    writeln_left!("{}", border);
    writeln_center!("");
    writeln_center!("~ {} ~", desc);
    writeln_center!("");

    // Temperature
    writeln_left!("{}", divider);
    writeln_left!("High: {:.0}F          Low: {:.0}F", daily.temperature_2m_max[0], daily.temperature_2m_min[0]);
    writeln_left!("Feels: {:.0}F / {:.0}F", daily.apparent_temperature_max[0], daily.apparent_temperature_min[0]);
    writeln_left!("{}", divider);

    // Conditions
    writeln_left!("Precip: {}%       UV Index: {:.0}", daily.precipitation_probability_max[0], daily.uv_index_max[0]);
    writeln_left!("Wind: {:.0} mph (gusts {:.0})", daily.wind_speed_10m_max[0], daily.wind_gusts_10m_max[0]);
    writeln_left!("{}", divider);
    writeln_left!("");

    // Hourly temps
    writeln_center!("HOURLY TEMPERATURES");
    for line in hourly_temps.lines() {
        writeln_left!("{}", line);
    }
    writeln_left!("{}", divider);
    writeln_left!("");

    // Daylight
    writeln_center!("DAYLIGHT");
    writeln_left!(">=day  -=night");
    for line in daylight_bar.lines() {
        writeln_left!("{}", line);
    }
    writeln_left!("Sunrise: {}    Sunset: {}", format_time(&daily.sunrise[0]), format_time(&daily.sunset[0]));
    writeln_left!("{}", divider);
    writeln_left!("");

    // Moon
    writeln_center!("MOON: {} {}", moon_symbol, moon_name);
    writeln_left!("");
    writeln_left!("{}", border);

    if let Some(ref mut printer) = printer {
        eprintln!("Flushing print buffer...");
        if let Err(e) = printer.print_cut() {
            eprintln!("Failed to print: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        eprintln!("Print successful");
    }

    Ok(())
}
