use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JObject, JString};
use jni::sys::{jfloat, jint, jlong};
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
use veloren_client::init::{CredentialProvider, Credentials};
use veloren_client::ClientType;
use veloren_client::addr::ConnectionArgs;
use veloren_common::comp::controller::{ControllerInputs, InputKind};
use veloren_common::comp::Controller;
use tokio::runtime::Runtime;

/// The Veloren client instance, stored as jlong (i64 raw pointer) in static storage.
/// jlong is i64 which is always Send + Sync.
static CLIENT: OnceLock<Mutex<Option<jlong>>> = OnceLock::new();
static RUNTIME: OnceLock<Mutex<Option<Runtime>>> = OnceLock::new();
static LAST_TICK: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

/// The wgpu renderer instance, stored as jlong raw pointer.
static RENDERER: OnceLock<Mutex<Option<jlong>>> = OnceLock::new();

/// The Android AAssetManager, stored as raw pointer.
#[cfg(target_os = "android")]
static ASSET_MANAGER: OnceLock<Mutex<Option<*mut ndk_sys::AAssetManager>>> = OnceLock::new();

/// The Android asset cache.
#[cfg(target_os = "android")]
static ASSET_CACHE: OnceLock<Mutex<Option<assets_manager::AssetCache>>> = OnceLock::new();

fn client_storage() -> &'static Mutex<Option<jlong>> {
    CLIENT.get_or_init(|| Mutex::new(None))
}

fn runtime_storage() -> &'static Mutex<Option<Runtime>> {
    RUNTIME.get_or_init(|| Mutex::new(None))
}

fn last_tick_storage() -> &'static Mutex<Option<Instant>> {
    LAST_TICK.get_or_init(|| Mutex::new(None))
}

fn renderer_storage() -> &'static Mutex<Option<jlong>> {
    RENDERER.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "android")]
fn asset_manager_storage() -> &'static Mutex<Option<jlong>> {
    ASSET_MANAGER.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "android")]
fn asset_cache_storage() -> &'static Mutex<Option<assets_manager::AssetCache>> {
    ASSET_CACHE.get_or_init(|| Mutex::new(None))
}

/// Initialize the Veloren client and connect to a server.
/// Called from Java: `velorenInit(serverAddress: String)`
/// Returns: jlong (raw pointer to Box<Client> cast to jlong)
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenInit(
    mut env: JNIEnv,
    _class: JClass,
    server_address: JString,
) -> jlong {
    let server_addr: String = env
        .get_string(server_address)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "veloren.club:3300".to_string());

    let runtime = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Failed to create tokio runtime: {}", e);
            return 0;
        }
    };

    let client_result = runtime.block_on(async {
        let addr = ConnectionArgs::Tcp {
            hostname: server_addr.clone(),
            prefer_ipv6: false,
        };
        let rt = Arc::new(runtime);
        let provider = StaticCredentialProvider;
        Client::new(
            addr,
            rt,
            None,
            &provider,
            &|_stage| {},
            &|_builder| {},
            PathBuf::from("/data/data/com.veloren.client/files"),
            ClientType::Game,
        )
        .await
    });

    match client_result {
        Ok(mut client) => {
            let boxed = Box::new(client);
            let ptr = Box::into_raw(boxed) as *mut Client as jlong;
            client_storage().lock().unwrap().replace(Some(ptr));
            runtime_storage().lock().unwrap().replace(runtime);
            last_tick_storage().lock().unwrap().replace(Instant::now());

            println!("Veloren client initialized, connected to {}", server_addr);
            ptr
        }
        Err(e) => {
            eprintln!("Failed to create Veloren client: {:?}", e);
            0
        }
    }
}

/// Run one tick of the Veloren client.
/// Called from Java each frame: `velorenTick(clientPtr, dt)`
/// Returns: jint (0 on success, -1 on error)
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenTick(
    mut env: JNIEnv,
    _class: JClass,
    client_ptr: jlong,
    dt: jfloat,
) -> jint {
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
/// Returns: jlong (raw pointer to Box<Renderer> cast to jlong)
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenCreateRenderer(
    mut env: JNIEnv,
    _class: JClass,
    surface: JObject,
    width: jint,
    height: jint,
) -> jlong {
    let renderer = match renderer::Renderer::new(&env, surface, width as u32, height as u32) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to create renderer: {}", e);
            return 0;
        }
    };

    let boxed = Box::new(renderer);
    let ptr = Box::into_raw(boxed) as *mut renderer::Renderer as jlong;
    renderer_storage().lock().unwrap().replace(Some(ptr));
    ptr
}

