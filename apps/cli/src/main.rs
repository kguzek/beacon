//! `beacon-cli`: summarise a Beacon status page for Waybar or scripts.

mod waybar;

use beacon_core::{Client, DEFAULT_STATUS_PAGE_URL};
use clap::Parser;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    version,
    about = "Current status of a Beacon status page, for Waybar or scripts"
)]
struct Args {
    /// Status page whose locations.json lists the publishers to check.
    #[arg(long, value_name = "URL", default_value = DEFAULT_STATUS_PAGE_URL)]
    status_page_url: String,

    /// Print the full snapshot as JSON instead of Waybar output.
    #[arg(long)]
    json: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = Args::parse();
    let result = match Client::new() {
        Ok(client) => {
            client
                .snapshot(&args.status_page_url, beacon_core::now())
                .await
        }
        Err(error) => Err(error),
    };
    if args.json {
        return match result {
            Ok(snapshot) => {
                println!("{}", serde_json::to_string_pretty(&snapshot).unwrap());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("beacon-cli: {error}");
                ExitCode::FAILURE
            }
        };
    }
    // Waybar hides a module whose command fails, so errors are rendered too.
    let output = match &result {
        Ok(snapshot) => waybar::render(snapshot),
        Err(error) => waybar::render_error(&args.status_page_url, error),
    };
    println!("{}", serde_json::to_string(&output).unwrap());
    ExitCode::SUCCESS
}
