use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock};

/// Placeholder bridge to voxygen's wgpu renderer.
///
/// When voxygen is ported to Android (winit removed, ANativeWindow surface used directly),
/// this module will instantiate voxygen::Renderer and forward render calls.
static VOXEGON_RENDERER: OnceLock<Mutex<Option<*mut VoxygenRendererState>>> = OnceLock::new();

struct VoxygenRendererState {
    width: u32,
    height: u32,
    initialized: bool,
}

fn voxygen_renderer_storage() -> &'static Mutex<Option<*mut VoxygenRendererState>> {
    VOXEGON_RENDERER.get_or_init(|| Mutex::new(None))
}

/// Initialize voxygen renderer bridge.
pub fn voxygen_renderer_init(width: u32, height: u32) -> Result<(), String> {
    let state = VoxygenRendererState {
        width,
        height,
        initialized: true,
    };
    let boxed = Box::new(state);
    let ptr = Box::into_raw(boxed) as *mut VoxygenRendererState;
    voxygen_renderer_storage().lock().unwrap().replace(ptr);
    Ok(())
}

/// Render one frame using voxygen (placeholder — clears screen until full port).
pub fn voxygen_renderer_render() -> Result<(), String> {
    let state = voxygen_renderer_storage().lock().unwrap().as_ref().and_then(|o| o.as_ref());
    let Some(_state) = state else {
        return Err("Voxygen renderer not initialized".to_string());
    };
    Ok(())
}

/// Resize voxygen renderer.
pub fn voxygen_renderer_resize(width: u32, height: u32) {
    if let Some(ptr) = voxygen_renderer_storage().lock().unwrap().as_ref().and_then(|o| o.as_ref()) {
        let state = unsafe { &mut *ptr };
        state.width = width;
        state.height = height;
    }
}

/// Shutdown voxygen renderer bridge.
pub fn voxygen_renderer_shutdown() {
    if let Some(ptr) = voxygen_renderer_storage().lock().unwrap().take() {
        unsafe {
            drop(Box::from_raw(ptr));
        }
    }
}