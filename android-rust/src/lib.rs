use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JObject, JString};
use ndk_sys::AAssetManager_fromJava;
use std::panic;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Last error message from `velorenInit`, surfaced to the UI on failure.
static LAST_INIT_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn last_init_error_storage() -> &'static Mutex<Option<String>> {
    LAST_INIT_ERROR.get_or_init(|| Mutex::new(None))
}

/// Account credentials entered in the UI (username, password).
static CREDENTIALS: OnceLock<Mutex<Option<(String, String)>>> = OnceLock::new();

fn credentials_storage() -> &'static Mutex<Option<(String, String)>> {
    CREDENTIALS.get_or_init(|| Mutex::new(None))
}

/// File that the panic hook and progress breadcrumbs are written to
/// (path set from Java in `onCreate`).
static DIAG_LOG_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

/// Game-loop counters used for the diagnostic breadcrumbs.
static TICK_COUNT: AtomicU64 = AtomicU64::new(0);
static FRAME_COUNT: AtomicU64 = AtomicU64::new(0);

fn diag_log_path_storage() -> &'static Mutex<Option<PathBuf>> {
    DIAG_LOG_PATH.get_or_init(|| Mutex::new(None))
}

/// Append one line to the diagnostic log. The release profile is built with
/// `panic = "abort"`, so a panic kills the process before the UI can react;
/// this file (plus the panic hook below) is the only record of how far a run
/// got before it died.
fn diag_log(line: &str) {
    let Some(storage) = DIAG_LOG_PATH.get() else {
        return;
    };
    // try_lock: the hook must not deadlock if a panic happens while the log
    // lock is held; skipping one line is fine.
    let Ok(mut guard) = storage.try_lock() else {
        return;
    };
    let Some(path) = guard.as_mut() else {
        return;
    };
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&*path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// Record every panic to the diagnostic log before the runtime aborts.
fn install_panic_hook() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let default_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "unknown".to_string());
            let thread = std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_string();
            diag_log(&format!(
                "PANIC: {} (at {location}, thread {thread})",
                panic_message(info.payload())
            ));
            default_hook(info);
        }));
    });
}

/// Keep a coarse breadcrumb of game-loop progress (the last line left behind
/// tells where a crash happened).
fn record_tick_progress() {
    let count = TICK_COUNT.fetch_add(1, Ordering::Relaxed);
    if count == 0 {
        diag_log("first tick ok");
    } else if count % 300 == 0 {
        diag_log(&format!("tick {count} ok"));
    }
}

fn record_frame_progress() {
    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
    if count == 0 {
        diag_log("first frame ok");
    } else if count % 300 == 0 {
        diag_log(&format!("frame {count} ok"));
    }
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
/// Returns: i64 (raw pointer to Box<Client> cast to i64), 0 on failure.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenInit(
    mut env: JNIEnv,
    _class: JClass,
    server_address: JString,
) -> i64 {
    install_panic_hook();
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        *last_init_error_storage().lock().unwrap() = None;

        let server_addr: String = env
            .get_string(&server_address)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "server.veloren.net".to_string());

        let runtime = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("Failed to create tokio runtime: {}", e);
                *last_init_error_storage().lock().unwrap() = Some(format!("tokio runtime: {e}"));
                return 0i64;
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
                diag_log(&format!("client connected to {server_addr}"));
                ptr
            }
            Err(e) => {
                eprintln!("Veloren client init failed: {:?}", e);
                // Without credentials the client falls back to the auth-less
                // "android" name, which auth-required servers reject; say so
                // on screen instead of leaving only the raw server error.
                let has_creds = credentials_storage().lock().unwrap().is_some();
                let msg = if has_creds {
                    format!("{e:?}")
                } else {
                    format!("no account credentials set; {e:?}")
                };
                *last_init_error_storage().lock().unwrap() = Some(msg);
                0
            }
        }
    }));

    match result {
        Ok(ptr) => ptr,
        Err(_panic) => {
            eprintln!("Veloren init panicked, caught to prevent crash");
            *last_init_error_storage().lock().unwrap() = Some("init panic".to_string());
            0
        }
    }
}

