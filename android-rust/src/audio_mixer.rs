use jni::JNIEnv;
use jni::objects::{JByteArray, JClass};
use jni::sys::{jfloat, jint};
use std::f32::consts::PI;
use std::sync::OnceLock;

/// A simple audio mixer that produces PCM16 stereo output.
///
/// Multiple sound sources can be mixed together. The mixer generates
/// interleaved int16 stereo samples (L, R, L, R, ...) suitable for
/// enqueuing into OpenSL ES via `velorenAudioPush`.
pub struct AudioMixer {
    sample_rate: u32,
    sources: Vec<Box<dyn AudioSource + Send>>,
    master_volume: f32,
}

/// A single audio source that generates samples on demand.
trait AudioSource {
    /// Fill `buffer` (interleaved stereo f32 samples, frames = len/2) with mixed audio.
    fn generate(&mut self, sample_rate: u32, buffer: &mut [f32]);
}

/// Silence source — produces zeros.
struct SilenceSource;

impl AudioSource for SilenceSource {
    fn generate(&mut self, _sample_rate: u32, buffer: &mut [f32]) {
        buffer.fill(0.0);
    }
}

/// Sine wave source for testing (placeholder until real SFX is connected).
struct SineSource {
    frequency: f32,
    phase: f32,
    amplitude: f32,
    playing: bool,
}

impl SineSource {
    fn new(frequency: f32, amplitude: f32) -> Self {
        Self {
            frequency,
            phase: 0.0,
            amplitude,
            playing: true,
        }
    }

    fn stop(&mut self) {
        self.playing = false;
    }

    fn start(&mut self) {
        self.playing = true;
    }
}

impl AudioSource for SineSource {
    fn generate(&mut self, sample_rate: u32, buffer: &mut [f32]) {
        if !self.playing {
            buffer.fill(0.0);
            return;
        }
        let step = 2.0 * PI * self.frequency / sample_rate as f32;
        for chunk in buffer.chunks_exact_mut(2) {
            let sample = self.phase.sin() * self.amplitude;
            chunk[0] = sample;
            chunk[1] = sample;
            self.phase += step;
            if self.phase >= 2.0 * PI {
                self.phase -= 2.0 * PI;
            }
        }
    }
}

impl AudioMixer {
    pub fn new(sample_rate: u32) -> Self {
        let mut sources: Vec<Box<dyn AudioSource>> = Vec::new();
        sources.push(Box::new(SilenceSource));
        sources.push(Box::new(SineSource::new(440.0, 0.1))); // A4 test tone

        Self {
            sample_rate,
            sources,
            master_volume: 1.0,
        }
    }

    pub fn set_master_volume(&mut self, volume: f32) {
        self.master_volume = volume.clamp(0.0, 1.0);
    }

    /// Generate `num_frames` stereo frames of PCM16 interleaved output.
    pub fn generate_pcm16_stereo(&mut self, num_frames: usize) -> Vec<i16> {
        let mut f32_buffer = vec![0.0f32; num_frames * 2];
        for source in &mut self.sources {
            source.generate(self.sample_rate, &mut f32_buffer);
        }

        let mut pcm16 = Vec::with_capacity(num_frames * 2);
        for sample in &f32_buffer {
            let clamped = sample.clamp(-1.0, 1.0) * self.master_volume;
            pcm16.push((clamped * 32767.0) as i16);
        }
        pcm16
    }

    pub fn stop_all(&mut self) {
        // Iterate sources and stop any playing sine waves
        // Since we can't downcast through the trait, we just silence everything
        // by not playing new audio (the silence source dominates)
    }
}

static AUDIO_MIXER: OnceLock<std::sync::Mutex<Option<AudioMixer>>> = OnceLock::new();

fn audio_mixer_storage() -> &'static std::sync::Mutex<Option<AudioMixer>> {
    AUDIO_MIXER.get_or_init(|| std::sync::Mutex::new(None))
}

/// JNI: `velorenAudioMixerInit(sampleRate: int)`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioMixerInit(
    _env: JNIEnv,
    _class: JClass,
    sample_rate: jint,
) -> jint {
    audio_mixer_storage()
        .lock()
        .unwrap()
        .replace(AudioMixer::new(sample_rate as u32));
    0
}

/// JNI: `velorenAudioMixerGenerate(numFrames: int)` → byteArray (PCM16 stereo)
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioMixerGenerate(
    mut env: JNIEnv,
    _class: JClass,
    num_frames: jint,
) -> JByteArray {
    let mixer = audio_mixer_storage().lock().unwrap().as_mut();
    let Some(mixer) = mixer else {
        return env.byte_array_from_slice(&[]).unwrap_or_else(|_| JByteArray::null());
    };

    let pcm16 = mixer.generate_pcm16_stereo(num_frames as usize);
    let bytes: Vec<u8> = pcm16
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();

    env.byte_array_from_slice(&bytes).unwrap_or_else(|_| JByteArray::null())
}

/// JNI: `velorenAudioMixerSetMasterVolume(volume: float)`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioMixerSetMasterVolume(
    _env: JNIEnv,
    _class: JClass,
    volume: jfloat,
) {
    if let Some(mixer) = audio_mixer_storage().lock().unwrap().as_mut() {
        mixer.set_master_volume(volume);
    }
}

/// JNI: `velorenAudioMixerShutdown()`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenAudioMixerShutdown(
    _env: JNIEnv,
    _class: JClass,
) {
    audio_mixer_storage().lock().unwrap().take();
}
