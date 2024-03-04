use std::net::{UdpSocket, ToSocketAddrs, SocketAddr};
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::window::{Window, WindowLevel};
use serde::{Serialize, Deserialize};
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc::channel;
use std::thread;
use clap::Parser;
use winit::event::MouseButton;
use std::error::Error;
use std::process::exit;
use igd::{PortMappingProtocol, search_gateway, SearchOptions};
use std::collections::HashMap;
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;

#[derive(Serialize,Deserialize,Debug,Copy,Clone)]
struct LaserPointerState {
    visible : bool,
    x : f32,
    y : f32,
}

struct Packet {
    owner : SocketAddr,
    state : LaserPointerState,
}

impl LaserPointerState {
    pub fn new() -> LaserPointerState {
        LaserPointerState { visible : false, x : 0.0, y : 0.0, }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let config : Config = Config::new();

    let valid_ports : [u16;6] = *&[51124, 51125, 51126, 51127, 51128, 51129];
    return if config.pointer {
        client(config, valid_ports)
    } else {
        server(config, valid_ports)
    }
}

fn get_socket(valid_ports : [u16;6], should_forward_port : bool) -> Result<UdpSocket, Box<dyn Error>> {
    for port in valid_ports.iter() {
        println!("Attempting to bind to port {}", port);
        let socket = match UdpSocket::bind(format!("0.0.0.0:{}", port)) {
            Ok(socket) => socket,
            Err(error) => {
                println!("Failed to bind to port: {}", error);
                continue
            },
        };
        if should_forward_port {
            match forward_ports(*port) {
                Ok(_) => return Ok(socket),
                Err(error) => {
                    println!("Failed to forward port: {}", error);
                    continue
                }
            }
        }
        println!("Successfully bound to port {}!", port);
        return Ok(socket);
    }
    return Err(Box::from("Couldn't bind to the network, all tries failed..."));
}

fn client(config: Config, valid_ports : [u16;6]) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new().with_title("Laser Pointer").build(&event_loop).unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut laser_pointer_state = LaserPointerState::new();
    let (tx, rx): (Sender<LaserPointerState>, Receiver<LaserPointerState>) = channel();

    thread::spawn(move || {
        let socket = get_socket(valid_ports,false).expect("Failed to connect to socket.");
        let server_details = config.address;
        let server: Vec<_> = server_details.to_socket_addrs().expect("Unable to resolve domain") .collect();
        loop {
            let interval = std::time::Duration::from_millis(20);
            thread::sleep(interval);
            match rx.try_iter().last() {
                None => {}
                Some(item) => {
                    //println!("Sent {} to {}", serde_json::to_string(&item).unwrap(), server[0]);
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
    let gateway = search_gateway(SearchOptions::default())?;
    let ip = gateway.get_external_ip()?;
    let local_addr = local_ip_addr::get_local_ip_address()?;
    let local_addr = format!("{}:{}", local_addr, port).parse().expect("Failed to get local socket.");
    gateway.add_port(PortMappingProtocol::UDP, port, local_addr, 38000, "Laser pointer!")?;
    println!("Successfully forwarded port {}:{} => {}", ip, port, local_addr);
    println!("Send this to your friend: {}:{}", ip, port);
    Ok(())
}

fn delete_ports(port : u16) -> Result<(), Box<dyn Error>> {
    let gateway = search_gateway(Default::default())?;
    gateway.remove_port(PortMappingProtocol::UDP, port)?;
    println!("Successfully un-forwarded port {}", port);
    Ok(())
}

fn server(_config: Config, valid_ports : [u16;6]) -> Result<(), Box<dyn Error>> {
    let (tx, rx): (Sender<Packet>, Receiver<Packet>) = channel();
    let socket = get_socket(valid_ports, true)?;
    let port = socket.local_addr()?.port();

    ctrlc::set_handler(move || {
        delete_ports(port).unwrap();
        exit(1);
    }).expect("Failed to set ctrl+c handler.");

    thread::spawn(move || {
        let mut buf = [0; 99];
        loop {
            let (amt, src) = socket.recv_from(&mut buf).expect("Failed to receive packet.");
            let buf = &mut buf[..amt];
            let state = serde_json::from_slice(buf).expect("Failed to deserialize packet");
            tx.send(Packet {
                owner: src,
                state
            }).expect("Failed to communicate with main thread.");
        }
    });

    let mut user_windows = HashMap::new();
    let event_loop = EventLoop::new().expect("Failed to build event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    loop {
        let user_packet = rx.recv().expect("Failed to communicate with server thread.");
        if !user_windows.contains_key(&user_packet.owner) {
            println!("Got a new connection from {}!", user_packet.owner.to_string());
            user_windows.insert(user_packet.owner.clone(), create_server_window(&event_loop));
        }
        let window = &user_windows[&user_packet.owner];
        let monitor_size = window.primary_monitor().expect("Failed to detect primary monitor.").size();
        let state = user_packet.state;
        if state.visible {
            window.set_outer_position(LogicalPosition::new(state.x * monitor_size.width as f32, state.y * monitor_size.height as f32));
        } else {
            window.set_outer_position(LogicalPosition::new(-1000,-1000));
        }
        window.request_redraw();
    }
    delete_ports(port).expect("Failed to unforward ports!");
    Ok(())
}

fn create_server_window(event_loop : &EventLoop<()>) -> Window {
    let window = WindowBuilder::new().with_title("Laser Pointer")
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(24, 24))
        .with_resizable(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_transparent(true)
        .build(&event_loop).expect("Failed to build window");

    window.set_cursor_hittest(false).expect("Failed to set window to be passthrough.");
    window.set_outer_position(LogicalPosition::new(-1000, -1000));
    return window;
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
