pub struct Scheduler;
pub enum Topic {
    DelayedSearch,
    SelectionScrolling,
}
pub struct TimerId;
impl TimerId {
    pub fn new(topic: Topic, _window_id: winit::window::WindowId) -> Self { Self }
}
impl Scheduler {
    pub fn unschedule(&mut self, _id: TimerId) {}
}
