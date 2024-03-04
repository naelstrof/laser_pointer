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
use igd::{Gateway, PortMappingProtocol, search_gateway, SearchOptions};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::rc::Rc;
use image::{GenericImageView};
use rand::Rng;
use winit::event_loop::EventLoopWindowTarget;

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
    let valid_ports : [u16;10] = core::array::from_fn(|i| (51124+i) as u16);
    if config.address == "0.0.0.0:0" {
        server(config, &valid_ports)
    } else {
        client(config, &valid_ports)
    }
}

fn get_socket(valid_ports : &[u16], should_forward_port : bool) -> Result<UdpSocket, Box<dyn Error>> {
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
            let gateway = search_gateway(SearchOptions::default());
            if gateway.is_err() {
                println!("Couldn't automatically forward the port because the gateway couldn't be found: {:?}", gateway.err());
                println!("You will need to forward the port {} yourself, or double check your NAT setup.", port);
                return Ok(socket);
            }
            match forward_ports(gateway.unwrap(), *port) {
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

fn client(config: Config, valid_ports : &[u16]) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(WindowBuilder::new().with_title("Laser Pointer").with_min_inner_size(LogicalSize::new(200, 80)).with_transparent(true).build(&event_loop).unwrap());
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut laser_pointer_state = LaserPointerState::new();
    let (tx, rx): (Sender<LaserPointerState>, Receiver<LaserPointerState>) = channel();


    let port_clone = valid_ports.to_owned();
    thread::spawn(move || {
        let socket = get_socket(&port_clone,false).expect("Failed to connect to socket.");
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

    let context = softbuffer::Context::new(window.clone()).expect("Failed to create graphics context.");
    let mut surface = softbuffer::Surface::new(&context, window.clone()).expect("Failed to create graphics surface.");
    let (mut width, mut height) = {
        let size = window.inner_size();
        (size.width,size.height)
    };
    surface.resize(NonZeroU32::new(width).unwrap(), NonZeroU32::new(height).unwrap()).unwrap();
    (width,height) = (0,0);

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
                window_id,
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                if window_id != window.id() {
                    return;
                }
                let (new_width, new_height) = {
                    let size = window.inner_size();
                    (size.width,size.height)
                };
                if width == new_width && height == new_height {
                    let buffer = surface.buffer_mut().unwrap();
                    buffer.present().unwrap();
                    return;
                }
                (width, height) = (new_width, new_height);
                if width == 0 || height == 0 {
                    return;
                }
                surface.resize(NonZeroU32::new(width).unwrap(), NonZeroU32::new(height).unwrap()).unwrap();
                let mut buffer = surface.buffer_mut().unwrap();
                let color = hex::decode("882d452e").unwrap();
                let color = (color[3] as u32) | ((color[2] as u32) << 8) | ((color[1] as u32)<<16) | ((color[0] as u32)<<24);
                buffer.fill(color);
                buffer.present().unwrap();
            },
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

fn forward_ports(gateway: Gateway, port : u16) -> Result<(), Box<dyn Error>> {
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

fn server(_config: Config, valid_ports : &[u16]) -> Result<(), Box<dyn Error>> {
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
                let user_packet = rx.recv().expect("Failed to communicate with server thread.");
                if !user_windows.contains_key(&user_packet.owner) {
                    println!("Got a new connection from {}!", user_packet.owner.to_string());
                    user_windows.insert(user_packet.owner.clone(), create_server_window(&elwt));
                }
                let window = &user_windows[&user_packet.owner];
                let monitor_size = window.primary_monitor().expect("Failed to detect primary monitor.").size();
                let state = user_packet.state;
                if state.visible {
                    window.set_outer_position(LogicalPosition::new(state.x * monitor_size.width as f32, state.y * monitor_size.height as f32));
                } else {
                    window.set_outer_position(LogicalPosition::new(-1000, -1000));
                }
                window.request_redraw();
            },
            _ => ()
        }
    }).unwrap();

    delete_ports(port).expect("Failed to unforward ports!");
    Ok(())
}

fn create_server_window(event_loop : &EventLoopWindowTarget<()>) -> Rc<Window> {
    let window = Rc::new(WindowBuilder::new().with_title("Laser Pointer")
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(24, 24))
        .with_resizable(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_transparent(true)
        .build(&event_loop).expect("Failed to build window"));

    window.set_cursor_hittest(false).expect("Failed to set window to be passthrough.");
    window.set_outer_position(LogicalPosition::new(-1000, -1000));

    let pointer_image_bytes = include_bytes!("pointer.png");
    let pointer_image = image::load_from_memory(pointer_image_bytes).expect("Failed to load pointer image from memory?? uh oh");

    let context = softbuffer::Context::new(window.clone()).expect("Failed to create graphics context.");
    let mut surface = softbuffer::Surface::new(&context, window.clone()).expect("Failed to create graphics surface.");

    surface.resize(NonZeroU32::new(24).unwrap(), NonZeroU32::new(24).unwrap()).unwrap();
    let mut buffer = surface.buffer_mut().unwrap();

    let mut rng = rand::thread_rng();
    let random_color : u32 = rng.gen();

    for index in 0..(24 * 24) {
        let y = index / 24;
        let x = index % 24;
        let pixel = pointer_image.get_pixel(x,y).0;
        if pixel[0] == 255 && pixel[1] == 0 && pixel[2] == 255 {
            buffer[index as usize] = random_color | (255 << 24);
        } else {
            buffer[index as usize] = (pixel[0] as u32) | ((pixel[1] as u32) << 8) | ((pixel[2] as u32) << 16) | ((pixel[3] as u32) << 24);
        }
    }

    buffer.present().unwrap();
    return window;
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    #[arg(short, long, default_value="0.0.0.0:0")]
    address : String,
}

impl Config {
    pub fn new () -> Config {
        let output = Config::parse();
        output
    }
}
