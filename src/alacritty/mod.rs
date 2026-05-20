pub mod event;
pub mod input;
pub mod config;
pub mod display;
pub mod message_bar;
pub mod scheduler;
pub mod clipboard;

pub use self::display::Display;
pub use self::clipboard::Clipboard;
pub use self::scheduler::Scheduler;
pub use self::message_bar::MessageBuffer as MessageBar;

pub struct Window;
impl Window {
    pub fn id(&self) -> winit::window::WindowId {
        // Dummy ID
        unsafe { std::mem::transmute(0usize) }
    }
}
