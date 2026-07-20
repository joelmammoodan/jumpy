use eframe::egui;
use jumpy_core::platform::PlatformHandler;
use jumpy_core::JumpyApp;
use std::sync::Arc;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_WHEEL,
    INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, SetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, ClipCursor,
    SetWindowsHookExW, UnhookWindowsHookEx, CallNextHookEx, GetMessageW, DispatchMessageW, TranslateMessage,
    WH_MOUSE_LL, WH_KEYBOARD_LL, MSG, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, 
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEWHEEL, WM_MOUSEMOVE, WM_KEYDOWN, WM_SYSKEYDOWN,
    MSLLHOOKSTRUCT, KBDLLHOOKSTRUCT
};
use windows_sys::Win32::Foundation::{POINT, RECT, LRESULT, WPARAM, LPARAM};
use std::mem::{size_of, zeroed};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use crossbeam_channel::{unbounded, Receiver, Sender};
use jumpy_core::network::MouseControlMsg;

#[derive(Clone)]
struct WindowsPlatform {
    is_capturing: Arc<AtomicBool>,
    event_rx: Receiver<MouseControlMsg>,
}

// We don't need rdev anymore, we use native virtual key codes.
// Native Windows hook state
static HOOK_TX: OnceLock<Sender<MouseControlMsg>> = OnceLock::new();
static IS_CAPTURING: OnceLock<Arc<AtomicBool>> = OnceLock::new();

unsafe extern "system" fn mouse_hook_proc(code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(is_capturing) = IS_CAPTURING.get() {
            if is_capturing.load(Ordering::SeqCst) {
                let msg = w_param as u32;
                let ms_struct = unsafe { *(l_param as *const MSLLHOOKSTRUCT) };
                

                let mut swallow = false;
                if let Some(tx) = HOOK_TX.get() {
                    match msg {
                        WM_LBUTTONDOWN => { tx.send(MouseControlMsg::Click { button: "Left".to_string(), pressed: true }).unwrap(); swallow = true; }
                        WM_LBUTTONUP => { tx.send(MouseControlMsg::Click { button: "Left".to_string(), pressed: false }).unwrap(); swallow = true; }
                        WM_RBUTTONDOWN => { tx.send(MouseControlMsg::Click { button: "Right".to_string(), pressed: true }).unwrap(); swallow = true; }
                        WM_RBUTTONUP => { tx.send(MouseControlMsg::Click { button: "Right".to_string(), pressed: false }).unwrap(); swallow = true; }
                        WM_MBUTTONDOWN => { tx.send(MouseControlMsg::Click { button: "Middle".to_string(), pressed: true }).unwrap(); swallow = true; }
                        WM_MBUTTONUP => { tx.send(MouseControlMsg::Click { button: "Middle".to_string(), pressed: false }).unwrap(); swallow = true; }
                        WM_MOUSEWHEEL => { 
                            let delta = (ms_struct.mouseData >> 16) as i16 as f32; // wheel delta is high word
                            tx.send(MouseControlMsg::Scroll { dy: delta }).unwrap();
                            swallow = true;
                        }
                        WM_MOUSEMOVE => {} // Do not swallow mouse move
                        _ => {}
                    }
                }
                
                if swallow {
                    return 1; // 1 means swallow the event!
                }
            }
        }
    }
    unsafe { CallNextHookEx(0, code, w_param, l_param) }
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(is_capturing) = IS_CAPTURING.get() {
            if is_capturing.load(Ordering::SeqCst) {
                let msg = w_param as u32;
                let kbd_struct = unsafe { *(l_param as *const KBDLLHOOKSTRUCT) };
                let vk_code = kbd_struct.vkCode;
                
                if vk_code == 27 { // ESC
                    is_capturing.store(false, Ordering::SeqCst);
                    if let Some(tx) = HOOK_TX.get() {
                        tx.send(MouseControlMsg::ReturnControl).unwrap();
                    }
                    return unsafe { CallNextHookEx(0, code, w_param, l_param) };
                }
                
                let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
                
                if let Some(tx) = HOOK_TX.get() {
                    // Send the raw virtual key code to the network
                    tx.send(MouseControlMsg::Key { key_code: vk_code, down }).unwrap();
                }
                return 1; // Swallow ALL keys while capturing!
            }
        }
    }
    unsafe { CallNextHookEx(0, code, w_param, l_param) }
}

