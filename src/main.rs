use std::net::{SocketAddr, SocketAddrV4, UdpSocket, ToSocketAddrs};
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::window::WindowLevel;
use serde::{Serialize, Deserialize};
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc::channel;
use std::thread;
use clap::Parser;
use winit::event::MouseButton;
use std::error::Error;
use std::str::FromStr;
use std::process::exit;
use igd::{PortMappingProtocol, search_gateway, SearchOptions};

#[derive(Serialize,Deserialize,Debug,Copy,Clone)]
struct LaserPointerState {
    visible : bool,
    x : f32,
    y : f32,
}

impl LaserPointerState {
    pub fn new() -> LaserPointerState {
        LaserPointerState { visible : false, x : 0.0, y : 0.0, }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let config : Config = Config::new();


    return if config.pointer {
        client(config)
    } else {
        server(config)
    }
}

fn client(config: Config) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new().with_title("Laser Pointer").build(&event_loop).unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut laser_pointer_state = LaserPointerState::new();
    let (tx, rx): (Sender<LaserPointerState>, Receiver<LaserPointerState>) = channel();

    thread::spawn(move || {
        let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind to socket");
        let server_details = config.address;
        let server: Vec<_> = server_details.to_socket_addrs().expect("Unable to resolve domain") .collect();
        loop {
            let interval = std::time::Duration::from_millis(20);
            thread::sleep(interval);
            match rx.try_iter().last() {
                None => {}
                Some(item) => {
                    println!("Sent {} to {}", serde_json::to_string(&item).unwrap(), server[0]);
                    socket.send_to(serde_json::to_string(&item).unwrap().as_bytes(), server[0]).expect("Failed to send packet");
                }
            }
        }
    });

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                println!("The close button was pressed; stopping");
                elwt.exit();
            },
            Event::AboutToWait => {
                window.request_redraw();
            },
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => { },
            Event::WindowEvent {
                event: WindowEvent::MouseInput {
                    state,
                    button,
                    ..
                },
                ..
            } => {
                if button != MouseButton::Left {
                    return;
                }
                laser_pointer_state.visible = state.is_pressed();
                tx.send(laser_pointer_state).unwrap();
            },
            Event::WindowEvent {
                event: WindowEvent::CursorMoved {
                    position,
                    ..
                },
                ..
            } => {
                let window_size = window.inner_size();
                laser_pointer_state.x = (position.x / window_size.width as f64) as f32;
                laser_pointer_state.y = (position.y / window_size.height as f64) as f32;
                tx.send(laser_pointer_state).unwrap();
            },
            _ => ()
        }
    }).unwrap();
    Ok(())
}

fn forward_ports(port : u16) -> Result<(), Box<dyn Error>> {
    let local_addr = local_ip_addr::get_local_ip_address()?;
    println!("Your local ip is {}", local_addr);

    let gateway = search_gateway(SearchOptions::default())?;
    let ip = gateway.get_external_ip()?;
    println!("Your external ip is {}", ip);

    let local_addr = format!("{}:{}", local_addr, port).parse().expect("Failed to get local socket.");
    gateway.add_port(PortMappingProtocol::UDP, port, local_addr, 38000, "Laser pointer!")?;
    println!("Successfully forwarded port {} => {}", port, local_addr);
    println!("Send this to your friend: {}:{}", ip, port);

    Ok(())
}

fn delete_ports(port : u16) -> Result<(), Box<dyn Error>> {
    let gateway = search_gateway(Default::default())?;
    gateway.remove_port(PortMappingProtocol::UDP, port)?;
    println!("Successfully unforwarded port {}", port);
    Ok(())
}

fn server(config: Config) -> Result<(), Box<dyn Error>> {
    let (tx, rx): (Sender<LaserPointerState>, Receiver<LaserPointerState>) = channel();
    let socket = UdpSocket::bind(config.address)?;
    println!("Bound to {}", socket.local_addr().unwrap().to_string());
    let port = socket.local_addr()?.port();
    forward_ports(port).expect("Failed to forward ports!");

    ctrlc::set_handler(move || {
        delete_ports(port).unwrap();
        exit(1);
    }).expect("Failed to set ctrl+c handler.");

    thread::spawn(move || {
        println!("Listening to socket!!");
        let mut buf = [0; 99];
        loop {
            let (amt, _src) = socket.recv_from(&mut buf).expect("Failed to receive packet.");
            let buf = &mut buf[..amt];
            println!("Got {}", String::from_utf8(buf.to_vec()).expect("Failed to convert packet to utf8"));
            tx.send(serde_json::from_slice(buf).expect("Failed to deserialize packet")).expect("Failed to communicate with main thread.");
        }
    });

    let event_loop = EventLoop::new().expect("Failed to build event loop");
    let window = WindowBuilder::new().with_title("Laser Pointer")
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(64,64))
        .with_resizable(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_transparent(true)
        .build(&event_loop).expect("Failed to build window");

    window.set_cursor_hittest(false).expect("Failed to set window to be passthrough.");
    window.set_outer_position(LogicalPosition::new(-1000,-1000));

    event_loop.set_control_flow(ControlFlow::Poll);

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                println!("The close button was pressed; stopping");
                elwt.exit();
            },
            Event::AboutToWait => {
                let laser_pointer_state = match rx.try_recv() {
                    Ok(state) => state,
                    Err(_) => {
                        window.request_redraw();
                        return;
                    }
                };
                let monitor_size = window.primary_monitor().expect("Failed to detect primary monitor.").size();
                if laser_pointer_state.visible {
                    window.set_outer_position(LogicalPosition::new(laser_pointer_state.x * monitor_size.width as f32, laser_pointer_state.y * monitor_size.height as f32));
                } else {
                    window.set_outer_position(LogicalPosition::new(-1000,-1000));
                }
                window.request_redraw();
            },
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
            },
            _ => ()
        }
    }).expect("Failed to run window loop...");
    delete_ports(port).expect("Failed to unforward ports!");
    Ok(())
}


#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    #[arg(short, long, default_value="0.0.0.0:0")]
    address : String,
    #[arg(short, long)]
    pointer : bool,
}

impl Config {
    pub fn new () -> Config {
        let output = Config::parse();
        output
    }
}
