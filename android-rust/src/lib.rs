use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JObject, JString};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

#[cfg(target_os = "android")]
mod android_assets;
#[cfg(target_os = "android")]
mod audio;
#[cfg(target_os = "android")]
mod audio_mixer;
#[cfg(target_os = "android")]
mod events;
#[cfg(target_os = "android")]
mod renderer;
#[cfg(target_os = "android")]
mod voxygen_bridge;

use veloren_client::Client;
use veloren_client::{ClientInitStage, CredentialProvider, Credentials, DispatcherBuilder, WorldExt};
use veloren_client::ClientType;
use veloren_client::addr::ConnectionArgs;
use veloren_common::comp::controller::{ControllerInputs, InputKind};
use veloren_common::comp::Controller;
use tokio::runtime::Runtime;

/// The Veloren client instance, stored as i64 (i64 raw pointer) in static storage.
static CLIENT: OnceLock<Mutex<Option<i64>>> = OnceLock::new();
static RUNTIME: OnceLock<Mutex<Option<Arc<Runtime>>>> = OnceLock::new();
static LAST_TICK: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

/// The wgpu renderer instance, stored as i64 raw pointer.
static RENDERER: OnceLock<Mutex<Option<i64>>> = OnceLock::new();

/// The Android AAssetManager, stored as raw pointer.
#[cfg(target_os = "android")]
static ASSET_MANAGER: OnceLock<Mutex<Option<i64>>> = OnceLock::new();

/// The Android asset cache.
#[cfg(target_os = "android")]
static ASSET_CACHE: OnceLock<Mutex<Option<assets_manager::AssetCache>>> = OnceLock::new();

fn client_storage() -> &'static Mutex<Option<i64>> {
    CLIENT.get_or_init(|| Mutex::new(None))
}

fn runtime_storage() -> &'static Mutex<Option<Arc<Runtime>>> {
    RUNTIME.get_or_init(|| Mutex::new(None))
}

fn last_tick_storage() -> &'static Mutex<Option<Instant>> {
    LAST_TICK.get_or_init(|| Mutex::new(None))
}

fn renderer_storage() -> &'static Mutex<Option<i64>> {
    RENDERER.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "android")]
fn asset_manager_storage() -> &'static Mutex<Option<i64>> {
    ASSET_MANAGER.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "android")]
fn asset_cache_storage() -> &'static Mutex<Option<assets_manager::AssetCache>> {
    ASSET_CACHE.get_or_init(|| Mutex::new(None))
}

/// `Client::new`'in `&(dyn Fn(ClientInitStage) + Send + Sync)` parametresi
/// bir isimli fonksiyon işaretçisiyle (fn pointer) sağlanıyor; closure
/// doğrudan bu türe coerce olmuyor.
fn stage_callback(_stage: ClientInitStage) {}
fn builder_callback(_builder: &mut DispatcherBuilder) {}

/// Initialize the Veloren client and connect to a server.
/// Called from Java: `velorenInit(serverAddress: String)`
/// Returns: i64 (raw pointer to Box<Client> cast to i64)
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenInit(
    mut env: JNIEnv,
    _class: JClass,
    server_address: JString,
) -> i64 {
    let server_addr: String = env
        .get_string(&server_address)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "veloren.club:3300".to_string());

    let runtime = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Failed to create tokio runtime: {}", e);
            return 0;
        }
    };

    let rt = std::sync::Arc::new(runtime);
    *runtime_storage().lock().unwrap() = Some(std::sync::Arc::clone(&rt));

    let addr = ConnectionArgs::Tcp {
        hostname: server_addr.clone(),
        prefer_ipv6: false,
    };

    let stage_callback: &(dyn Fn(ClientInitStage) + Send + Sync) = &|_: ClientInitStage| {};

    let client = rt.block_on(Client::new(
        addr,
        std::sync::Arc::clone(&rt),
        None,
        &StaticCredentialProvider,
        stage_callback,
        builder_callback,
        PathBuf::default(),
        ClientType::Game,
    ));

    match client {
        Ok(c) => {
            let boxed = Box::new(c);
            let ptr = Box::into_raw(boxed) as *mut Client as i64;
            *client_storage().lock().unwrap() = Some(ptr);
            *last_tick_storage().lock().unwrap() = Some(Instant::now());

            println!("Veloren client initialized, connected to {}", server_addr);
            ptr
        }
        Err(e) => {
            eprintln!("Veloren client init failed: {:?}", e);
            0
        }
    }
}

