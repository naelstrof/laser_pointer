use serde::{Deserialize, Serialize};

#[derive(Serialize,Deserialize,Debug,Copy,Clone)]
pub struct LaserPointerState {
    pub visible : bool,
    pub x : f32,
    pub y : f32,
    pub frame : u32,
}

impl LaserPointerState {
    pub fn new() -> LaserPointerState {
        LaserPointerState { visible : false, x : 0.0, y : 0.0, frame: 0 }
    }
}