/// Render one frame.
/// Called from Java each frame: `velorenRenderFrame(clientPtr)`
/// Returns: jint (0 on success, -1 on error)
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenRenderFrame(
    _env: JNIEnv,
    _class: JClass,
    client_ptr: jlong,
) -> jint {
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
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenResize(
    _env: JNIEnv,
    _class: JClass,
    client_ptr: jlong,
    width: jint,
    height: jint,
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
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetAssetManager(
    mut env: JNIEnv,
    _class: JClass,
    asset_manager: JObject,
) -> jint {
    let mgr_ptr = unsafe {
        ndk_sys::AAssetManager_fromJava(
            env.get_raw_jnienv() as *mut _,
            asset_manager.into_raw() as *mut _,
        )
    };

    if mgr_ptr.is_null() {
        eprintln!("Failed to get AAssetManager from Java AssetManager");
        return -1;
    }

    asset_manager_storage().lock().unwrap().replace(mgr_ptr as jlong);

    let android_source = match android_assets::AndroidAssets::from_raw(mgr_ptr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to create Android asset source: {e}");
            return -2;
        }
    };

    let cache = assets_manager::AssetCache::with_source(android_source);
    asset_cache_storage().lock().unwrap().replace(cache);

    println!("Android AAssetManager set and asset cache initialized");
    0
}

/// Load an asset from the APK and return its bytes as a jbyteArray.
/// Called from Java: `velorenLoadAsset(path: String)`
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenLoadAsset(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> JObject {
    let path_str: String = env
        .get_string(path)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (id, ext) = match path_str.rfind('.') {
        Some(dot) => (&path_str[..dot], &path_str[dot + 1..]),
        None => (&path_str, ""),
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

    match cache.load_owned::<Vec<u8>>(format!("{id}.{ext}").as_str()) {
        Ok(bytes) => match env.byte_array_from_slice(&bytes) {
            Ok(arr) => arr.into(),
            Err(_) => JObject::null(),
        },
        Err(_) => JObject::null(),
    }
}

/// Shutdown the Veloren client and renderer.
/// Called from Java: `velorenShutdown(clientPtr)`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenShutdown(
    _env: JNIEnv,
    _class: JClass,
    client_ptr: jlong,
) {
    if client_ptr != 0 {
        unsafe {
            drop(Box::from_raw(client_ptr as *mut Client));
        }
    }
    client_storage().lock().unwrap().take();
    if let Some(renderer_ptr) = renderer_storage().lock().unwrap().take() {
        unsafe {
            drop(Box::from_raw(renderer_ptr as *mut renderer::Renderer));
        }
    }
    runtime_storage().lock().unwrap().take();
    last_tick_storage().lock().unwrap().take();

    #[cfg(target_os = "android")]
    {
        asset_manager_storage().lock().unwrap().take();
        asset_cache_storage().lock().unwrap().take();
    }
}

/// Set controller inputs from Android touch/joystick.
/// Called from Java: `velorenSetInput(clientPtr, moveX, moveY, moveZ, action, pressed)`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetInput(
    _env: JNIEnv,
    _class: JClass,
    client_ptr: jlong,
    move_x: jfloat,
    move_y: jfloat,
    move_z: jfloat,
    action: jint,
    pressed: jboolean,
) {
    let mut client_guard = client_storage().lock().unwrap();
    let Some(client_ptr) = client_guard.as_ref().copied() else {
        return;
    };

    let input_kind = match action {
        0 => InputKind::Primary,
        1 => InputKind::Secondary,
        2 => InputKind::Block,
        3 => InputKind::Jump,
        4 => {
            let mut look = look_delta_storage().lock().unwrap();
            *look = Some(vek::Vec2::new(move_x, move_y));
            return;
        }
        _ => InputKind::Primary,
    };

    let client = unsafe { &mut *(client_ptr as *mut Client) };
    client.handle_input(input_kind, pressed != 0, None, None);

    if let Some(controller) = client
        .state_mut()
        .ecs_mut()
        .write_storage::<Controller>()
        .get_mut(client.entity())
    {
        controller.inputs.move_dir = vek::Vec2::new(move_x, move_y);
        controller.inputs.move_z = move_z;
    }
}

/// Get the current tick count (for debugging).
/// Called from Java: `velorenGetTick(clientPtr)`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetTick(
    _env: JNIEnv,
    _class: JClass,
    client_ptr: jlong,
) -> jint {
    let client_guard = client_storage().lock().unwrap();
    match client_guard.as_ref() {
        Some(client_ptr) => {
            let client = unsafe { &mut *(*client_ptr as *mut Client) };
            client.get_tick() as jint
        }
        None => -1,
    }
}

/// Set mobile performance tier for Android optimizations.
/// Called from Java: `velorenSetMobilePerfTier(clientPtr, tier: int)`
/// tier: 0=None, 1=Low, 2=Medium
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetMobilePerfTier(
    _env: JNIEnv,
    _class: JClass,
    client_ptr: jlong,
    tier: jint,
) {
    let mut client_guard = client_storage().lock().unwrap();
    let Some(client) = client_guard.as_mut() else {
        return;
    };

    let perf_tier = match tier {
        1 => veloren_client::MobilePerfTier::Low,
        2 => veloren_client::MobilePerfTier::Medium,
        _ => veloren_client::MobilePerfTier::None,
    };

    client.set_mobile_perf_tier(perf_tier);
}

