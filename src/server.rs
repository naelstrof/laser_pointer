use std::error::Error;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread;
use std::process::exit;
use std::collections::HashMap;
use winit::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use winit::event::{Event, WindowEvent};
use winit::dpi::{LogicalPosition, LogicalSize};
use std::rc::Rc;
use winit::window::{Window, WindowBuilder, WindowLevel};
use std::num::NonZeroU32;
use std::time::SystemTime;
use image::{DynamicImage, GenericImageView};
use winit::platform::windows::WindowBuilderExtWindows;
use softbuffer::Surface;
use steamworks::{Client, P2PSessionRequest, SteamId};
use crate::{Config};
use crate::shared::{CURSOR_SIZE, UserAnimationStates, UserState, UserPacket, APP_ID};
use crate::shared::UserState::Idle;

enum UserData {
    State(UserState),
    AnimationStates(UserAnimationStates),
    Image(DynamicImage),
}

struct ThreadPacket {
    owner : SteamId,
    data : UserData,
}

struct UserWindow {
    window : Rc<Window>,
    surface : Surface<Rc<Window>, Rc<Window>>,
    frame : u32,
    state : UserState,
    animation_set : UserAnimationStates,
    image : DynamicImage
}

pub fn server(_config: Config) -> Result<(), Box<dyn Error>> {
    let (steam_client, single_client) = Client::init_app(APP_ID)?;

    let steam_client_copy = steam_client.to_owned();
    steam_client.register_callback(move |session_request : P2PSessionRequest| {
        let networking = steam_client_copy.networking();
        networking.accept_p2p_session(session_request.remote);
    });

    let steam_client_copy_also = steam_client.to_owned();
    let steam_id = steam_client.user().steam_id().raw();
    println!("Press CTRL+C to close.");
    println!("--");
    println!("Please share this with your friend: {}", steam_id);

    let (tx, rx): (Sender<ThreadPacket>, Receiver<ThreadPacket>) = channel();
    thread::spawn(move || {
        loop {
            let mut buf = [0;1000000];
            match steam_client.networking().read_p2p_packet(&mut buf) {
                None => {
                    single_client.run_callbacks();
                    let interval = std::time::Duration::from_millis(10);
                    thread::sleep(interval);
                }
                Some((steam_id, amt)) => {
                    let buf = &mut buf[..amt];
                    let deserialize: Result<UserPacket,_> = serde_json::from_slice(buf);
                    match deserialize {
                        Ok(packet) => {
                            match packet {
                                UserPacket::State(state) => {
                                    match tx.send(ThreadPacket {
                                        owner: steam_id,
                                        data: UserData::State(state),
                                    }) {
                                        Ok(_) => {}
                                        Err(_) => { break }
                                    }
                                }
                                UserPacket::AnimationSet(set) => {
                                    match tx.send(ThreadPacket {
                                        owner: steam_id,
                                        data: UserData::AnimationStates(set),
                                    }) {
                                        Ok(_) => {}
                                        Err(_) => { break }
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            println!("{}", err);
                            match image::load_from_memory(buf) {
                                Ok(image) => {
                                    if image.width()%CURSOR_SIZE != 0 {
                                        println!("Failed to use image, its width isn't a factor of {}!", CURSOR_SIZE);
                                    } else if image.height() != CURSOR_SIZE {
                                        println!("Failed to use image, its height isn't {}!", CURSOR_SIZE);
                                    } else {
                                        match tx.send(ThreadPacket {
                                            owner: steam_id,
                                            data: UserData::Image(image),
                                        }) {
                                            Ok(_) => {}
                                            Err(_) => {break}
                                        }
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

    let now = SystemTime::now();
    let mut user_windows = HashMap::new();
    let event_loop = EventLoop::new().expect("Failed to build event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
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
                update_windows(&now, &mut user_windows);
                let user_packet = match rx.try_recv() {
                    Ok(packet) => packet,
                    Err(TryRecvError::Empty) => {
                        let interval = std::time::Duration::from_millis(10);
                        thread::sleep(interval);
                        return;
                    }
                    Err(TryRecvError::Disconnected) => {
                        println!("Lost connection with server thread.");
                        exit(1);
                    }
                };
                if !user_windows.contains_key(&user_packet.owner) {
                    let friend_name = steam_client_copy_also.friends().get_friend(user_packet.owner).name();
                    println!("Got a connection from {}", friend_name);
                    user_windows.insert(user_packet.owner.clone(), create_server_window(&elwt));
                }
                let user_info = user_windows.get_mut(&user_packet.owner).unwrap();
                let window = &user_info.window;
                match user_packet.data {
                    UserData::State(state) => {
                        user_info.state = state.clone();
                        let monitor_size = window.primary_monitor().expect("Failed to detect primary monitor.").size();
                        match state {
                            UserState::Idle => {
                                window.set_outer_position(LogicalPosition::new(-1000, -1000));
                            }
                            UserState::Visible(position) => {
                                window.set_outer_position(LogicalPosition::new(position.x * monitor_size.width as f32, position.y * monitor_size.height as f32));
                                let frame = user_info.animation_set.visible.get_frame(now.elapsed().unwrap().as_secs_f32()).index;
                                if user_info.frame != frame {
                                    set_frame(&mut user_info.surface, &user_info.image, frame);
                                    user_info.frame = frame;
                                }
                            }
                            UserState::Flashing(position) => {
                                window.set_outer_position(LogicalPosition::new(position.x * monitor_size.width as f32, position.y * monitor_size.height as f32));
                                let frame = user_info.animation_set.flashing.get_frame(now.elapsed().unwrap().as_secs_f32()).index;
                                if user_info.frame != frame {
                                    set_frame(&mut user_info.surface, &user_info.image, frame);
                                    user_info.frame = frame;
                                }
                            }
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
                    UserData::AnimationStates(new_animation_set) => {
                        println!("Received custom animation set.");
                        user_info.animation_set = new_animation_set;
                    }
                }
                window.request_redraw();
            },
            _ => ()
        }
    }).unwrap();
    Ok(())
}

fn update_windows(now : &SystemTime, windows : &mut HashMap<SteamId,UserWindow>) {
    for (_steamid, user_info) in windows {
        match &user_info.state {
            UserState::Visible(_) => {
                let frame = user_info.animation_set.visible.get_frame(now.elapsed().unwrap().as_secs_f32()).index;
                if user_info.frame != frame {
                    set_frame(&mut user_info.surface, &user_info.image, frame);
                    user_info.frame = frame;
                }
            }
            UserState::Flashing(_) => {
                let frame = user_info.animation_set.flashing.get_frame(now.elapsed().unwrap().as_secs_f32()).index;
                if user_info.frame != frame {
                    set_frame(&mut user_info.surface, &user_info.image, frame);
                    user_info.frame = frame;
                }
            }
            _ => {}
        }
    }
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
        state : Idle,
        frame : 0,
        image : pointer_image,
        animation_set : UserAnimationStates::new()
    }
}