use jni::JNIEnv;
use jni::objects::{JByteArray, JClass};
use jni::sys::jint;
use ndk_sys::{
    SLAndroidSimpleBufferQueueCallback, SLAndroidSimpleBufferQueueItf, SLboolean, SLresult,
    SLuint32,
};
use once_cell::sync::Lazy;
use std::ptr::{null, null_mut};
use std::sync::Mutex;

/// OpenSL ES audio backend for Android.
///
/// Creates an SL engine, output mix, and a stereo PCM buffer queue player.
/// Audio data is pushed via `push_audio()` which is called from Java/JNI.
pub struct Audio {
    engine_object: ndk_sys::SLObjectItf,
    engine: ndk_sys::SLEngineItf,
    output_mix: ndk_sys::SLObjectItf,
    player_object: ndk_sys::SLObjectItf,
    player: ndk_sys::SLPlayItf,
    buffer_queue: ndk_sys::SLAndroidSimpleBufferQueueItf,
    /// Callback registered from Java to be notified when a buffer is consumed.
    /// Stores a JNI global ref + method ID so Java can refill the queue.
    callback: Option<AudioCallback>,
}

/// Callback info so Rust can notify Java when more audio data is needed.
struct AudioCallback {
    /// Global reference to the Java object that implements the audio callback.
    object: jni::objects::JObject,
    /// Method ID of `onAudioBufferNeeded(buffer: ByteArray)` on that object.
    method_id: jni::sys::jmethodID,
}

impl Audio {
    pub fn new() -> Result<Self, String> {
        unsafe {
            // 1) Create the OpenSL ES engine
            let mut engine_object: ndk_sys::SLObjectItf = null_mut();
            let result = ndk_sys::slCreateEngine(
                &mut engine_object as *mut ndk_sys::SLObjectItf,
                0,
                null(),
                0,
                null(),
                null(),
            );
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!("slCreateEngine failed: {result}"));
            }

