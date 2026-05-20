#[derive(Debug, Clone, Copy)]
pub struct SizeInfo {
    width: f32,
    height: f32,
    cell_width: f32,
    cell_height: f32,
    padding_x: f32,
    padding_y: f32,
}

impl SizeInfo {
    pub fn new(width: f32, height: f32, cell_width: f32, cell_height: f32, padding_x: f32, padding_y: f32) -> Self {
        Self { width, height, cell_width, cell_height, padding_x, padding_y }
    }

    pub fn width(&self) -> f32 { self.width }
    pub fn height(&self) -> f32 { self.height }
    pub fn cell_width(&self) -> f32 { self.cell_width }
    pub fn cell_height(&self) -> f32 { self.cell_height }
    pub fn padding_x(&self) -> f32 { self.padding_x }
    pub fn padding_y(&self) -> f32 { self.padding_y }

    pub fn columns(&self) -> usize {
        ((self.width - 2.0 * self.padding_x) / self.cell_width).floor() as usize
    }

    pub fn screen_lines(&self) -> usize {
        ((self.height - 2.0 * self.padding_y) / self.cell_height).floor() as usize
    }

    pub fn contains_point(&self, x: usize, y: usize) -> bool {
        (x as f32) >= self.padding_x && (x as f32) < self.width - self.padding_x &&
        (y as f32) >= self.padding_y && (y as f32) < self.height - self.padding_y
    }
}

pub struct Display {
    pub size_info: SizeInfo,
    pub window: crate::alacritty::Window,
}

impl Display {
    pub fn new(size_info: SizeInfo) -> Self {
        Self { size_info, window: crate::alacritty::Window }
    }
}