/// Set weather enabled (rain/snow).
/// Called from Java: `velorenSetWeatherEnabled(enabled: boolean)`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetWeatherEnabled(
    _env: JNIEnv,
    _class: JClass,
    enabled: jboolean,
) {
    static WEATHER_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    *WEATHER_ENABLED.lock().unwrap() = enabled != 0;
}

/// Get weather enabled state.
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetWeatherEnabled(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    static WEATHER_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    if *WEATHER_ENABLED.lock().unwrap() { 1 } else { 0 }
}

/// Set clouds enabled.
/// Called from Java: `velorenSetCloudsEnabled(enabled: boolean)`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetCloudsEnabled(
    _env: JNIEnv,
    _class: JClass,
    enabled: jboolean,
) {
    static CLOUDS_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    *CLOUDS_ENABLED.lock().unwrap() = enabled != 0;
}

/// Get clouds enabled state.
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetCloudsEnabled(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    static CLOUDS_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    if *CLOUDS_ENABLED.lock().unwrap() { 1 } else { 0 }
}

/// Set particles enabled.
/// Called from Java: `velorenSetParticlesEnabled(enabled: boolean)`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetParticlesEnabled(
    _env: JNIEnv,
    _class: JClass,
    enabled: jboolean,
) {
    static PARTICLES_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    *PARTICLES_ENABLED.lock().unwrap() = enabled != 0;
}

/// Get particles enabled state.
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetParticlesEnabled(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    static PARTICLES_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
    if *PARTICLES_ENABLED.lock().unwrap() { 1 } else { 0 }
}

/// Initialize shader cache on first launch to avoid stuttering.
/// Called from Java: `velorenInitShaderCache()`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenInitShaderCache(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    println!("Shader cache initialization (placeholder)");
    0
}

/// Set Rayon thread count for big.LITTLE CPU architecture.
/// Called from Java: `velorenSetThreadCount(count: int)`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetThreadCount(
    _env: JNIEnv,
    _class: JClass,
    count: jint,
) {
    static THREAD_COUNT: OnceLock<Mutex<usize>> = OnceLock::new();
    *THREAD_COUNT.lock().unwrap() = count.max(1) as usize;
    println!("Rayon thread count set to {}", count);
}

/// Get configured thread count.
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetThreadCount(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    static THREAD_COUNT: OnceLock<Mutex<usize>> = OnceLock::new();
    *THREAD_COUNT.lock().unwrap() as jint
}

/// Enable/disable on-demand asset loading.
/// Called from Java: `velorenSetOnDemandAssets(enabled: boolean)`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetOnDemandAssets(
    _env: JNIEnv,
    _class: JClass,
    enabled: jboolean,
) {
    static ON_DEMAND_ASSETS: OnceLock<Mutex<bool>> = OnceLock::new();
    *ON_DEMAND_ASSETS.lock().unwrap() = enabled != 0;
}

/// Get on-demand asset loading state.
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetOnDemandAssets(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    static ON_DEMAND_ASSETS: OnceLock<Mutex<bool>> = OnceLock::new();
    if *ON_DEMAND_ASSETS.lock().unwrap() { 1 } else { 0 }
}

/// Set texture compression format for mobile (ASTC preferred).
/// Called from Java: `velorenSetTextureCompression(format: int)`
/// format: 0=Default, 1=ASTC, 2=ETC2
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetTextureCompression(
    _env: JNIEnv,
    _class: JClass,
    format: jint,
) {
    static TEXTURE_COMPRESSION: OnceLock<Mutex<i32>> = OnceLock::new();
    *TEXTURE_COMPRESSION.lock().unwrap() = format;
}

/// Get texture compression format.
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetTextureCompression(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    static TEXTURE_COMPRESSION: OnceLock<Mutex<i32>> = OnceLock::new();
    *TEXTURE_COMPRESSION.lock().unwrap()
}

/// Set maximum FPS limit for battery/heat saving.
/// Called from Java: `velorenSetMaxFps(fps: int)`
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetMaxFps(
    _env: JNIEnv,
    _class: JClass,
    fps: jint,
) {
    static MAX_FPS: OnceLock<Mutex<u32>> = OnceLock::new();
    *MAX_FPS.lock().unwrap() = fps.max(15).min(120);
}

/// Get maximum FPS limit.
#[no_mangle]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetMaxFps(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    static MAX_FPS: OnceLock<Mutex<u32>> = OnceLock::new();
    *MAX_FPS.lock().unwrap() as jint
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