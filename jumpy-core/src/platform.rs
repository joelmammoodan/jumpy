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
    
    /// Enable or disable global input capture. When true, the OS cursor is frozen and movements are intercepted.
    fn set_capture_mode(&self, _active: bool, _x: i32, _y: i32) {}
    
    /// Get the raw hardware delta accumulated by the global hook. Returns (dx, dy) and resets the accumulator.
    fn get_raw_delta(&self) -> (f64, f64) { (0.0, 0.0) }
}
