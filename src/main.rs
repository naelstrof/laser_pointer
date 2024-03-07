use clap::Parser;
use std::error::Error;

mod server;
mod shared;
mod client;

fn main() -> Result<(), Box<dyn Error>> {
    let config : Config = Config::new();
    if config.steam_id == 0 {
        server::server(config)
    } else {
        client::client(config)
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    #[arg(short, long, default_value="0")]
    steam_id : u64,
    #[arg(short, long, default_value="")]
    cursor_path: String,
    #[arg(long, default_value="")]
    animation_json_path: String,
}

impl Config {
    pub fn new () -> Config {
        let mut output = Config::parse();
        output.cursor_path = shellexpand::full(&output.cursor_path).unwrap().to_string();
        output.animation_json_path = shellexpand::full(&output.animation_json_path).unwrap().to_string();
        output
    }
}