/// Get the last client init error message (empty string when none).
/// Called from Java: `velorenLastInitError()`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenLastInitError(
    mut env: JNIEnv,
    _class: JClass,
) -> jni::sys::jstring {
    let msg = last_init_error_storage()
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_default();
    env.new_string(msg)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Set the account credentials used for server auth.
/// Called from Java: `velorenSetCredentials(username, password)`.
/// An empty username clears them (falls back to the auth-less "android" name).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetCredentials(
    mut env: JNIEnv,
    _class: JClass,
    username: JString,
    password: JString,
) {
    let user = env
        .get_string(&username)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let pass = env
        .get_string(&password)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let user = user.trim().to_string();
    *credentials_storage().lock().unwrap() = if user.is_empty() {
        None
    } else {
        Some((user, pass))
    };
}

/// Set the CA certificate bundle used for TLS connections (account auth).
/// Called from Java: `velorenSetCaCertPath(path)`
/// Android has no system CA store that rustls can read, so the app copies a
/// bundled PEM to its files dir and points `SSL_CERT_FILE` at it;
/// rustls-native-certs (used by the auth client) reads that variable.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetCaCertPath(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) {
    if let Ok(path_str) = env.get_string(&path) {
        let path_str: String = path_str.into();
        // SAFETY: called once from `MainActivity.onCreate` before any client
        // thread exists, so nothing else reads the environment concurrently.
        unsafe { std::env::set_var("SSL_CERT_FILE", &path_str) };
        println!("Android CA bundle path set: {path_str}");
    }
}

/// Create a writable stub asset directory and point the shared asset system
/// at it via `VELOREN_ASSETS`.
/// Called from Java: `velorenSetAssetDirPath(path)`
/// The APK ships no game asset files, but parts of the client load from the
/// asset system opportunistically (the physics system reads the ship
/// figuredata manifest on every tick), and that load panics while locating a
/// missing asset directory. A directory holding the canary file and an empty
/// ship manifest keeps the asset system functional; content that needs real
/// asset files stays unavailable.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetAssetDirPath(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) {
    if let Ok(path_str) = env.get_string(&path) {
        let path_str: String = path_str.into();
        // `ASSETS_PATH` appends `assets` unless the path already ends with it.
        let mut root = PathBuf::from(&path_str);
        if !root.ends_with("assets") {
            root = root.join("assets");
        }
        let common = root.join("common");
        let manifests = common.join("manifests");
        let result = std::fs::create_dir_all(&manifests)
            .and_then(|()| std::fs::write(common.join("canary.canary"), "VELOREN_CANARY_MAGIC\n"))
            .and_then(|()| {
                std::fs::write(
                    manifests.join("ship_manifest.ron"),
                    "#![enable(unwrap_newtypes)]\n{}\n",
                )
            });
        match result {
            Ok(()) => {
                // SAFETY: called once from `MainActivity.onCreate` before any
                // client thread exists, so nothing else reads the environment
                // concurrently.
                unsafe { std::env::set_var("VELOREN_ASSETS", &root) };
                println!("Android stub asset dir set: {}", root.display());
            },
            Err(err) => {
                diag_log(&format!("stub asset dir setup failed: {err}"));
                println!("Android stub asset dir setup failed: {err}");
            },
        }
    }
}

/// Set the file that the panic hook and progress breadcrumbs write to.
/// Called from Java: `velorenSetCrashLogPath(path)`
/// The Java side reads this file on the next launch and shows its content
/// when the previous run ended abnormally (`panic = "abort"` leaves no other
/// trace on the phone).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetCrashLogPath(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) {
    if let Ok(path_str) = env.get_string(&path) {
        let path_str: String = path_str.into();
        *diag_log_path_storage().lock().unwrap() = Some(PathBuf::from(&path_str));
        install_panic_hook();
        println!("Android crash log path set: {path_str}");
    }
}

