use std::error::Error;
use winit::window::{Icon, Window, WindowBuilder};
use winit::event_loop::{ControlFlow, EventLoop};
use std::rc::Rc;
use winit::dpi::LogicalSize;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::fs::File;
use std::num::NonZeroU32;
use winit::event::{Event, MouseButton, WindowEvent};
use image::GenericImageView;
use winit::platform::windows::WindowBuilderExtWindows;
use softbuffer::Surface;
use steamworks::{Client, SendType, SteamId};
use crate::{Config};
use crate::shared::{UserState, CURSOR_SIZE, MousePosition, UserAnimationStates, APP_ID, UserPacket};

struct MouseState  {
    left_mouse_down : bool,
    right_mouse_down : bool,
    position : MousePosition,
}

impl MouseState {
    fn new() -> MouseState {
        MouseState {
            left_mouse_down : false,
            right_mouse_down : false,
            position : MousePosition { x : 0.0, y : 0.0, }
        }
    }
}

pub fn client(config: Config) -> Result<(), Box<dyn Error>> {
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

    let mut laser_state = UserState::Idle;
    let mut mouse_state = MouseState::new();
    let (tx, rx): (Sender<UserState>, Receiver<UserState>) = channel();

    let (steam_client, single_client) = Client::init_app(APP_ID)?;
    thread::spawn(move || {
        loop {
            single_client.run_callbacks();
            let interval = std::time::Duration::from_millis(16);
            thread::sleep(interval);
        }
    });

    let steam_server_id = SteamId::from_raw(config.steam_id);
    thread::spawn(move || {
        let networking = steam_client.networking();
        if &config.cursor_path != "" {
            let file_bytes = std::fs::read(&config.cursor_path).expect("Failed to read cursor image."); // The file is compressed.
            let image = image::load_from_memory(&*file_bytes).expect("Failed to read cursor image.");
            if image.height() != CURSOR_SIZE || image.width()%CURSOR_SIZE != 0 {
                println!("Failed to load user image, its height needs to be {}, and the width needs to be a multiple of {}!", CURSOR_SIZE, CURSOR_SIZE);
            } else {
                println!("Sent server a cursor of size {}", &file_bytes.len());
                networking.send_p2p_packet(steam_server_id, SendType::Reliable, &*file_bytes);
            }
        }
        if &config.animation_json_path != "" {
            let animations = get_animations(&config.animation_json_path).unwrap();
            let packet = UserPacket::AnimationSet(animations);
            let packet_string = serde_json::to_string(&packet).unwrap();
            println!("Sent server custom animation states.");
            networking.send_p2p_packet(steam_server_id, SendType::Reliable, packet_string.as_ref());
        }
        loop {
            let item = rx.recv().expect("Failed to read from main thread.");
            let packet = UserPacket::State(item);
            let packet_string = serde_json::to_string(&packet).unwrap();
            networking.send_p2p_packet(steam_server_id, SendType::UnreliableNoDelay, packet_string.as_ref());
        }
    });

    let context = softbuffer::Context::new(window.clone()).expect("Failed to create graphics context.");
    let mut surface = Surface::new(&context, window.clone()).expect("Failed to create graphics surface.");
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
                let old_laser_state = laser_state.clone();
                if mouse_state.left_mouse_down && mouse_state.right_mouse_down {
                    laser_state = UserState::Flashing(mouse_state.position.clone());
                } else if mouse_state.left_mouse_down {
                    laser_state = UserState::Visible(mouse_state.position.clone());
                } else {
                    laser_state = UserState::Idle;
                }

                if old_laser_state != laser_state {
                    let copy = laser_state.to_owned();
                    tx.send(copy).unwrap();
                }
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
                fill_buffer_with_transparent(&window, &mut surface, width, height);
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
                        mouse_state = MouseState {
                            left_mouse_down : state.is_pressed(),
                            .. mouse_state
                        };
                    }
                    MouseButton::Right => {
                        mouse_state = MouseState {
                            right_mouse_down : state.is_pressed(),
                            .. mouse_state
                        };
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
                mouse_state = MouseState {
                    position : MousePosition {
                        x : (position.x / window_size.width as f64) as f32,
                        y : (position.y / window_size.height as f64) as f32,
                    },
                    .. mouse_state
                }
            },
            _ => ()
        }
    }).unwrap();
    Ok(())
}

fn get_animations(json_path : &str) -> Result<UserAnimationStates, Box<dyn Error>> {
    if json_path == "" {
        return Ok(UserAnimationStates::new());
    }
    Ok(serde_json::from_reader(File::open(json_path)?)?)
}

fn fill_buffer_with_transparent(window: &Rc<Window>, surface: &mut Surface<Rc<Window>, Rc<Window>>, mut width: u32, mut height: u32) {
    let (new_width, new_height) = {
        let size = window.inner_size();
        (size.width, size.height)
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
}
