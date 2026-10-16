use jni::JNIEnv;
use jni::objects::JObject;
use std::ptr::null_mut;

/// Wrapper around ANativeWindow pointer to make it Send + Sync.
/// Raw pointers are always Send + Sync, but the compiler sometimes struggles
/// with opaque FFI types.
struct AndroidWindow(*mut std::ffi::c_void);

unsafe impl Send for AndroidWindow {}
unsafe impl Sync for AndroidWindow {}

/// Holds the wgpu device, queue, and surface tied to an Android SurfaceView.
pub struct Renderer {
    _instance: Option<()>,
    _adapter: Option<()>,
    pub device: Option<()>,
    pub queue: Option<()>,
    pub surface: Option<()>,
    pub config: Option<()>,
    /// Keep the ANativeWindow alive for the lifetime of the surface.
    _window: AndroidWindow,
}

impl Renderer {
    pub fn new(_env: &JNIEnv, _surface: JObject, _width: u32, _height: u32) -> Result<Self, String> {
        // TODO: wgpu 22 + raw-window-handle 0.6 API uyumsuzluğu nedeniyle
        // gerçek Android NDK surface oluşturma şu an devre dışı.
        // Bu bir stub'dır — gerçek render için wgpu surface kodu eklenmeli.
        let _window = AndroidWindow(null_mut());

        Ok(Renderer {
            _instance: None,
            _adapter: None,
            device: None,
            queue: None,
            surface: None,
            config: None,
            _window,
        })
    }

    pub fn resize(&mut self, _width: u32, _height: u32) {
        // Stub
    }

    pub fn render(&mut self) -> Result<(), ()> {
        // Stub — no actual rendering yet
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // Stub — no ANativeWindow_release available in ndk-sys 0.6
    }
}