/// Run one tick of the Veloren client.
/// Called from Java each frame: `velorenTick(clientPtr, dt)`
/// Returns: i32 (0 on success, -1 on error)
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenTick(
    mut env: JNIEnv,
    _class: JClass,
    dt: f32,
) -> i32 {
    let client_ptr = client_storage().lock().unwrap().as_ref().copied();
    let Some(client_ptr) = client_ptr else {
        return -1;
    };

    let mut last_tick = last_tick_storage().lock().unwrap();
    let now = Instant::now();
    let elapsed = last_tick
        .map(|t| now.duration_since(t))
        .unwrap_or(std::time::Duration::from_secs_f64(dt as f64));
    *last_tick = Some(now);
    drop(last_tick);

    let inputs = ControllerInputs::default();

    let client = unsafe { &mut *(client_ptr as *mut Client) };
    match client.tick(inputs, elapsed) {
        Ok(events) => {
            if !events.is_empty() {
                events::forward_events(&mut env, &events);
            }
            0
        }
        Err(e) => {
            eprintln!("Veloren tick error: {:?}", e);
            -3
        }
    }
}

/// Create the wgpu renderer and bind it to the Android Surface.
/// Called from Java: `velorenCreateRenderer(surface, width, height)`
/// Returns: i64 (raw pointer to Box<Renderer> cast to i64)
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenCreateRenderer(
    mut env: JNIEnv,
    _class: JClass,
    surface: JObject,
    width: i32,
    height: i32,
) -> i64 {
    let renderer = match renderer::Renderer::new(&env, surface, width as u32, height as u32) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to create renderer: {}", e);
            return 0;
        }
    };

    let boxed = Box::new(renderer);
    let ptr = Box::into_raw(boxed) as *mut renderer::Renderer as i64;
    *renderer_storage().lock().unwrap() = Some(ptr);
    ptr
}

/// Render one frame.
/// Called from Java each frame: `velorenRenderFrame(clientPtr)`
/// Returns: i32 (0 on success, -1 on error)
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenRenderFrame(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    let renderer_ptr = renderer_storage().lock().unwrap().as_ref().copied();
    let Some(renderer_ptr) = renderer_ptr else {
        return -1;
    };

    let renderer = unsafe { &mut *(renderer_ptr as *mut renderer::Renderer) };
    match renderer.render() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Render frame error: {:?}", e);
            -3
        }
    }
}

/// Resize the renderer surface.
/// Called from Java: `velorenResize(clientPtr, width, height)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenResize(
    _env: JNIEnv,
    _class: JClass,
    width: i32,
    height: i32,
) {
    let mut renderer_guard = renderer_storage().lock().unwrap();
    let Some(renderer_ptr) = renderer_guard.as_mut() else {
        return;
    };

    let renderer = unsafe { &mut *(*renderer_ptr as *mut renderer::Renderer) };
    renderer.resize(width as u32, height as u32);
}

/// Set the Android AAssetManager for loading assets from the APK.
/// Called from Java: `velorenSetAssetManager(assetManager)`
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetAssetManager(
    mut env: JNIEnv,
    _class: JClass,
    asset_manager: JObject,
) -> i32 {
    // Stub: ndk-sys 0.6 does not export AAssetManager_fromJava directly.
    // Real implementation needs JNI call to get ANativeWindow/AAssetManager from Java Surface.
    let mgr_ptr: *mut ndk_sys::AAssetManager = std::ptr::null_mut();
    let _ = env;
    let _ = asset_manager;

    if mgr_ptr.is_null() {
        eprintln!("Failed to get AAssetManager from Java AssetManager (stub)");
        return -1;
    }

    *asset_manager_storage().lock().unwrap() = Some(mgr_ptr as i64);

    let android_source = match android_assets::AndroidAssets::from_raw(mgr_ptr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to create Android asset source: {e}");
            return -2;
        }
    };

    let cache = assets_manager::AssetCache::with_source(android_source);
    *asset_cache_storage().lock().unwrap() = Some(cache);

    println!("Android AAssetManager set and asset cache initialized");
    0
}

