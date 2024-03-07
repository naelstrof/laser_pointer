use std::error::Error;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::process::exit;
use std::collections::HashMap;
use winit::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use winit::event::{Event, WindowEvent};
use winit::dpi::{LogicalPosition, LogicalSize};
use std::rc::Rc;
use winit::window::{Window, WindowBuilder, WindowLevel};
use std::num::NonZeroU32;
use image::{DynamicImage, GenericImageView};
use winit::platform::windows::WindowBuilderExtWindows;
use softbuffer::Surface;
use steamworks::{Client, P2PSessionRequest, SteamId};
use crate::{Config};
use crate::shared::{CURSOR_SIZE, LaserPointerState};

enum UserData {
    State(LaserPointerState),
    Image(DynamicImage),
}

struct UserPacket {
    owner : SteamId,
    data : UserData,
}

struct UserWindow {
    window : Rc<Window>,
    surface : Surface<Rc<Window>, Rc<Window>>,
    frame : u32,
    image : DynamicImage
}

pub fn server(_config: Config) -> Result<(), Box<dyn Error>> {
    let (steam_client, single_client) = Client::init_app(2686900)?;
    thread::spawn(move || {
        loop {
            single_client.run_callbacks();
            let interval = std::time::Duration::from_millis(10);
            thread::sleep(interval);
        }
    });

    let steam_client_copy = steam_client.to_owned();
    steam_client.register_callback(move |session_request : P2PSessionRequest| {
        let networking = steam_client_copy.networking();
        networking.accept_p2p_session(session_request.remote);
        println!("Got connection!");
    });

    let (tx, rx): (Sender<UserPacket>, Receiver<UserPacket>) = channel();
    thread::spawn(move || {
        loop {
            let mut buf = [0;1000000];
            match steam_client.networking().read_p2p_packet(&mut buf) {
                None => {
                    let interval = std::time::Duration::from_millis(10);
                    thread::sleep(interval);
                }
                Some((steam_id, amt)) => {
                    let buf = &mut buf[..amt];
                    match serde_json::from_slice(buf) {
                        Ok(state) => {
                            tx.send(UserPacket {
                                owner : steam_id,
                                data : UserData::State(state),
                            }).unwrap();
                        }
                        Err(_err) => {
                            match image::load_from_memory(buf) {
                                Ok(image) => {
                                    if image.width()%CURSOR_SIZE != 0 {
                                        println!("Failed to use image, its width isn't a factor of {}!", CURSOR_SIZE);
                                    } else if image.height() != CURSOR_SIZE {
                                        println!("Failed to use image, its height isn't {}!", CURSOR_SIZE);
                                    } else {
                                        tx.send(UserPacket {
                                            owner: steam_id,
                                            data: UserData::Image(image),
                                        }).unwrap();
                                    }
                                }
                                Err(err) => {
                                    println!("Failed to load image from user, corruption? {}", err);
                                }
                            };
                        }
                    };
                }
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
                exit(0);
            },
            Event::AboutToWait => {
                let user_packet = rx.recv().expect("Failed to communicate with server thread.");
                if !user_windows.contains_key(&user_packet.owner) {
                    println!("Got a new connection!");
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