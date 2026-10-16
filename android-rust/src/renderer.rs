use jni::JNIEnv;
use jni::objects::JObject;
use std::ptr::null_mut;

struct AndroidWindow(*mut std::ffi::c_void);

unsafe impl Send for AndroidWindow {}
unsafe impl Sync for AndroidWindow {}

pub struct Renderer {
    _instance: Option<()>,
    _adapter: Option<()>,
    pub device: Option<()>,
    pub queue: Option<()>,
    pub surface: Option<()>,
    pub config: Option<()>,
    _window: AndroidWindow,
}

impl Renderer {
    pub fn new(_env: &JNIEnv, _surface: JObject, _width: u32, _height: u32) -> Result<Self, String> {
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

    pub fn resize(&mut self, _width: u32, _height: u32) {}

    pub fn render(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {}
}
