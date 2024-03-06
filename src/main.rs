use clap::Parser;
use std::error::Error;

mod server;
mod shared;
mod client;

fn main() -> Result<(), Box<dyn Error>> {
    let config : Config = Config::new();
    let valid_ports : [u16;10] = core::array::from_fn(|i| (51124+i) as u16);
    if config.address == "0.0.0.0:0" {
        server::server(config, &valid_ports)
    } else {
        client::client(config, &valid_ports)
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    #[arg(short, long, default_value="0.0.0.0:0")]
    address : String,
    #[arg(short, long, default_value="")]
    cursor_path: String
}

impl Config {
    pub fn new () -> Config {
        let mut output = Config::parse();
        output.cursor_path = shellexpand::full(&output.cursor_path).unwrap().to_string();
        output
    }
}