/// Extract a readable message from a caught panic payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Run one tick of the Veloren client.
/// Called from Java each frame: `velorenTick(dt)`
/// Returns: i32 (0 on success, negative on error; the message goes to
/// `velorenLastInitError`)
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

    // A stretch in the background can leave a minutes-wide gap between ticks;
    // cap it like a slow frame instead of feeding one huge step to the client.
    let elapsed = elapsed.min(std::time::Duration::from_secs(1));

    let inputs = ControllerInputs::default();

    let client = unsafe { &mut *(client_ptr as *mut Client) };
    match client.tick(inputs, elapsed) {
        Ok(events) => {
            if !events.is_empty() {
                events::forward_events(&mut env, &events);
            }
            record_tick_progress();
            0
        }
        Err(e) => {
            eprintln!("Veloren tick error: {:?}", e);
            *last_init_error_storage().lock().unwrap() = Some(format!("tick error: {e:?}"));
            diag_log(&format!("tick error: {e:?}"));
            -3
        }
    }
}

/// Create the wgpu renderer and bind it to the Android Surface.
/// Called from Java: `velorenCreateRenderer(surface, width, height)`
/// Returns: i64 (raw pointer to Box<Renderer> cast to i64), 0 on failure.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenCreateRenderer(
    mut env: JNIEnv,
    _class: JClass,
    surface: JObject,
    width: i32,
    height: i32,
) -> i64 {
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let renderer = match renderer::Renderer::new(&env, surface, width as u32, height as u32) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Failed to create renderer: {}", e);
                return 0i64;
            }
        };

        // Defensive: release any renderer still stored (a missed drop) instead
        // of leaking it when the pointer is replaced.
        drop_renderer();

        let boxed = Box::new(renderer);
        let ptr = Box::into_raw(boxed) as *mut renderer::Renderer as i64;
        *renderer_storage().lock().unwrap() = Some(ptr);
        diag_log(&format!("renderer ok ({width} x {height})"));
        ptr
    }));

    match result {
        Ok(ptr) => ptr,
        Err(_panic) => {
            eprintln!("Renderer creation panicked, caught to prevent crash");
            0
        }
    }
}

/// Render one frame.
/// Called from Java each frame: `velorenRenderFrame()`
/// Returns: i32 (0 on success, -1 on error)
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenRenderFrame(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    // Hold the storage lock for the whole frame: a concurrent drop (surface
    // destroyed) must not tear the surface down while it is in use.
    let mut renderer_guard = renderer_storage().lock().unwrap();
    let Some(renderer_ptr) = renderer_guard.as_mut() else {
        return -1;
    };

    let renderer = unsafe { &mut *(*renderer_ptr as *mut renderer::Renderer) };
    match renderer.render() {
        Ok(()) => {
            record_frame_progress();
            0
        }
        Err(e) => {
            eprintln!("Render frame error: {:?}", e);
            *last_init_error_storage().lock().unwrap() = Some(format!("render error: {e:?}"));
            diag_log(&format!("render error: {e:?}"));
            -3
        }
    }
}

/// Resize the renderer surface.
/// Called from Java: `velorenResize(width, height)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenResize(
    _env: JNIEnv,
    _class: JClass,
    width: i32,
    height: i32,
) {
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let mut renderer_guard = renderer_storage().lock().unwrap();
        let Some(renderer_ptr) = renderer_guard.as_mut() else {
            return;
        };

        let renderer = unsafe { &mut *(*renderer_ptr as *mut renderer::Renderer) };
        renderer.resize(width as u32, height as u32);
    }));

    if result.is_err() {
        eprintln!("Renderer resize panicked, caught to prevent crash");
    }
}