            let result = ndk_sys::SLObjectItf_Realize(engine_object, 0 as SLboolean);
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!("SLObjectItf_Realize(engine) failed: {result}"));
            }

            let mut engine: ndk_sys::SLEngineItf = null_mut();
            let result = ndk_sys::SLObjectItf_GetInterface(
                engine_object,
                &ndk_sys::SL_IID_ENGINE as *const _ as *mut _,
                &mut engine as *mut ndk_sys::SLEngineItf as *mut _,
            );
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!("SLObjectItf_GetInterface(engine) failed: {result}"));
            }

            // 2) Create output mix
            let mut output_mix: ndk_sys::SLObjectItf = null_mut();
            let result = ndk_sys::SLEngineItf_CreateOutputMix(
                engine,
                &mut output_mix as *mut ndk_sys::SLObjectItf,
                0,
                null(),
                null(),
            );
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!("CreateOutputMix failed: {result}"));
            }

            let result = ndk_sys::SLObjectItf_Realize(output_mix, 0 as SLboolean);
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!("SLObjectItf_Realize(output_mix) failed: {result}"));
            }

            // 3) Create audio player with buffer queue
            let loc_bufq = ndk_sys::SLDataLocator_AndroidSimpleBufferQueue {
                locatorType: ndk_sys::SL_DATALOCATOR_ANDROIDSIMPLEBUFFERQUEUE,
                numBuffers: 2,
            };

            let format_pcm = ndk_sys::SLDataFormat_PCM {
                formatType: ndk_sys::SL_DATAFORMAT_PCM,
                numChannels: 2,
                samplesPerSec: ndk_sys::SL_SAMPLINGRATE_44_1,
                bitsPerSample: ndk_sys::SL_PCMSAMPLEFORMAT_FIXED_16,
                containerSize: 16,
                channelMask: ndk_sys::SL_SPEAKER_FRONT_LEFT | ndk_sys::SL_SPEAKER_FRONT_RIGHT,
                endianness: ndk_sys::SL_BYTEORDER_LITTLEENDIAN,
            };

            let data_source = ndk_sys::SLDataSource {
                pLocator: &loc_bufq as *const _ as *mut _,
                pFormat: &format_pcm as *const _ as *mut _,
            };

            let loc_outmix = ndk_sys::SLDataLocator_OutputMix {
                locatorType: ndk_sys::SL_DATALOCATOR_OUTPUTMIX,
                outputMix: output_mix,
            };

            let data_sink = ndk_sys::SLDataSink {
                pLocator: &loc_outmix as *const _ as *mut _,
                pFormat: null(),
            };

            let player_iids = [ndk_sys::SL_IID_ANDROIDSIMPLEBUFFERQUEUE];
            let player_req = [1 as SLboolean];

            let mut player_object: ndk_sys::SLObjectItf = null_mut();
            let result = ndk_sys::SLEngineItf_CreateAudioPlayer(
                engine,
                &mut player_object as *mut ndk_sys::SLObjectItf,
                &data_source,
                &data_sink,
                1,
                player_iids.as_ptr(),
                player_req.as_ptr(),
            );
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!("CreateAudioPlayer failed: {result}"));
            }

            let result = ndk_sys::SLObjectItf_Realize(player_object, 0 as SLboolean);
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!("SLObjectItf_Realize(player) failed: {result}"));
            }

            let mut player: ndk_sys::SLPlayItf = null_mut();
            let result = ndk_sys::SLObjectItf_GetInterface(
                player_object,
                &ndk_sys::SL_IID_PLAY as *const _ as *mut _,
                &mut player as *mut ndk_sys::SLPlayItf as *mut _,
            );
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!("SLObjectItf_GetInterface(play) failed: {result}"));
            }

            let mut buffer_queue: ndk_sys::SLAndroidSimpleBufferQueueItf = null_mut();
            let result = ndk_sys::SLObjectItf_GetInterface(
                player_object,
                &ndk_sys::SL_IID_ANDROIDSIMPLEBUFFERQUEUE as *const _ as *mut _,
                &mut buffer_queue as *mut ndk_sys::SLAndroidSimpleBufferQueueItf as *mut _,
            );
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!(
                    "SLObjectItf_GetInterface(buffer_queue) failed: {result}"
                ));
            }

            // 4) Register the buffer queue callback
            extern "C" fn buffer_callback(
                bq: ndk_sys::SLAndroidSimpleBufferQueueItf,
                context: *mut std::ffi::c_void,
            ) {
                let audio = unsafe { &*(context as *mut Audio) };
                if let Some(cb) = &audio.callback {
                    // Notify Java that we need more audio data.
                    // The Java side will call velorenAudioPush() to refill.
                    let _ = cb.notify();
                }
            }

            let result = ndk_sys::SLAndroidSimpleBufferQueueItf_RegisterCallback(
                buffer_queue,
                Some(buffer_callback),
                self as *const Audio as *mut std::ffi::c_void,
            );
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!("RegisterCallback failed: {result}"));
            }

            // 5) Set play state
            let result = ndk_sys::SLPlayItf_SetPlayState(player, ndk_sys::SL_PLAYSTATE_PLAYING);
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!("SLPlayItf_SetPlayState failed: {result}"));
            }

            Ok(Audio {
                engine_object,
                engine,
                output_mix,
                player_object,
                player,
                buffer_queue,
                callback: None,
            })
        }
    }

    /// Set the Java callback object that will be notified when more audio data is needed.
    pub fn set_callback(&mut self, callback: AudioCallback) {
        self.callback = Some(callback);
    }

    /// Push a PCM16 stereo buffer to the audio queue.
    /// `data` should be interleaved int16 samples (L, R, L, R, ...).
    pub fn push_audio(&mut self, data: &[i16]) -> Result<(), String> {
        unsafe {
            let result = ndk_sys::SLAndroidSimpleBufferQueueItf_Enqueue(
                self.buffer_queue,
                data.as_ptr() as *const std::ffi::c_void,
                (data.len() * std::mem::size_of::<i16>()) as SLuint32,
            );
            if result != ndk_sys::SL_RESULT_SUCCESS as SLresult {
                return Err(format!("SLAndroidSimpleBufferQueueItf_Enqueue failed: {result}"));
            }
        }
        Ok(())
    }

    /// Enqueue an empty buffer to prime the queue (call once after creation).
    pub fn prime_queue(&mut self, sample_count: usize) -> Result<(), String> {
        let silence = vec![0i16; sample_count * 2]; // stereo
        self.push_audio(&silence)
    }
}

