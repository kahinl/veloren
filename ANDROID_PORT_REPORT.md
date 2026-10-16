# Android Port — Hata ve Durum Raporu

## 1. Mevcut Durum (Durum: BUILD HATASI - Çalışmıyor)

Depo: `kahinl/veloren`, branch: `android-port`  
Fork: `kahinl/veloren`  
Origin: `veloren/veloren`  
GitHub Actions workflow: `.github/workflows/android.yml` ve `voxygen-check.yml`  
Hedef: `android-port` branch'inden Android APK üretmek  

Son durum: **Rust kodu derlenmiyor**. `Client::new()` fonksiyonu `&dyn Fn(ClientInitStage) + Send + Sync` istiyor, ancak Rust closure'ları bu tipe coerce olmuyor. Tüm alternatifler denendi ve hepsi fail oldu.

## 2. Crate Sürümleri (Cargo.lock'dan)

| Crate | Sürüm | Not |
|-------|-------|-----|
| jni | 0.21.1 | JNI FFI, API değişmiş |
| ndk-sys | 0.6.0+11769913 | OpenSL ES yok, ANativeWindow_from_surface yok |
| ndk | 0.6 | Android NDC wrapper |
| assets_manager | 0.13.10 | FileContent = Box<dyn AsRef<[u8]>> |
| wgpu | 22.1.0 | SurfaceTarget::AndroidNdk yok |
| futures-util | 0.3.34 | Kaldırıldı |
| once_cell | 1.19 | Kullanılmıyor, std::sync::OnceLock tercih ediliyor |
| tokio | 1.x | rt, macros, time |
| vek | 0.17 | serde, mint |
| veloren-client | local path | Client::new imzası değişmiş |
| veloren-common | local path | ECS, Controller, ControllerInputs |

## 3. Tespit Edilen Tüm Hatalar

### A. JNI / jni 0.21 API Uyumsuzluğu
1. `JMethodID: Into<JNIString>` değil — `call_method` metodu `JMethodID`'yi doğrudan kabul etmiyor
2. `JObject<'a>` lifetime specifier gerekli
3. `get_string(&event_type)` — referans bekleniyor
4. `JByteArray::null()` yok — `Default::default()` kullanılmalı
5. `GlobalRef` → `JObject` dönüşümü yok
6. `JObject::as_obj()` yok
7. `JMethodID.as_raw()` yok
8. `env.get_raw_jnienv()` yok — `env.get_raw()` ile değiştirildi

### B. Send / Sync (Statik depolama)
9. `*mut Audio` cannot be sent between threads safely
10. `*mut VoxygenRendererState` cannot be sent
11. `*mut jni_sys::_jmethodID` cannot be sent
12. `*mut ndk_sys::AAssetManager` cannot be sent

### C. OnceLock Kullanımı
13. `OnceLock<T>.lock()` metodu yok — `.get().unwrap().lock()` veya `.get()` kullanılmalı
14. `once_cell::sync::Lazy` → `std::sync::OnceLock` geçişi

### D. assets_manager 0.13
15. `FileContent` import yolu — `assets_manager::source::FileContent` yok
16. `FileContent::Owned(bytes)` — `Vec<u8>` değil, `Box<dyn AsRef<[u8]>>` istiyor
17. `path_of` ve `root` trait metodu yok
18. `Vec<u8>: assets_manager::Asset` değil — `Box<str>` veya özel tür gerekli

### E. wgpu 22 + raw-window-handle 0.6
19. `SurfaceTarget::AndroidNdk` yok
20. `Backends::VULKAN.primary()` yok — `Backends::VULKAN` direkt
21. `futures::executor::block_on` yok
22. `raw-window-handle` 0.6'da `WindowHandle` trait değil, struct

### F. veloren-client API Değişiklikleri
23. `veloren_client::init::{CredentialProvider, Credentials}` private modül
24. `client.state_mut().ecs_mut().write_storage::<Controller>()` — `write_storage` metodu yok
25. `client_ptr.as_mut()` — `jlong` (i64) üzerinde `as_mut()` yok
26. `CharacterId::to_string()` yok — `Debug` (`{:?}`) kullanılmalı
27. `GenericChatMsg<String>: Display` yok
28. `client.get_tick()` — `u64` döndürür, `as jint` cast gerekiyor

### G. Borrow Checker / Lifetime
29. `runtime` moved in async block — `Arc<Runtime>` ile çözüldü
30. `*mut client` immutable borrow conflict
31. `binding` not mutable — `let mut binding = ...` ile düzeltildi
32. `unwrap_or_default()` on `&str` — yok
33. `ext.unwrap_or_default()` on `&str` — yok

### H. Diğer
35. `AASSET_MODE_UNKNOWN` — `u32`, `i32` cast gerekli
36. `PathBuf` import eksik
37. `MAX_FPS` — `u32` vs `i32` cast
38. `client_ptr` unused parameter warnings
39. `Arc` import eksik

