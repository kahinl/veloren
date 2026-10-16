use jni::JNIEnv;
use jni::objects::{JByteArray, JClass};
use std::sync::{Mutex, OnceLock};

/// OpenSL ES audio backend for Android.
/// Stub implementation — OpenSL ES symbols are not available in ndk-sys 0.6.
/// Audio is disabled until a proper binding is added.
pub struct Audio {
    _placeholder: Mutex<()>,
}

unsafe impl Send for Audio {}
unsafe impl Sync for Audio {}

impl Audio {
    pub fn new() -> Result<Self, String> {
        Err("OpenSL ES audio not available in ndk-sys 0.6".to_string())
    }

    pub fn set_callback(&mut self, _callback: AudioCallback) {}

    pub fn push_audio(&mut self, _data: &[i16]) -> Result<(), String> {
        Err("audio not available".to_string())
    }

    pub fn prime_queue(&mut self, _sample_count: usize) -> Result<(), String> {
        Err("audio not available".to_string())
    }
}

struct AudioCallback;

unsafe impl Send for AudioCallback {}
unsafe impl Sync for AudioCallback {}

impl AudioCallback {
    pub fn new(_object: jni::objects::JObject, _method_id: jni::sys::jmethodID) -> Self {
        Self
    }

    pub fn notify(&self) -> Result<(), jni::errors::Error> {
        Ok(())
    }
}

impl Drop for Audio {
    fn drop(&mut self) {}
}

/// Global audio instance, stored as Box<Audio> across JNI calls.
/// Box<Audio> is Send + Sync because Audio implements both.
static AUDIO: OnceLock<Mutex<Option<Box<Audio>>>> = OnceLock::new();

fn audio_storage() -> &'static Mutex<Option<Box<Audio>>> {
    AUDIO.get_or_init(|| Mutex::new(None))
}

/// JNI: `velorenAudioInit()`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioInit(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    match Audio::new() {
        Ok(audio) => {
            audio_storage()
                .lock()
                .unwrap()
                .replace(Box::new(audio));
            println!("OpenSL ES audio initialized (stub)");
            0
        }
        Err(e) => {
            eprintln!("Failed to initialize audio: {e}");
            -1
        }
    }
}

/// JNI: `velorenAudioPush(buffer: byteArray)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioPush(
    _env: JNIEnv,
    _class: JClass,
    _buffer: JByteArray,
) -> i32 {
    -1
}

/// JNI: `velorenAudioShutdown()`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioShutdown(
    _env: JNIEnv,
    _class: JClass,
) {
    audio_storage().lock().unwrap().take();
}

/// Silence the audio (enqueue silence buffers).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioMute(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    -1
}