/// Drop the renderer together with its wgpu surface. The Android window behind
/// the surface is destroyed when the app is backgrounded, so the surface can
/// never be reused — configuring it again aborts inside wgpu. A fresh renderer
/// is created when a new surface arrives, and until then frames are skipped.
fn drop_renderer() {
    let mut guard = renderer_storage().lock().unwrap();
    if let Some(renderer_ptr) = guard.take() {
        unsafe {
            drop(Box::from_raw(renderer_ptr as *mut renderer::Renderer));
        }
        diag_log("renderer dropped");
    }
}

/// Drop the renderer so a fresh one can be created for the next Surface.
/// Called from Java: `velorenDropRenderer()`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenDropRenderer(
    _env: JNIEnv,
    _class: JClass,
) {
    let result = panic::catch_unwind(panic::AssertUnwindSafe(drop_renderer));

    if result.is_err() {
        eprintln!("Renderer drop panicked, caught to prevent crash");
    }
}

/// Set the Android AAssetManager for loading assets from the APK.
/// Called from Java: `velorenSetAssetManager(assetManager)`
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetAssetManager(
    env: JNIEnv,
    _class: JClass,
    asset_manager: JObject,
) -> i32 {
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let env_ptr = env.get_raw();

        let mgr_ptr: *mut ndk_sys::AAssetManager = unsafe {
            AAssetManager_fromJava(env_ptr, asset_manager.into_raw())
        };

        if mgr_ptr.is_null() {
            eprintln!("Failed to get AAssetManager from Java");
            return -1;
        }

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
    }));

    match result {
        Ok(code) => code,
        Err(_panic) => {
            eprintln!("Asset manager setup panicked, caught to prevent crash");
            -1
        }
    }
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
/// Called from Java: `velorenShutdown()`
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
    drop_renderer();
    *runtime_storage().lock().unwrap() = None;
    *last_tick_storage().lock().unwrap() = None;

    // A clean shutdown proves the run did not crash: clear the diagnostics so
    // the next launch does not show a stale report.
    if let Some(path) = diag_log_path_storage().lock().unwrap().as_ref() {
        let _ = std::fs::remove_file(path);
    }

    #[cfg(target_os = "android")]
    {
        *asset_manager_storage().lock().unwrap() = None;
        *asset_cache_storage().lock().unwrap() = None;
    }
}

/// Set controller inputs from Android touch/joystick.
/// Called from Java: `velorenSetInput(moveX, moveY, moveZ, action, pressed)`
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
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
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
    }));

    if result.is_err() {
        eprintln!("Set input panicked, caught to prevent crash");
    }
}

/// Get the current tick count (for debugging).
/// Called from Java: `velorenGetTick()`
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

/// Offline spike (throwaway branch): references `veloren_server::Server::new` so the
/// linker proves the server crate can link for aarch64-linux-android. Not called from Java.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenOfflineSpikeMarker(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    let _keep = veloren_server::Server::new as *const ();
    0
}

/// Set mobile performance tier for Android optimizations.
/// Called from Java: `velorenSetMobilePerfTier(tier: int)`
/// tier: 0=None, 1=Low, 2=Medium
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetMobilePerfTier(
    _env: JNIEnv,
    _class: JClass,
    tier: i32,
) {
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
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
    }));

    if result.is_err() {
        eprintln!("Set mobile perf tier panicked, caught to prevent crash");
    }
}

/// Shared storage for the graphics/performance settings below.
/// These are initialized lazily via `get_or_init` so a first-time
/// call from Java never panics on an uninitialized `OnceLock`.
static WEATHER_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
static CLOUDS_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
static PARTICLES_ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
static THREAD_COUNT: OnceLock<Mutex<usize>> = OnceLock::new();
static ON_DEMAND_ASSETS: OnceLock<Mutex<bool>> = OnceLock::new();
static TEXTURE_COMPRESSION: OnceLock<Mutex<i32>> = OnceLock::new();
static MAX_FPS: OnceLock<Mutex<u32>> = OnceLock::new();

