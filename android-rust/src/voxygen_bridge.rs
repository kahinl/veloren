use std::ptr::NonNull;
use std::sync::{Arc, Mutex, OnceLock};

/// Placeholder bridge to voxygen's wgpu renderer.
///
/// When voxygen is ported to Android (winit removed, ANativeWindow surface used directly),
/// this module will instantiate voxygen::Renderer and forward render calls.
static VOXEGON_RENDERER: OnceLock<Mutex<Option<NonNull<VoxygenRendererState>>>> = OnceLock::new();

struct VoxygenRendererState {
    width: u32,
    height: u32,
    initialized: bool,
}

fn voxygen_renderer_storage() -> &'static Mutex<Option<NonNull<VoxygenRendererState>>> {
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
    let ptr = Box::into_raw(boxed);
    let non_null = NonNull::new(ptr).expect("voxygen renderer state pointer is null");
    voxygen_renderer_storage().lock().unwrap().replace(non_null);
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
        let state = unsafe { ptr.as_mut() };
        state.width = width;
        state.height = height;
    }
}

/// Shutdown voxygen renderer bridge.
pub fn voxygen_renderer_shutdown() {
    voxygen_renderer_storage().lock().unwrap().take();
}
