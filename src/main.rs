use axum::{Router, body::Bytes, extract::State, http::StatusCode, routing::post};
use escpos::{driver, printer::Printer, printer_options::PrinterOptions, utils::Protocol};
use std::{env, time::Duration};

type UsbPrinter = Printer<driver::UsbDriver>;

const CHARS_PER_LINE: usize = 48;

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
    let app = Router::new().route("/", post(print)).with_state(printer);

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
    body: Bytes,
) -> Result<(), StatusCode> {
    let str = std::str::from_utf8(&body).or(Err(StatusCode::UNPROCESSABLE_ENTITY))?;
    eprintln!("Received print request: {} bytes", body.len());
    eprintln!("Content: {:?}", str);

    if let None = printer {
        eprintln!("No printer connected, outputting to stdout");
        println!("{}", "-".repeat(CHARS_PER_LINE))
    }

    for line in str.lines() {
        let mut chars_written = 0;
        let mut extra_char = 0;

        let mut write_chunk = |chunk: &str| {
            if let Some(ref mut printer) = printer {
                if let Err(e) = printer.write(chunk) {
                    eprintln!("Failed to write chunk: {:?}", e);
                }
            } else {
                print!("{}", chunk)
            }
        };

        for chunk in line.split_ascii_whitespace() {
            if chars_written + chunk.len() + extra_char > CHARS_PER_LINE {
                write_chunk("\n");
                chars_written = 0;
                extra_char = 0;
            }

            if extra_char == 1 {
                write_chunk(" ")
            }

            write_chunk(chunk);

            chars_written += extra_char + chunk.len();
            extra_char = 1;
        }

        write_chunk("\n")
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