### I. Client::new Closure Hatası (SON HATA)
40. `Client::new()` expects `&dyn Fn(ClientInitStage) + Send + Sync`
41. Closure'lar `&dyn Fn` tipine coerce olmuyor
42. `Box<dyn Fn(ClientInitStage) + Send + Sync>` ile de denendi, fail
43. `Box::leak()` ile `&mut dyn Fn` denendi, fail
44. Named functions (`fn stage_callback(_: ClientInitStage) {}`) denendi, fail
45. `fn` pointer直接` kullanımı denendi, fail
46. `&dyn Fn(ClientInitStage) + Send + Sync` tipine dönüşüm yapılamıyor

## 4. Kod Dosyaları (android-rust/src/)

| Dosya | Durum |
|-------|-------|
| lib.rs | JNI giriş noktaları — **ÇALIŞMADI** (Client::new close hatası) |
| renderer.rs | wgpu renderer — STUB |
| audio.rs | OpenSL ES — STUB |
| audio_mixer.rs | PCM mixer — kısmi |
| events.rs | Java event callback — **KIRDIK** (call_method JNIString hatası) |
| voxygen_bridge.rs | Voxygen renderer — STUB |
| android_assets.rs | AAssetManager — kısmi |
| Cargo.toml | jni=0.21, wgpu=22, assets_manager=0.13, ndk=0.6, ndk-sys=0.6 |

## 5. Deneyen Tüm Yaklaşımlar (Sıralı)

### Senaryo 1: `&|client_ptr: ClientInitStage| {}` closure
**Sonuç:** FAIL — `JMethodID: Into<JNIString>` hatası

### Senaryo 2: `|_stage: ClientInitStage| {}` (no `&`)
**Sonuç:** FAIL — `JMethodID: Into<JNIString>` hatası

### Senaryo 3: `Box::new(|_stage| {}) as Box<dyn Fn(ClientInitStage) + Send + Sync>`
**Sonuç:** FAIL — `JMethodID: Into<JNIString>` hatası

### Senaryo 4: Named functions `fn stage_callback(_stage: ClientInitStage) {}`
**Sonuç:** FAIL — `JMethodID: Into<JNIString>` hatası

### Senaryo 5: `&stage_callback` ve `&builder_callback` (fonksiyon pointer'ı)
**Sonuç:** FAIL — `JMethodID: Into<JNIString>` hatası

### Senaryo 6: `Box::new(...) as Box<dyn Fn(...) + Send + Sync>`
**Sonuç:** FAIL — `JMethodID: Into<JNIString>` hatası

### Senaryo 7: `Box::leak(box)` for `&mut dyn Fn(...) + Send + Sync`
**Sonuç:** FAIL — `builder_cb does not live long enough`

### Senaryo 8: `Client::new`'i çağırmayı bırak, dummy `ptr: *mut Client`
**Sonuç:** FAIL — `match client_result` tipi `*mut Client` ama `Ok(mut client)` pattern bekleniyor

### Senaryo 9: `if ptr.is_null()` pattern ile dummy ptr
**Sonuç:** FAIL — `match client_result` tipi `*mut veloren_client::Client`, `Ok(mut client)` pattern uyuşmuyor

### Şu Anki Durum (Son Push)
- `client_result` = `*mut Client` (dummy null pointer)
- `match client_result` ifadesi `Ok(mut client)` pattern ile fail oluyor
- `client_result` bir raw pointer, `Result` değil
- `match` yapısı `Ok/Err` bekliyor ama `ptr` dönüyor

## 6. Olası Çözüm Yolları

### Seçenek A: `Client::new`'i tamamen kaldır
`Client::new` yerine `Client::default()` veya `Client::new_without_client()` gibi bir şey yok. `Client::new` tek constructor.

### Seçenek B: `Client::new`'i çağırmayı tamamen kaldır, sadece dummy ptr dön
```rust
let ptr: *mut Client = std::ptr::null_mut();
client_storage().lock().unwrap().replace(Some(ptr));
ptr as i64
```
Bu derlenir ama oyun çalışmaz (null pointer).

### Seçenek C: `Client::new`'in imzasını bul
`veloren_client::Client::new` imzasını inceleyip doğru parametreleri bulmak gerekir. Muhtemelen `add_foreign_systems` ve `event_handler` callback'leri `Option` yapılmış olabilir veya farklı bir imza vardır.

### Seçenek D: `Client::new`'i çağırmayı tamamen bırak, sadece APK üret
Derleme başarılı olur ama oyun çalışmaz. En azından APK dosyası elde edilir.

## 7. Sonraki Adımlar

1. `Client::new` çağrısını kaldırıp dummy pointer dön (Şu anki durum)
2. Eğer derlenirse, APK üretilir
3. Ardından `Client::new`'i nasıl çağıracağımız hakkında araştırma yapılır
4. `ClientInitStage` tipi incelenir, `Send + Sync` kontrol edilir
5. `Callback` türü doğru şekilde tanımlanır

---

**Son Güncelleme:** 2026-10-16 — Tüm senaryolar denendi, hiçbiri çalışmadı. `Client::new`'in `&dyn Fn(ClientInitStage) + Send + Sync` istemesi çözülemedi. Şu anki kod derlenmiyor.