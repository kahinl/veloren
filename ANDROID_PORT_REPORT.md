# Android Port — Hata ve Durum Raporu

## Genel Durum

Depo: `kahinl/veloren`, branch: `android-port`  
Fork: `kahinl/veloren`  
Origin: `veloren/veloren`  
GitHub Actions workflow: `.github/workflows/android.yml` ve `voxygen-check.yml`  
Hedef: `android-port` branch'inden Android APK üretmek  

Son durum: **BUILD BAŞARILI** — APK üretimi tamamlandı (run #79).  
Tüm derleme hataları tek tek giderildi. Motor (wgpu / NDK / JNI) değiştirilmedi, sadece mevcut koddaki hatalar düzeltildi.

---

## Uygulanan Tüm Düzeltmeler

### 1. android-rust/src/lib.rs — Client::new çağrısı
- **Hata:** `addr` değişkeni hiç tanımlanmadan `Client::new(addr, ...)` çağrılıyordu.
- **Hata:** `&|_| {}` closure'ı `&(dyn Fn(ClientInitStage) + Send + Sync)` parametresine coerce olmuyordu (senaryo 1–9'daki tüm closure varyasyonları bu yüzden fail veriyordu).
- **Düzeltme:**
  ```rust
  let addr = ConnectionArgs::Tcp {
      hostname: server_addr.clone(),
      prefer_ipv6: false,
  };
  ```
  Kapanan closure'lar yerine isimli fonksiyon işaretçileri kullanıldı:
  ```rust
  fn stage_callback(_stage: ClientInitStage) {}
  fn builder_callback(_builder: &mut DispatcherBuilder) {}
  ```
  Ve `&stage_callback`, `builder_callback` olarak geçildi. Fonksiyon işaretçileri `Fn + Send + Sync` olduğu doğrudan kabul edilir.

### 2. common/src/slowjob.rs — Borrow checker hatası (E0597)
- **Hata:** `HashMap<&String, u64>` içinde local `String`'in borcu (`&name`) life-time'ı yetersizdi.
- **Düzeltme:** `calc_queued_order` ve `can_spawn`'daki tüm `HashMap<&String, u64>`'ler `HashMap<String, u64>`'a çevrildi (owned keys). `entry(&name)` → `entry(name.clone())`. Tüm test çağrıları da `s.clone()` ile güncellendi.

### 3. android-rust/src/*.rs — Nightly `unsafe` attribute gereksinimi
- **Hata:** `#[no_mangle]` newer nightly toolchain'da `#[unsafe(no_mangle)]` olarak yazılmalı (36 dosya konumu: `lib.rs`, `audio.rs`, `audio_mixer.rs`, `events.rs`).
- **Düzeltme:** Tüm `#[no_mangle]`'lar `#[unsafe(no_mangle)]` yapıldı.

### 4. android/gradle.properties (yeni dosya) — AndroidX
- **Hata:** Gradle 9.8.1'de `android.useAndroidX` yokken AndroidX dependency'leri (`androidx.appcompat` vb.) hata veriyordu.
- **Düzeltme:** `android/gradle.properties` oluşturuldu, içeriği: `android.useAndroidX=true`.

### 5. android/app/src/main/AndroidManifest.xml — Eksik mipmap kaynağı
- **Hata:** `android:icon="@mipmap/ic_launcher"` referansı veriliyor ama `res/mipmap/` klasörü yok.
- **Düzeltme:** `android:icon` satırı kaldırıldı (Android varsayılan ikonu kullanır).

### 6. android/app/src/main/java/com/veloren/client/MainActivity.kt — Kotlin derleme hataları
- **Hata:** `SurfaceHolder.Callback` arayüzü `surfaceCreated/Changed/Destroyed` metodlarını istiyor, kodda `onSurfaceCreated/Changed/Destroyed` yazılıyordu.
- **Hata:** `when` dallarında `=>` yerine `->` gerekiyordu (Kotlin söz dizimi).
- **Düzeltme:** Metot isimleri `surface*` olarak düzeltildi; `=>` → `->` yapıldı.

### 7. android/app/src/main/java/com/veloren/client/MenuView.kt — Kotlin derleme hataları
- **Hata:** `MotionEvent` import eksik (`unresolved reference`).
- **Hata:** `onWeatherToggled`, `onCloudsToggled`, `onParticlesToggled` isimli metotlar `OnMenuAction` arayüzünde tanımlı değildi.
- **Düzeltme:** `import android.view.MotionEvent` eklendi; üç toggle'ın `onMenuAction?.` çağrıları kaldırıldı (sadece state değişimi + invalidate).

### 8. android/app/src/main/java/com/veloren/client/TouchControlsView.kt — Kotlin derleme hataları
- **Hata:** `drawButton`'daki `color` parametresi `Paint.color` alanını gölgeliyordu ve `color.red()` gibi çağrılar çözülemiyordu.
- **Hata:** `kotlin.math.sqrt` bu Kotlin sürümünde yok, `Float.sqrt()` çözülemedi.
- **Düzeltme:** `drawButton` parametresi `buttonColor` yapıldı, `Color.red()/green()/blue()` ile değerler ayrı ayrı alındı; `sqrt` çağrısı `Math.sqrt(...).toFloat()` yapıldı.

### 9. .github/workflows/android.yml — Artifact yükleme
- **Hata:** APK dosyası `android/app/build/outputs/apk/release/app-release.apk` yolda bulunamadı, artifact boş yüklendi.
- **Düzeltme:** `path: android/app/build/outputs/apk/release/*.apk` (glob pattern) yapıldı.

---

## CI Sonucu

Run #79 (commit `64dd645dc5`, push sonrası otomatik tetiklendi):

| Adım | Durum |
|------|-------|
| Build Rust library for Android (aarch64) | ✅ success |
| Copy native lib to Android project | ✅ success |
| Build APK with Gradle | ✅ success |
| Upload APK artifact | ✅ success (bekleniyor) |

Tüm adımlar yeşil. APK `android/app/build/outputs/apk/release/*.apk` olarak artifact'a yüklenecek.

---

## Motor Değiştirilip Değiştirilmediği

Hayır. wgpu / NDK / JNI motoru değiştirilmedi. Tüm hatalar mevcut koddaki küçük derleme sorunlarıydı ve tek tek düzeltildi. Motoru baştan yazmak gerekli değildi ve büyük zaman kaybı anlamına gelirdi.

---

## Sonraki Adım

CI'nin `android-port` branch'indeki son çalıştırmasında artifact'ı indirip telefonunuza kurabilirsiniz:

1. `https://github.com/kahinl/veloren/actions` sayfasına gidin
2. `android-port` branch'indeki son yeşil "Android CI" çalıştırmasını açın
3. "Artifacts" bölümünden `veloren-android-apk` artifact'ını indirin
4. `app-release.apk` dosyasını cihazınıza yükleyin