impl WindowsPlatform {
    fn new() -> Self {
        let is_capturing = Arc::new(AtomicBool::new(false));
        let (tx, rx) = unbounded();
        
        let hook_capturing = Arc::clone(&is_capturing);
        let hook_tx = tx.clone();
        
        std::thread::spawn(move || {
            let _ = HOOK_TX.set(hook_tx);
            let _ = IS_CAPTURING.set(hook_capturing);
            
            unsafe {
                let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), 0, 0);
                let kbd_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), 0, 0);
                
                if mouse_hook == 0 || kbd_hook == 0 {
                    println!("Error: Failed to install global hooks!");
                }
                
                let mut msg: MSG = zeroed();
                while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                
                if mouse_hook != 0 { UnhookWindowsHookEx(mouse_hook); }
                if kbd_hook != 0 { UnhookWindowsHookEx(kbd_hook); }
            }
        });
        
        Self { is_capturing, event_rx: rx }
    }
}

impl PlatformHandler for WindowsPlatform {
    fn get_mouse_pos(&self) -> (i32, i32) {
        unsafe {
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            (pt.x, pt.y)
        }
    }

    fn set_mouse_pos(&self, x: i32, y: i32) {
        unsafe {
            SetCursorPos(x, y);
        }
    }

    fn get_screen_size(&self) -> (i32, i32) {
        unsafe {
            let w = GetSystemMetrics(SM_CXSCREEN);
            let h = GetSystemMetrics(SM_CYSCREEN);
            (w, h)
        }
    }

    fn send_mouse_move(&self, dx: i32, dy: i32) {
        unsafe {
            let mut input: INPUT = zeroed();
            input.r#type = INPUT_MOUSE;
            let mut mi: MOUSEINPUT = zeroed();
            mi.dx = dx;
            mi.dy = dy;
            mi.dwFlags = MOUSEEVENTF_MOVE;
            input.Anonymous.mi = mi;
            SendInput(1, &input as *const INPUT, size_of::<INPUT>() as i32);
        }
    }

    fn send_mouse_click(&self, button: &str, pressed: bool) {
        let flags = match (button, pressed) {
            ("Left", true) => MOUSEEVENTF_LEFTDOWN,
            ("Left", false) => MOUSEEVENTF_LEFTUP,
            ("Right", true) => MOUSEEVENTF_RIGHTDOWN,
            ("Right", false) => MOUSEEVENTF_RIGHTUP,
            ("Middle", true) => MOUSEEVENTF_MIDDLEDOWN,
            ("Middle", false) => MOUSEEVENTF_MIDDLEUP,
            _ => return,
        };
        unsafe {
            let mut input: INPUT = zeroed();
            input.r#type = INPUT_MOUSE;
            let mut mi: MOUSEINPUT = zeroed();
            mi.dwFlags = flags;
            input.Anonymous.mi = mi;
            SendInput(1, &input as *const INPUT, size_of::<INPUT>() as i32);
        }
    }

    fn send_mouse_scroll(&self, dy: i32) {
        unsafe {
            let mut input: INPUT = zeroed();
            input.r#type = INPUT_MOUSE;
            let mut mi: MOUSEINPUT = zeroed();
            mi.mouseData = (dy * 120) as u32;
            mi.dwFlags = MOUSEEVENTF_WHEEL;
            input.Anonymous.mi = mi;
            SendInput(1, &input as *const INPUT, size_of::<INPUT>() as i32);
        }
    }
    
