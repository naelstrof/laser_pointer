use std::error::Error;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::process::exit;
use laminar::{DeliveryGuarantee, Socket, SocketEvent};
use std::collections::HashMap;
use winit::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use winit::event::{Event, WindowEvent};
use winit::dpi::{LogicalPosition, LogicalSize};
use igd_next::{Gateway, PortMappingProtocol, search_gateway, SearchOptions};
use std::rc::Rc;
use winit::window::{Window, WindowBuilder, WindowLevel};
use std::num::NonZeroU32;
use image::{DynamicImage, GenericImageView};
use winit::platform::windows::WindowBuilderExtWindows;
use std::net::{IpAddr, SocketAddr};
use softbuffer::Surface;
use crate::{Config};
use crate::shared::LaserPointerState;

const CURSOR_SIZE : u32 = 64;

struct UserPacket {
    owner : SocketAddr,
    data : UserData,
}

struct UserWindow {
    window : Rc<Window>,
    surface : Surface<Rc<Window>, Rc<Window>>,
    frame : u32,
    image : DynamicImage
}

enum UserData {
    State(LaserPointerState),
    Image(DynamicImage),
}

pub fn get_gateway() -> Result<Gateway, Box<dyn Error>> {
    let local_ip = local_ip_addr::get_local_ip_address()?;
    let local_ip = format!("{}:0", local_ip);
    let local_ip : SocketAddr = local_ip.parse()?;
    let search_options = SearchOptions {
        bind_addr: local_ip,
        .. SearchOptions::default()
    };
    Ok(search_gateway(search_options)?)
}
fn forward_ports(port: &u16, gateway: &Gateway) -> Result<IpAddr, Box<dyn Error>> {
    let ip = gateway.get_external_ip()?;
    let local_addr = local_ip_addr::get_local_ip_address()?;
    let local_addr = format!("{}:{}", local_addr, port).parse().expect("Failed to get local socket.");
    gateway.add_port(PortMappingProtocol::UDP, port.clone(), local_addr, 38000, "Laser pointer!")?;
    Ok(ip)
}

fn delete_ports(port : &u16, gateway: &Gateway) -> Result<(), Box<dyn Error>> {
    gateway.remove_port(PortMappingProtocol::UDP, port.clone())?;
    println!("Successfully un-forwarded port {}", port);
    Ok(())
}

fn get_socket(valid_ports : &[u16]) -> Result<(Socket,Option<Gateway>), Box<dyn Error>> {
    for port in valid_ports {
        println!("Attempting to bind to port {}", port);
        let socket = match Socket::bind(format!("0.0.0.0:{}", port)) {
            Ok(socket) => socket,
            Err(error) => {
                println!("bind failed: {}", error);
                continue;
            },
        };
        let gateway = match get_gateway() {
            Ok(gateway) => gateway,
            Err(error) => {
                println!("{}", error);
                println!("You will need to forward port {} yourself.", port);
                println!("Press CTRL+C on this window to close it.");
                println!("--");
                println!("Running normally from here, but you'll need to share your own public IP (not your local) and the above port.");
                return Ok((socket, None));
            },
        };
        match forward_ports(port, &gateway) {
            Ok(ip) => {
                println!("Press CTRL+C on this window to close it.");
                println!("--");
                println!("Send this to your friend: {}:{}", ip, port);
                return Ok((socket, Some(gateway)));
            },
            Err(error) => {
                println!("Failed to forward port: {}", error);
                continue;
            },
        }
    }
    return Err(Box::from("Failed to bind to any available socket..."));
}

pub fn server(_config: Config, valid_ports : &[u16]) -> Result<(), Box<dyn Error>> {
    let (tx, rx): (Sender<UserPacket>, Receiver<UserPacket>) = channel();

    let (mut socket, gateway) = get_socket(valid_ports)?;
    let event_receiver = socket.get_event_receiver();
    let port = socket.local_addr()?.port();
    thread::spawn(move || socket.start_polling());

    let gateway_clone = gateway.to_owned();
    ctrlc::set_handler(move || {
        if gateway_clone.as_ref().is_some() {
            delete_ports(&port, &gateway_clone.as_ref().unwrap()).unwrap();
        }
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
                    tx.send(UserPacket {
                        owner: event,
                        data: UserData::State(LaserPointerState::new()),
                    }).expect("Failed to communicate with main thread.");
                },
                SocketEvent::Disconnect(event) => {
                    tx.send(UserPacket {
                        owner: event,
                        data: UserData::State(LaserPointerState::new()),
                    }).expect("Failed to communicate with main thread.");
                },
            }
        }
    });

    let mut user_windows = HashMap::new();
    let event_loop = EventLoop::new().expect("Failed to build event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let gateway_clone = gateway.to_owned();
    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                println!("The close button was pressed; stopping");
                if gateway_clone.as_ref().is_some() {
                    delete_ports(&port, &gateway_clone.as_ref().unwrap()).unwrap();
                }
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
                        if user_info.frame != state.frame {
                            set_frame(&mut user_info.surface, &user_info.image, state.frame);
                            user_info.frame = state.frame;
                        }
                    }
                    UserData::Image(image) => {
                        if image.width()%CURSOR_SIZE != 0 {
                            println!("Failed to use image, its width isn't a factor of {}!", CURSOR_SIZE);
                        } else if image.height() != CURSOR_SIZE {
                            println!("Failed to use image, its height isn't {}!", CURSOR_SIZE);
                        } else {
                            user_info.image = image;
                            set_frame(&mut user_info.surface, &user_info.image, 0);
                        }
                    }
                }
                window.request_redraw();
            },
            _ => ()
        }
    }).unwrap();

    if gateway.is_some() {
        delete_ports(&port, &gateway.unwrap()).unwrap();
    }
    Ok(())
}

fn set_frame(surface : &mut Surface<Rc<Window>,Rc<Window>>, image : &DynamicImage, frame : u32) {
    if image.width()-frame*CURSOR_SIZE <= 0 {
        return;
    }
    let mut buffer = surface.buffer_mut().unwrap();
    let image_crop = image.crop_imm(frame*CURSOR_SIZE,0,CURSOR_SIZE,CURSOR_SIZE);
    for index in 0..(CURSOR_SIZE * CURSOR_SIZE) {
        let y = index / CURSOR_SIZE;
        let x = index % CURSOR_SIZE;
        buffer[index as usize] = u32::from_ne_bytes(image_crop.get_pixel(x,y).0);
    }
    buffer.present().unwrap();
}

fn create_server_window(event_loop : &EventLoopWindowTarget<()>) -> UserWindow {
    let window = Rc::new(WindowBuilder::new().with_title("Laser Pointer")
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(64, 64))
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
    let mut surface = Surface::new(&context, window.clone()).expect("Failed to create graphics surface.");

    surface.resize(NonZeroU32::new(CURSOR_SIZE).unwrap(), NonZeroU32::new(CURSOR_SIZE).unwrap()).unwrap();

    set_frame(&mut surface, &pointer_image, 0);

    UserWindow {
        window,
        surface,
        frame : 0,
        image : pointer_image,
    }
}