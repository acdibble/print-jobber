use axum::{Router, body::Bytes, extract::State, http::StatusCode, routing::post};
use escpos::{driver, printer::Printer, printer_options::PrinterOptions, utils::Protocol};
use std::{env, time::Duration};

type UsbPrinter = Printer<driver::UsbDriver>;

fn create_printer() -> Option<UsbPrinter> {
    let driver =
        driver::UsbDriver::open(0x04b8, 0x0e28, Some(Duration::from_secs(2)), None).ok()?;
    let mut printer = Printer::new(
        driver,
        Protocol::default(),
        Some(PrinterOptions::new(
            Some(escpos::utils::PageCode::PC437),
            None,
            80,
        )),
    );

    printer.init().ok()?;

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

    if let None = printer {
        println!("{}", "-".repeat(48))
    }

    for line in str.lines() {
        let mut chars_written = 0;
        let mut extra_char = 0;

        let mut write_chunk = |chunk: &str| {
            if let Some(ref mut printer) = printer {
                printer.write(chunk).expect("failed to write chunk");
            } else {
                print!("{}", chunk)
            }
        };

        for chunk in line.split_ascii_whitespace() {
            if chars_written + chunk.len() + extra_char > 48 {
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

    if let None = printer {
        println!("{}", "-".repeat(48))
    }

    Ok(())
}