    fn send_key(&self, key_code: u32, down: bool) {
        let vk = map_evdev_to_vk(key_code);
        if vk == 0 { return; }
        
        unsafe {
            let mut input: INPUT = zeroed();
            input.r#type = INPUT_KEYBOARD;
            let mut ki: KEYBDINPUT = zeroed();
            ki.wVk = vk;
            ki.dwFlags = if down { 0 } else { KEYEVENTF_KEYUP };
            input.Anonymous.ki = ki;
            SendInput(1, &input as *const INPUT, size_of::<INPUT>() as i32);
        }
    }
    
    fn set_capture_mode(&self, active: bool, cx: i32, cy: i32) {
        self.is_capturing.store(active, Ordering::SeqCst);
        unsafe {
            if active {
                SetCursorPos(cx, cy);
                
                let rect = RECT {
                    left: cx - 10,
                    top: cy - 10,
                    right: cx + 10,
                    bottom: cy + 10,
                };
                ClipCursor(&rect);
            } else {
                ClipCursor(std::ptr::null());
            }
        }
    }
    
    fn get_grabbed_events(&self) -> Vec<MouseControlMsg> {
        let mut events = Vec::new();
        while let Ok(msg) = self.event_rx.try_recv() {
            events.push(msg);
        }
        events
    }
}

fn map_evdev_to_vk(evdev: u32) -> u16 {
    match evdev {
        1 => 0x1B, // ESC
        2 => 0x31, // 1
        3 => 0x32, // 2
        4 => 0x33, // 3
        5 => 0x34, // 4
        6 => 0x35, // 5
        7 => 0x36, // 6
        8 => 0x37, // 7
        9 => 0x38, // 8
        10 => 0x39, // 9
        11 => 0x30, // 0
        12 => 0xBD, // MINUS
        13 => 0xBB, // EQUAL
        14 => 0x08, // BACKSPACE
        15 => 0x09, // TAB
        16 => 0x51, // Q
        17 => 0x57, // W
        18 => 0x45, // E
        19 => 0x52, // R
        20 => 0x54, // T
        21 => 0x59, // Y
        22 => 0x55, // U
        23 => 0x49, // I
        24 => 0x4F, // O
        25 => 0x50, // P
        26 => 0xDB, // [
        27 => 0xDD, // ]
        28 => 0x0D, // ENTER
        29 => 0x11, // LCTRL
        30 => 0x41, // A
        31 => 0x53, // S
        32 => 0x44, // D
        33 => 0x46, // F
        34 => 0x47, // G
        35 => 0x48, // H
        36 => 0x4A, // J
        37 => 0x4B, // K
        38 => 0x4C, // L
        39 => 0xBA, // ;
        40 => 0xDE, // '
        41 => 0xC0, // `
        42 => 0x10, // LSHIFT
        43 => 0xDC, // \
        44 => 0x5A, // Z
        45 => 0x58, // X
        46 => 0x43, // C
        47 => 0x56, // V
        48 => 0x42, // B
        49 => 0x4E, // N
        50 => 0x4D, // M
        51 => 0xBC, // ,
        52 => 0xBE, // .
        53 => 0xBF, // /
        54 => 0x10, // RSHIFT
        56 => 0x12, // LALT
        57 => 0x20, // SPACE
        58 => 0x14, // CAPS
        103 => 0x26, // UP
        105 => 0x25, // LEFT
        106 => 0x27, // RIGHT
        108 => 0x28, // DOWN
        _ => 0
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 520.0])
            .with_min_inner_size([680.0, 480.0])
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "JUMPY - Windows",
        options,
        Box::new(|cc| {
            let platform = Box::new(WindowsPlatform::new());
            let app = JumpyApp::new(cc, platform);
            
            let platform_arc = Arc::new(WindowsPlatform::new()) as Arc<dyn PlatformHandler + Send + Sync>;
            JumpyApp::start_mouse_receiver(Arc::clone(&app.state), platform_arc);
            
            Ok(Box::new(app))
        }),
    )
}
