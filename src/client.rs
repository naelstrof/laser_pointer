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
use std::time::SystemTime;
use serde::{Deserialize, Serialize};
use softbuffer::Surface;
use steamworks::{Client, SendType, SteamId};
use crate::{Config};
use crate::shared::{CURSOR_SIZE, LaserPointerState};

#[derive(PartialEq,Clone)]
struct MousePosition {
    x : f32,
    y : f32,
}
#[derive(PartialEq,Clone)]
enum UserState {
    Idle,
    Visible(MousePosition),
    Flashing(MousePosition)
}

struct MouseState  {
    left_mouse_down : bool,
    right_mouse_down : bool,
}

#[derive(Serialize,Deserialize,Debug)]
struct UserAnimationStates {
    idle : Animation,
    visible : Animation,
    flashing : Animation,
}
#[derive(Serialize,Deserialize,Debug)]
struct Animation {
    frames : Vec<Frame>,
}
#[derive(Serialize,Deserialize,Debug)]
struct Frame {
    index : u32,
    duration : f32,
}

impl UserAnimationStates {
    fn new() -> UserAnimationStates {
        UserAnimationStates {
            idle: Animation::new(),
            visible : Animation::new(),
            flashing : Animation {
                frames : vec![Frame {
                    index : 1,
                    .. Frame::new()
                }],
                .. Animation::new()
            },
        }
    }
}
impl Animation {
    fn new() -> Animation {
        Animation { frames : vec![Frame::new()] }
    }
    fn get_frame(&self, time : f32) -> &Frame {
        let mut total_time = 0.0;
        self.frames.iter().for_each(|frame| {
            total_time += frame.duration;
        });
        let curr_time = time%total_time;
        let mut frame_time = 0.0;
        let possible_frame=  self.frames.iter().find(|frame| {
            if frame_time+frame.duration > curr_time {
                return true;
            }
            frame_time+=frame.duration;
            return false;
        });
        match possible_frame {
            None => { self.frames.get(0).unwrap() }
            Some(frame) => frame
        }
    }
}

impl Frame {
    fn new() -> Frame {
        Frame { index: 0, duration: 1.0, }
    }
}

impl MouseState {
    fn new() -> MouseState {
        MouseState { left_mouse_down : false, right_mouse_down : false, }
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

    let mut laser_pointer_state = LaserPointerState::new();
    let mut laser_state = UserState::Idle;
    let mut mouse_state = MouseState::new();
    let (tx, rx): (Sender<LaserPointerState>, Receiver<LaserPointerState>) = channel();

    let animation_states = get_animations(&config.animation_json_path)?;

    let (steam_client, single_client) = Client::init_app(2686900)?;
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
        loop {
            let item = rx.recv().expect("Failed to read from main thread.");
            networking.send_p2p_packet(steam_server_id, SendType::Unreliable, serde_json::to_string(&item).unwrap().as_ref());
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

    let now = SystemTime::now();
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
                    laser_state = UserState::Flashing(MousePosition{
                        x : laser_pointer_state.x,
                        y : laser_pointer_state.y,
                    });
                } else if mouse_state.left_mouse_down {
                    laser_state = UserState::Visible(MousePosition {
                        x : laser_pointer_state.x,
                        y : laser_pointer_state.y,
                    });
                } else {
                    laser_state = UserState::Idle;
                }

                let new_frame = match laser_state {
                    UserState::Idle => { animation_states.idle.get_frame(now.elapsed().unwrap().as_secs_f32()).index }
                    UserState::Visible(_) => { animation_states.visible.get_frame(now.elapsed().unwrap().as_secs_f32()).index }
                    UserState::Flashing(_) => { animation_states.flashing.get_frame(now.elapsed().unwrap().as_secs_f32()).index }
                };

                if old_laser_state != laser_state || laser_pointer_state.frame != new_frame {
                    laser_pointer_state.visible = laser_state != UserState::Idle;
                    laser_pointer_state.frame = new_frame;
                    tx.send(laser_pointer_state).unwrap();
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
                laser_pointer_state.x = (position.x / window_size.width as f64) as f32;
                laser_pointer_state.y = (position.y / window_size.height as f64) as f32;
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
