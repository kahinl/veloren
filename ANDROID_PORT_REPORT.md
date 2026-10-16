# Android Port — Hata ve Durum Raporu

## 1. Mevcut Durum (Durum: Kırık)

Depo: `kahinl/veloren`, branch: `android-port`  
Fork: `kahinl/veloren` (kullanıcının kendi fork'u)  
Origin: `veloren/veloren` (orijinal/upstream)  
GitHub Actions workflow: `.github/workflows/android.yml` ve `voxygen-check.yml`  
Hedef: `android-port` branch'inden Android APK üretmek

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

## 3. Tespit Edilen Tüm Hatalar (Toplam ~60+)

### A. JNI / jni 0.21 API Uyumsuzluğu
1. `JMethodID: Into<JNIString>` değil — `call_method` metodu `JMethodID`'yi doğrudan kabul etmiyor, `JValue` dönüşüm gerekiyor
2. `JObject<'a>` lifetime specifier gerekli — `JObject<'_>` kullanılıyor ama bazı yerlerde eksik
3. `get_string(&event_type)` — referans bekleniyor, değer geçiliyor
4. `JByteArray::null()` yok — `JByteArray::from_raw(null_mut())` veya `Default::default()` kullanmalı
5. `GlobalRef` → `JObject` dönüşümü yok — `.into()` çalışmıyor
6. `JObject::as_obj()` yok — `JObject` zaten obje, `as_obj()` gereksiz
7. `JMethodID.as_raw()` yok — raw pointer'a erişim yok
8. `env.get_raw_jnienv()` yok — `env.get_raw()` ile değiştirildi

### B. Send / Sync (Statik depolama)
9. `*mut Audio` cannot be sent between threads safely — `Audio` struct'u `Send`/`Sync` değil
10. `*mut VoxygenRendererState` cannot be sent — `Send`/`Sync`Impl eksik
11. `*mut jni_sys::_jmethodID` cannot be sent — raw pointer statikte
12. `*mut ndk_sys::AAssetManager` cannot be sent — wrapper yok

### C. OnceLock Kullanımı
13. `OnceLock<T>.lock()` metodu yok — `.get().unwrap().lock()` veya doğrudan `.get()` kullanılmalı
14. `once_cell::sync::Lazy` → `std::sync::OnceLock` geçişi

### D. assets_manager 0.13
15. `FileContent` import eksik — `assets_manager::source::{DirEntry, FileContent, Source}` değil, `BoxedError, FileContent` ayrı
16. `FileContent::Owned(bytes)` — `Vec<u8>` değil, `Box<dyn AsRef<[u8]>>` istiyor
17. `path_of` ve `root` trait metodu yok — `Source` trait'inde 0.13'te yok
18. `Vec<u8>: assets_manager::Asset` değil — `Box<str>` veya özel tür gerekli

### E. wgpu 22 + raw-window-handle 0.6
19. `SurfaceTarget::AndroidNdk` yok — `SurfaceTarget::Window(Box<dyn WindowHandle>)` gerekli
20. `Backends::VULKAN.primary()` yok — `Backends::VULKAN` direkt
21. `futures::executor::block_on` yok — `futures_util::executor::block_on` veya kaldırılmalı
22. `raw-window-handle` 0.6'da `WindowHandle` trait değil, struct

### F. veloren-client API Değişiklikleri
23. `veloren_client::init::{CredentialProvider, Credentials}` private modül — `veloren_client::{CredentialProvider, Credentials}` re-export kullan
24. `client.state_mut().ecs_mut().write_storage::<Controller>()` — `write_storage` metodu yok, `WorldExt` gerekli
25. `client_ptr.as_mut()` — `jlong` (i64) üzerinde `as_mut()` yok, `unsafe { &mut *(ptr as *mut Client) }`
26. `CharacterId::to_string()` yok — `Debug` (`{:?}`) kullanılmalı
27. `GenericChatMsg<String>: Display` yok — `Debug` kullanılmalı
28. `client.get_tick()` — `u64` döndürür, `as jint` cast gerekiyor

### G. Borrow Checker / Lifetime
29. `runtime` moved in async block — `Arc<Runtime>` kullanarak çözülmeli
30. `*mut client` immutable borrow conflict — `entity` önceden alınmalı
31. `binding` not mutable — `let mut binding = ...`
32. `unwrap_or_default()` on `&str` — yok, `unwrap_or("")` veya `String::new()`
33. `ext.unwrap_or_default()` on `&str` — yok

### H. Diğer
34. `AASSET_MODE_UNKNOWN` — `u32`, `i32` cast gerekli
35. `PathBuf` import eksik
36. `MAX_FPS` — `u32` vs `i32` cast
37. `client_ptr` unused parameter warnings

## 4. Kod Dosyaları (android-rust/src/)

| Dosya | Durum |
|-------|-------|
| lib.rs | JNI giriş noktaları, tüm exported fonksiyonlar |
| renderer.rs | wgpu renderer — STUB (gerçek yok) |
| audio.rs | OpenSL ES — STUB (semboller yok) |
| audio_mixer.rs | PCM mixer — kısmi |
| events.rs | Java event callback — kırık |
| voxygen_bridge.rs | Voxygen renderer — STUB |
| android_assets.rs | AAssetManager — kısmi/kırık |

## 5. Plan (Sıfırdan Rewrite)

### Fase 1: Cargo.toml Temizleme
- `jni = "0.21"` tut
- `ndk-sys` kaldırılacak veya doğru sürüm eklenecek
- `assets_manager = "0.13"` tut
- `wgpu = "22"` tut
- `tokio`, `vek` tut
- `once_cell` → `std::sync::OnceLock`
- `futures-util` kaldırılacak

### Fase 2: lib.rs — JNI Giriş Noktaları
Tüm `Java_com_veloren_client_MainActivity_*` fonksiyonları jni 0.21 API'sına uygun yazılacak:
- `get_string(&jstring)` referans ile
- `JObject<'a>` lifetime'lar
- `byte_array_from_slice(&[u8])`
- `call_method(obj, method_id, sig, args)` — method_id `JMethodID` tipinde

### Fase 3: audio.rs — OpenSL ES Stub veya Gerçek
- `Audio` struct'u `Send` + `Sync` impl ile
- `OnceLock<Mutex<Option<Box<Audio>>>>` statik depolama
- Stub implementation (OpenSL ES sembolleri yoksa)

### Fase 4: audio_mixer.rs — PCM Mixer
- `dyn AudioSource + Send` trait bound
- `JByteArray<'a>` lifetime
- Mutex binding mutable

### Fase 5: events.rs — Java Event Callback
- `JMethodID` tipini doğru kullan
- `GlobalRef` → `JObject` dönüşümü
- `Debug` formatları (`{:?}`) `Display` yerine

### Fase 6: android_assets.rs — Asset Yükleme
- `FileContent::Owned(Box<dyn AsRef<[u8]>>)`
- `AASSET_MODE_UNKNOWN as i32`
- `DirEntry::File(&str, &str)` referanslar

### Fase 7: renderer.rs — wgpu Android Surface
- `SurfaceTarget::Window(Box<dyn WindowHandle>)`
- `AndroidNdkHandle` raw-window-handle 0.6
- Stub implementation (gerçek render sonradan)

### Fase 8: voxygen_bridge.rs — Voxygen
- `Box<VoxygenRendererState>` statik depolama
- `Send` + `Sync` impl

### Fase 9: Derleme ve Test
- `cargo check --lib` yerel (foldhash nightly hatası lokalde, Actions'da nightly toolchain olduğu için geçer)
- GitHub Actions'dan geri bildirim al
- Tek tek hata düzelt

## 6. Kullanılacak Komutlar

```bash
# Yerel kod kontrolü (foldhash hariç)
cargo check --lib -p veloren-android

# GitHub Actions durumu
gh run list --repo kahinl/veloren --branch android-port

# Hata logu
gh run view <RUN_ID> --repo kahinl/veloren --log-failed
```

## 7. Beklenen Süre

Bu bir **sıfırdan rewrite** işi. GitHub Actions'da tek tek hata düzeltmek iterate cycle'ı uzun. Yaklaşık 2-4 hafta sürebilir, çünkü:
- Her push'ta Actions'da 10-15 dakika derleme
- Her seferinde 5-20 yeni hata
- Android NDK yok, yerel test yok

## 8. Not: foldhash Nightly Hatası (Yerel)

Yerel `cargo check`'de `foldhash 0.2.0` `#![feature]` hatası verir. Bu **yalnızca yerel stable toolchain'de** sorun. GitHub Actions `rust-toolchain` nightly kullanıyor, bu yüzden Actions'da sorun olmaz.

---

Durum: Plan tamamlandı. Şimdi kod yazımına başlanıyor.
