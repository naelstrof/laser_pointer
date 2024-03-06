use std::error::Error;
use winit::window::{Icon, WindowBuilder};
use winit::event_loop::{ControlFlow, EventLoop};
use std::rc::Rc;
use winit::dpi::LogicalSize;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use laminar::{Packet, Socket};
use std::num::NonZeroU32;
use winit::event::{Event, MouseButton, WindowEvent};
use image::GenericImageView;
use winit::platform::windows::WindowBuilderExtWindows;
use std::net::ToSocketAddrs;
use crate::{Config};
use crate::shared::LaserPointerState;

fn get_socket(valid_ports : &[u16]) -> Result<Socket, Box<dyn Error>> {
    for port in valid_ports {
        println!("Attempting to bind to port {}", port);
        match Socket::bind(format!("0.0.0.0:{}", port)) {
            Ok(socket) => return Ok(socket),
            Err(error) => {
                println!("Failed to bind: {}", error);
                continue;
            }
        };
    }
    return Err(Box::from("Failed to find valid socket out of allocated ports..."));
}
pub fn client(config: Config, valid_ports : &[u16]) -> Result<(), Box<dyn Error>> {
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


    let valid_port_clone = valid_ports.to_owned();
    thread::spawn(move || {
        let mut socket= get_socket(&valid_port_clone).expect("Failed to get socket.");
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
                match button {
                    MouseButton::Left => {
                        laser_pointer_state.visible = state.is_pressed();
                        tx.send(laser_pointer_state).unwrap();
                    }
                    MouseButton::Right => {
                        laser_pointer_state.frame = match state.is_pressed() {
                            true => 1,
                            false => 0,
                        };
                        tx.send(laser_pointer_state).unwrap();
                    }
                    MouseButton::Middle => {}
                    MouseButton::Back => {}
                    MouseButton::Forward => {}
                    MouseButton::Other(_) => {}
                }
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
