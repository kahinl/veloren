use std::sync::{Mutex, OnceLock};

/// Placeholder bridge to voxygen's wgpu renderer.
///
/// When voxygen is ported to Android (winit removed, ANativeWindow surface used directly),
/// this module will instantiate voxygen::Renderer and forward render calls.
static VOXEGON_RENDERER: OnceLock<Mutex<Option<Box<VoxygenRendererState>>>> = OnceLock::new();

struct VoxygenRendererState {
    width: u32,
    height: u32,
    initialized: bool,
}

fn voxygen_renderer_storage() -> &'static Mutex<Option<Box<VoxygenRendererState>>> {
    VOXEGON_RENDERER.get_or_init(|| Mutex::new(None))
}

/// Initialize voxygen renderer bridge.
pub fn voxygen_renderer_init(width: u32, height: u32) -> Result<(), String> {
    let state = VoxygenRendererState {
        width,
        height,
        initialized: true,
    };
    voxygen_renderer_storage()
        .lock()
        .unwrap()
        .replace(Box::new(state));
    Ok(())
}

/// Render one frame using voxygen (placeholder — clears screen until full port).
pub fn voxygen_renderer_render() -> Result<(), String> {
    let binding = voxygen_renderer_storage().lock().unwrap();
    let state = binding.as_ref();

    let Some(_state) = state else {
        return Err("Voxygen renderer not initialized".to_string());
    };
    Ok(())
}

/// Resize voxygen renderer.
pub fn voxygen_renderer_resize(width: u32, height: u32) {
    let binding = voxygen_renderer_storage().lock().unwrap();
    if let Some(state) = binding.as_mut() {
        state.width = width;
        state.height = height;
    }
}

/// Shutdown voxygen renderer bridge.
pub fn voxygen_renderer_shutdown() {
    voxygen_renderer_storage().lock().unwrap().take();
}
