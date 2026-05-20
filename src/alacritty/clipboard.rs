use alacritty_terminal::term::ClipboardType;

pub struct Clipboard;

impl Clipboard {
    pub fn new_nop() -> Self { Self }
    pub fn store(&mut self, _ty: ClipboardType, _text: String) {}
    pub fn load(&mut self, _ty: ClipboardType) -> String { String::new() }
}
