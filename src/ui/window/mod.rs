//! Window on the GUI.

#[cfg(feature = "backend_gtk4")]
mod gtk;
#[cfg(feature = "backend_piston")]
mod piston;

#[cfg(feature = "backend_gtk4")]
pub use gtk::Window;
#[cfg(feature = "backend_piston")]
pub use piston::Window;