fn weather_enabled_storage() -> &'static Mutex<bool> {
    WEATHER_ENABLED.get_or_init(|| Mutex::new(true))
}

fn clouds_enabled_storage() -> &'static Mutex<bool> {
    CLOUDS_ENABLED.get_or_init(|| Mutex::new(true))
}

fn particles_enabled_storage() -> &'static Mutex<bool> {
    PARTICLES_ENABLED.get_or_init(|| Mutex::new(true))
}

fn thread_count_storage() -> &'static Mutex<usize> {
    THREAD_COUNT.get_or_init(|| Mutex::new(4))
}

fn on_demand_assets_storage() -> &'static Mutex<bool> {
    ON_DEMAND_ASSETS.get_or_init(|| Mutex::new(true))
}

fn texture_compression_storage() -> &'static Mutex<i32> {
    TEXTURE_COMPRESSION.get_or_init(|| Mutex::new(0))
}

fn max_fps_storage() -> &'static Mutex<u32> {
    MAX_FPS.get_or_init(|| Mutex::new(60))
}

/// Set weather enabled (rain/snow).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetWeatherEnabled(
    _env: JNIEnv,
    _class: JClass,
    enabled: u8,
) {
    *weather_enabled_storage().lock().unwrap() = enabled != 0;
}

/// Get weather enabled state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetWeatherEnabled(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    if *weather_enabled_storage().lock().unwrap() {
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
    *clouds_enabled_storage().lock().unwrap() = enabled != 0;
}

/// Get clouds enabled state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetCloudsEnabled(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    if *clouds_enabled_storage().lock().unwrap() {
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
    *particles_enabled_storage().lock().unwrap() = enabled != 0;
}

/// Get particles enabled state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetParticlesEnabled(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    if *particles_enabled_storage().lock().unwrap() {
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
    *thread_count_storage().lock().unwrap() = count.max(1) as usize;
    println!("Rayon thread count set to {}", count);
}

/// Get configured thread count.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetThreadCount(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    *thread_count_storage().lock().unwrap() as i32
}

/// Enable/disable on-demand asset loading.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetOnDemandAssets(
    _env: JNIEnv,
    _class: JClass,
    enabled: u8,
) {
    *on_demand_assets_storage().lock().unwrap() = enabled != 0;
}

/// Get on-demand asset loading state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetOnDemandAssets(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    if *on_demand_assets_storage().lock().unwrap() {
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
    *texture_compression_storage().lock().unwrap() = format;
}

/// Get texture compression format.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetTextureCompression(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    *texture_compression_storage().lock().unwrap()
}

/// Set maximum FPS limit for battery/heat saving.
/// Called from Java: `velorenSetMaxFps(fps: int)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenSetMaxFps(
    _env: JNIEnv,
    _class: JClass,
    fps: i32,
) {
    *max_fps_storage().lock().unwrap() = fps.max(15).min(120) as u32;
}

/// Get maximum FPS limit.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_veloren_client_MainActivity_velorenGetMaxFps(
    _env: JNIEnv,
    _class: JClass,
) -> i32 {
    *max_fps_storage().lock().unwrap() as i32
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
        let creds = credentials_storage().lock().unwrap().clone();
        async move {
            Ok(match creds {
                Some((user, pass)) if !user.is_empty() && !pass.is_empty() => {
                    Credentials::UsernamePassword(user, pass)
                },
                Some((user, _)) if !user.is_empty() => Credentials::Username(user),
                _ => Credentials::Username("android".to_string()),
            })
        }
    }
}

/// The look delta (camera rotation) from touch input.
static LOOK_DELTA: OnceLock<Mutex<Option<vek::Vec2<f32>>>> = OnceLock::new();

fn look_delta_storage() -> &'static Mutex<Option<vek::Vec2<f32>>> {
    LOOK_DELTA.get_or_init(|| Mutex::new(None))
}
