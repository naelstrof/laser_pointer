use std::net::{ToSocketAddrs, SocketAddr};
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::window::{Icon, Window, WindowLevel};
use serde::{Serialize, Deserialize};
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc::channel;
use std::thread;
use clap::Parser;
use winit::event::MouseButton;
use std::error::Error;
use std::process::exit;
use igd_next::{Gateway, PortMappingProtocol, search_gateway, SearchOptions};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::rc::Rc;
use image::{DynamicImage, GenericImageView};
use rand::Rng;
use winit::event_loop::EventLoopWindowTarget;
use winit::platform::windows::WindowBuilderExtWindows;
use laminar::{Socket, Packet, SocketEvent, DeliveryGuarantee};

#[derive(Serialize,Deserialize,Debug,Copy,Clone)]
struct LaserPointerState {
    visible : bool,
    x : f32,
    y : f32,
}

enum UserData {
    State(LaserPointerState),
    Image(DynamicImage),
}
struct UserPacket {
    owner : SocketAddr,
    data : UserData,
}

struct UserWindow {
    window : Rc<Window>,
    surface : softbuffer::Surface<Rc<Window>, Rc<Window>>
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

fn get_socket(valid_ports : &[u16], should_forward_port : bool) -> Result<Socket, Box<dyn Error>> {
    for port in valid_ports.iter() {
        println!("Attempting to bind to port {}", port);
        let socket = match Socket::bind(format!("0.0.0.0:{}", port)) {
            Ok(socket) => socket,
            Err(error) => {
                println!("Failed to bind to port: {}", error);
                continue
            },
        };

        if should_forward_port {
            let local_ip = local_ip_addr::get_local_ip_address()?;
            let local_ip = format!("{}:0", local_ip);
            let local_ip : SocketAddr = local_ip.parse()?;
            println!("Searching for gateway on {}", local_ip);
            let search_options = SearchOptions {
                bind_addr: local_ip,
                .. SearchOptions::default()
            };
            let gateway = search_gateway(search_options);
            if gateway.is_err() {
                println!("Port forwarding failed: Gateway couldn't be found: {:?}", gateway.err());
                println!("You will need to forward port {} yourself.", port);
                println!("Running normally from here, but you'll need to share your own public IP (not your local) and the above port.");
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
    let icon_small_image = include_bytes!("icon.png");
    let icon_small_image = image::load_from_memory(icon_small_image).expect("Failed to load icon image from memory?? uh oh");
    let (icon_width, icon_height) = icon_small_image.dimensions();
    let icon_small = Icon::from_rgba(icon_small_image.into_bytes(), icon_width, icon_height)?;

    let icon_big_image = include_bytes!("icon_big.png");
    let icon_big_image = image::load_from_memory(icon_big_image).expect("Failed to load icon image from memory?? uh oh");
    let (icon_width, icon_height) = icon_big_image.dimensions();
    let icon_big = Icon::from_rgba(icon_big_image.into_bytes(), icon_width, icon_height)?;

    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(WindowBuilder::new().with_title("Laser Pointer")
        .with_min_inner_size(LogicalSize::new(200, 80))
        .with_transparent(true)
        .with_taskbar_icon(Some(icon_big))
        .with_window_icon(Some(icon_small))
        .build(&event_loop).unwrap());
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut laser_pointer_state = LaserPointerState::new();
    let (tx, rx): (Sender<LaserPointerState>, Receiver<LaserPointerState>) = channel();


    let port_clone = valid_ports.to_owned();
    thread::spawn(move || {
        let mut socket = get_socket(&port_clone,false).expect("Failed to connect to socket.");
        let server_details = config.address;
        let server: Vec<_> = server_details.to_socket_addrs().expect("Unable to resolve domain") .collect();
        let packet_sender = socket.get_packet_sender();
        thread::spawn( move || socket.start_polling());

        if &config.cursor_path != "" {
            let file_bytes = std::fs::read(&config.cursor_path).expect("Failed to read cursor image."); // The file is compressed, and has endian information.
            println!("Sent server a cursor of size {}", &file_bytes.len());
            let reliable = Packet::reliable_unordered(server[0], file_bytes);
            packet_sender.send(reliable).expect("Failed to send cursor image.");
        }
        loop {
            let interval = std::time::Duration::from_millis(20);
            thread::sleep(interval);
            match rx.try_iter().last() {
                None => {}
                Some(item) => {
                    let unreliable = Packet::unreliable(server[0], Vec::from(serde_json::to_string(&item).unwrap()));
                    packet_sender.send(unreliable).expect("Failed to send packet.");
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
                buffer.fill(0);
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
    println!("Press CTRL+C on this window to close it.");
    println!("--");
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
    let (tx, rx): (Sender<UserPacket>, Receiver<UserPacket>) = channel();
    let mut socket = get_socket(valid_ports, true)?;
    let event_receiver = socket.get_event_receiver();
    let port = socket.local_addr()?.port();
    thread::spawn(move || socket.start_polling());

    ctrlc::set_handler(move || {
        delete_ports(port).unwrap();
        exit(1);
    }).expect("Failed to set ctrl+c handler.");

    thread::spawn(move || {
        loop {
            let result = event_receiver.recv();
            if result.is_err() {
                println!("Something went wrong with receiving packets: {:?}", result.err());
                continue;
            }
            match result.unwrap() {
                SocketEvent::Packet(packet) => {
                    match packet.delivery_guarantee() {
                        DeliveryGuarantee::Unreliable => {
                            //println!("Got {}", String::from_utf8(packet.payload().to_vec()).unwrap());
                            let state = serde_json::from_slice(packet.payload()).expect("Failed to deserialize packet");
                            tx.send(UserPacket {
                                owner: packet.addr(),
                                data: UserData::State(state)
                            }).expect("Failed to communicate with main thread.");
                        },
                        DeliveryGuarantee::Reliable => {
                            let image = image::load_from_memory(packet.payload()).expect(format!("{} sent us a bad cursor image, failed to load it!", packet.addr()).as_str());
                            let user_image_packet = UserPacket {
                                owner: packet.addr(),
                                data: UserData::Image(image)
                            };
                            tx.send(user_image_packet).unwrap()
                        }
                    }
                },
                SocketEvent::Connect(event) => println!("{} connected", event),
                SocketEvent::Timeout(event) => {
                    println!("{} timed out", event);
                    tx.send(UserPacket {
                        owner: event,
                        data: UserData::State(LaserPointerState {
                            visible : false,
                            x : 0.0,
                            y : 0.0,
                        })
                    }).expect("Failed to communicate with main thread.");
                },
                SocketEvent::Disconnect(event) => {
                    println!("{} disconnected", event);
                    tx.send(UserPacket {
                        owner: event,
                        data: UserData::State(LaserPointerState {
                            visible : false,
                            x : 0.0,
                            y : 0.0,
                        })
                    }).expect("Failed to communicate with main thread.");
                },
            }
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
                delete_ports(port).expect("Failed to unforward ports!");
                exit(0);
            },
            Event::AboutToWait => {
                let user_packet = rx.recv().expect("Failed to communicate with server thread.");
                if !user_windows.contains_key(&user_packet.owner) {
                    println!("Got a new connection from {}!", user_packet.owner.to_string());
                    user_windows.insert(user_packet.owner.clone(), create_server_window(&elwt));
                }
                let user_info = user_windows.get_mut(&user_packet.owner).unwrap();
                let window = &user_info.window;
                match user_packet.data {
                    UserData::State(state) => {
                        let monitor_size = window.primary_monitor().expect("Failed to detect primary monitor.").size();
                        if state.visible {
                            window.set_outer_position(LogicalPosition::new(state.x * monitor_size.width as f32, state.y * monitor_size.height as f32));
                        } else {
                            window.set_outer_position(LogicalPosition::new(-1000, -1000));
                        }
                    }
                    UserData::Image(image) => {
                        let mut rng = rand::thread_rng();
                        let random_color : u32 = rng.gen();
                        let surface = &mut user_info.surface;
                        let mut buffer = surface.buffer_mut().unwrap();
                        for index in 0..(24 * 24) {
                            let y = index / 24;
                            let x = index % 24;
                            let pixel = image.get_pixel(x,y).0;
                            if pixel[0] == 255 && pixel[1] == 0 && pixel[2] == 255 {
                                buffer[index as usize] = random_color | (255 << 24);
                            } else {
                                buffer[index as usize] = (pixel[0] as u32) | ((pixel[1] as u32) << 8) | ((pixel[2] as u32) << 16) | ((pixel[3] as u32) << 24);
                            }
                        }
                        buffer.present().unwrap();
                    }
                }
                window.request_redraw();
            },
            _ => ()
        }
    }).unwrap();

    delete_ports(port).expect("Failed to unforward ports!");
    Ok(())
}

fn create_server_window(event_loop : &EventLoopWindowTarget<()>) -> UserWindow {
    let window = Rc::new(WindowBuilder::new().with_title("Laser Pointer")
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(24, 24))
        .with_resizable(false)
        .with_skip_taskbar(true)
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
    UserWindow {
        window,
        surface
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