/// Load an asset from the APK and return its bytes as a jbyteArray.
/// Called from Java: `velorenLoadAsset(path: String)`
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenLoadAsset<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass,
    path: JString,
) -> JObject<'a> {
    let path_str: String = env
        .get_string(&path)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|_| String::new());

    let (id, ext) = match path_str.rfind('.') {
        Some(dot) => (&path_str[..dot], &path_str[dot + 1..]),
        None => (path_str.as_str(), ""),
    };

    let cache = asset_cache_storage()
        .lock()
        .unwrap()
        .as_ref()
        .cloned();

    let Some(cache) = cache else {
        return env
            .byte_array_from_slice(&[])
            .map(|arr| arr.into())
            .unwrap_or_else(|_| JObject::null());
    };

    match cache.load_owned::<Box<str>>(format!("{id}.{ext}").as_str()) {
        Ok(bytes) => match env.byte_array_from_slice(bytes.as_bytes()) {
            Ok(arr) => arr.into(),
            Err(_) => JObject::null(),
        },
        Err(_) => JObject::null(),
    }
}

/// Shutdown the Veloren client and renderer.
/// Called from Java: `velorenShutdown(clientPtr)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenShutdown(
    _env: JNIEnv,
    _class: JClass,
) {
    if let Some(ptr) = client_storage().lock().unwrap().take() {
        unsafe {
            drop(Box::from_raw(ptr as *mut Client));
        }
    }
    *client_storage().lock().unwrap() = None;
    if let Some(renderer_ptr) = renderer_storage().lock().unwrap().take() {
        unsafe {
            drop(Box::from_raw(renderer_ptr as *mut renderer::Renderer));
        }
    }
    *runtime_storage().lock().unwrap() = None;
    *last_tick_storage().lock().unwrap() = None;

    #[cfg(target_os = "android")]
    {
        *asset_manager_storage().lock().unwrap() = None;
        *asset_cache_storage().lock().unwrap() = None;
    }
}

/// Set controller inputs from Android touch/joystick.
/// Called from Java: `velorenSetInput(clientPtr, moveX, moveY, moveZ, action, pressed)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetInput(
    _env: JNIEnv,
    _class: JClass,
    move_x: f32,
    move_y: f32,
    move_z: f32,
    action: i32,
    pressed: u8,
) {
    let client_guard = client_storage().lock().unwrap();
    let Some(client_ptr) = client_guard.as_ref().copied() else {
        return;
    };

    let input_kind = match action {
        0 => InputKind::Primary,
        1 => InputKind::Secondary,
        2 => InputKind::Block,
        3 => InputKind::Jump,
        4 => {
            // look delta handled separately
            return;
        }
        _ => InputKind::Primary,
    };

    let client = unsafe { &mut *(client_ptr as *mut Client) };
    let entity = client.entity();
    client.handle_input(input_kind, pressed != 0, None, None);

    if let Some(controller) = client
        .state_mut()
        .ecs_mut()
        .write_storage::<Controller>()
        .get_mut(entity)
    {
        controller.inputs.move_dir = vek::Vec2::new(move_x, move_y);
        controller.inputs.move_z = move_z;
    }
}

/// Get the current tick count (for debugging).
/// Called from Java: `velorenGetTick(clientPtr)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetTick(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    let client_guard = client_storage().lock().unwrap();
    match client_guard.as_ref() {
        Some(ptr) => {
            let client = unsafe { &mut *(*ptr as *mut Client) };
            client.get_tick() as i32
        }
        None => -1,
    }
}

/// Set mobile performance tier for Android optimizations.
/// Called from Java: `velorenSetMobilePerfTier(clientPtr, tier: int)`
/// tier: 0=None, 1=Low, 2=Medium
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetMobilePerfTier(
    _env: JNIEnv,
    _class: JClass,
    tier: i32,
) {
    let mut client_guard = client_storage().lock().unwrap();
    let Some(ptr) = client_guard.as_mut() else {
        return;
    };
    let client = unsafe { &mut *(*ptr as *mut Client) };

    let perf_tier = match tier {
        1 => veloren_client::MobilePerfTier::Low,
        2 => veloren_client::MobilePerfTier::Medium,
        _ => veloren_client::MobilePerfTier::None,
    };

    client.set_mobile_perf_tier(perf_tier);
}

/// Set weather enabled (rain/snow).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetWeatherEnabled(
    _env: JNIEnv,
    _class: JClass,
    enabled: u8,
) {
    static WEATHER_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    *WEATHER_ENABLED.get().unwrap().lock().unwrap() = enabled != 0;
}

/// Get weather enabled state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetWeatherEnabled(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    static WEATHER_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    if *WEATHER_ENABLED.get().unwrap().lock().unwrap() {
        1
    } else {
        0
    }
}

/// Set clouds enabled.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetCloudsEnabled(
    _env: JNIEnv,
    _class: JClass,
    enabled: u8,
) {
    static CLOUDS_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    *CLOUDS_ENABLED.get().unwrap().lock().unwrap() = enabled != 0;
}