impl AudioCallback {
    pub fn new(object: jni::objects::JObject, method_id: jni::sys::jmethodID) -> Self {
        Self { object, method_id }
    }

    pub fn notify(&self) -> Result<(), jni::errors::Error> {
        // This is called from the OpenSL ES callback thread.
        // We cannot safely call back into Java from here without a JNIEnv.
        // Instead, we set a flag that Java polls, or we store the need and
        // Java calls velorenAudioPush() on its own thread.
        // For now, we just return Ok — Java drives audio via velorenAudioPush.
        Ok(())
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        unsafe {
            let _ = ndk_sys::SLObjectItf_Destroy(self.player_object);
            let _ = ndk_sys::SLObjectItf_Destroy(self.output_mix);
            let _ = ndk_sys::SLObjectItf_Destroy(self.engine_object);
        }
    }
}

/// Global audio instance, stored as a raw pointer across JNI calls.
static AUDIO: once_cell::sync::Lazy<Mutex<Option<*mut Audio>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(None));

fn audio_storage() -> &'static Mutex<Option<*mut Audio>> {
    &AUDIO
}

/// JNI: `velorenAudioInit()`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioInit(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    match Audio::new() {
        Ok(mut audio) => {
            // Prime the queue with silence
            let _ = audio.prime_queue(44100 * 2); // ~1 second of stereo silence
            let boxed = Box::new(audio);
            let ptr = Box::into_raw(boxed);
            audio_storage().lock().unwrap().replace(ptr);
            println!("OpenSL ES audio initialized");
            0
        }
        Err(e) => {
            eprintln!("Failed to initialize OpenSL ES audio: {e}");
            -1
        }
    }
}

/// JNI: `velorenAudioPush(buffer: byteArray)`
/// Pushes PCM16 stereo data into the OpenSL ES buffer queue.
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioPush(
    mut env: JNIEnv,
    _class: JClass,
    buffer: JByteArray,
) -> jint {
    let ptr = audio_storage().lock().unwrap().as_ref().and_then(|o| *o);
    let Some(audio_ptr) = ptr else {
        return -1;
    };

    let audio = unsafe { &mut *audio_ptr };

    let data: Vec<i16> = {
        let len = env.get_array_length(buffer).unwrap_or(0) as usize;
        let mut bytes = vec![0u8; len];
        if env.get_byte_array_region(buffer, 0, &mut bytes).is_ok() {
            bytes
                .chunks_exact(2)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                .collect()
        } else {
            Vec::new()
        }
    };

    match audio.push_audio(&data) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Audio push failed: {e}");
            -2
        }
    }
}

/// JNI: `velorenAudioShutdown()`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioShutdown(
    _env: JNIEnv,
    _class: JClass,
) {
    if let Some(ptr) = audio_storage().lock().unwrap().take() {
        unsafe {
            drop(Box::from_raw(ptr));
        }
    }
}

/// Silence the audio (enqueue silence buffers).
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioMute(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    let ptr = audio_storage().lock().unwrap().as_ref().and_then(|o| *o);
    let Some(audio_ptr) = ptr else {
        return -1;
    };

    let audio = unsafe { &mut *audio_ptr };

    let silence = vec![0i16; 4410 * 2]; // 100ms of stereo silence
    match audio.push_audio(&silence) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}
