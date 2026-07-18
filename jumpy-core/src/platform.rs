use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Edge {
    None,
    Left,
    Right,
    Top,
    Bottom,
}

pub trait PlatformHandler: Send + Sync {
    /// Get the current global mouse position (x, y)
    fn get_mouse_pos(&self) -> (i32, i32);
    
    /// Set the global mouse position
    fn set_mouse_pos(&self, x: i32, y: i32);
    
    /// Get the screen dimensions (width, height)
    fn get_screen_size(&self) -> (i32, i32);
    
    /// Send a relative mouse movement
    fn send_mouse_move(&self, dx: i32, dy: i32);
    
    /// Send a mouse click
    fn send_mouse_click(&self, button: &str, pressed: bool);
    
    /// Send a mouse scroll event
    fn send_mouse_scroll(&self, dy: i32);
}