/// Get clouds enabled state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetCloudsEnabled(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    static CLOUDS_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    if *CLOUDS_ENABLED.get().unwrap().lock().unwrap() {
        1
    } else {
        0
    }
}

/// Set particles enabled.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetParticlesEnabled(
    _env: JNIEnv,
    _class: JClass,
    enabled: u8,
) {
    static PARTICLES_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    *PARTICLES_ENABLED.get().unwrap().lock().unwrap() = enabled != 0;
}

/// Get particles enabled state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetParticlesEnabled(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    static PARTICLES_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    if *PARTICLES_ENABLED.get().unwrap().lock().unwrap() {
        1
    } else {
        0
    }
}

/// Initialize shader cache on first launch to avoid stuttering.
/// Called from Java: `velorenInitShaderCache()`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenInitShaderCache(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    println!("Shader cache initialization (placeholder)");
    0
}

/// Set Rayon thread count for big.LITTLE CPU architecture.
/// Called from Java: `velorenSetThreadCount(count: int)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetThreadCount(
    _env: JNIEnv,
    _class: JClass,
    count: i32,
) {
    static THREAD_COUNT: OnceLock<Mutex<usize>> = OnceLock::new();
    *THREAD_COUNT.get().unwrap().lock().unwrap() = count.max(1) as usize;
    println!("Rayon thread count set to {}", count);
}

/// Get configured thread count.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetThreadCount(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    static THREAD_COUNT: OnceLock<Mutex<usize>> = OnceLock::new();
    *THREAD_COUNT.get().unwrap().lock().unwrap() as i32
}

/// Enable/disable on-demand asset loading.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetOnDemandAssets(
    _env: JNIEnv,
    _class: JClass,
    enabled: u8,
) {
    static ON_DEMAND_ASSETS: OnceLock<Mutex<bool>> = OnceLock::new();
    *ON_DEMAND_ASSETS.get().unwrap().lock().unwrap() = enabled != 0;
}

/// Get on-demand asset loading state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetOnDemandAssets(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    static ON_DEMAND_ASSETS: OnceLock<Mutex<bool>> = OnceLock::new();
    if *ON_DEMAND_ASSETS.get().unwrap().lock().unwrap() {
        1
    } else {
        0
    }
}

/// Set texture compression format for mobile (ASTC preferred).
/// Called from Java: `velorenSetTextureCompression(format: int)`
/// format: 0=Default, 1=ASTC, 2=ETC2
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetTextureCompression(
    _env: JNIEnv,
    _class: JClass,
    format: i32,
) {
    static TEXTURE_COMPRESSION: OnceLock<Mutex<i32>> = OnceLock::new();
    *TEXTURE_COMPRESSION.get().unwrap().lock().unwrap() = format;
}

/// Get texture compression format.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetTextureCompression(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    static TEXTURE_COMPRESSION: OnceLock<Mutex<i32>> = OnceLock::new();
    *TEXTURE_COMPRESSION.get().unwrap().lock().unwrap()
}

/// Set maximum FPS limit for battery/heat saving.
/// Called from Java: `velorenSetMaxFps(fps: int)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetMaxFps(
    _env: JNIEnv,
    _class: JClass,
    fps: i32,
) {
    static MAX_FPS: OnceLock<Mutex<u32>> = OnceLock::new();
    *MAX_FPS.get().unwrap().lock().unwrap() = fps.max(15).min(120) as u32;
}

/// Get maximum FPS limit.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetMaxFps(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    static MAX_FPS: OnceLock<Mutex<u32>> = OnceLock::new();
    *MAX_FPS.get().unwrap().lock().unwrap() as i32
}

/// Simple credential provider for Android (no auth).
struct StaticCredentialProvider;

impl CredentialProvider for StaticCredentialProvider {
    fn auth_trusted(&self, _auth_provider: &Option<String>) -> bool {
        true
    }

    fn get_credentials(
        &self,
        _auth_provider: &Option<String>,
    ) -> impl std::future::Future<Output = Result<Credentials, ()>> + Send {
        async { Ok(Credentials::Username("android".to_string())) }
    }
}

/// The look delta (camera rotation) from touch input.
static LOOK_DELTA: OnceLock<Mutex<Option<vek::Vec2<f32>>>> = OnceLock::new();

fn look_delta_storage() -> &'static Mutex<Option<vek::Vec2<f32>>> {
    LOOK_DELTA.get_or_init(|| Mutex::new(None))
}
