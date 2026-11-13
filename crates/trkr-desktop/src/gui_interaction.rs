//! Temporary defaults owned only by the hardware-disabled GUI interaction launcher.
#[derive(Default)]
pub struct GuiInteraction {
    pub sandbox: Option<tempfile::TempDir>,
}
