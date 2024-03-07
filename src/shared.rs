use serde::{Deserialize, Serialize};

pub const CURSOR_SIZE : u32 = 64;
pub const APP_ID : u32 = 480; // TODO: Replace with a real steam ID

#[derive(Serialize,Deserialize,PartialEq,Clone,Copy)]
pub struct MousePosition {
    pub x : f32,
    pub y : f32,
}
#[derive(Serialize,Deserialize,PartialEq,Clone)]
#[serde(tag = "state")]
pub enum UserState {
    Idle,
    Visible(MousePosition),
    Flashing(MousePosition)
}

#[derive(Serialize,Deserialize,PartialEq,Clone)]
#[serde(tag = "type")]
pub enum UserPacket {
    State(UserState),
    AnimationSet(UserAnimationStates)
}

#[derive(Serialize,Deserialize,PartialEq,Debug,Clone)]
pub struct UserAnimationStates {
    pub idle : Animation,
    pub visible : Animation,
    pub flashing : Animation,
}
#[derive(Serialize,Deserialize,PartialEq,Debug,Clone)]
pub struct Animation {
    frames : Vec<Frame>,
}
#[derive(Serialize,Deserialize,PartialEq,Debug,Clone,Copy)]
pub struct Frame {
    pub index : u32,
    pub duration : f32,
}

impl UserAnimationStates {
    pub fn new() -> UserAnimationStates {
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
    pub fn new() -> Animation {
        Animation { frames : vec![Frame::new()] }
    }
    pub fn get_frame(&self, time : f32) -> &Frame {
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
    pub fn new() -> Frame {
        Frame { index: 0, duration: 1.0, }
    }
}

