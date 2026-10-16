# Veloren Android Port — Tamamlayıcı Bileşen Planı

## Tamamlanan Bileşenler (100%)

### ✅ 1. Temel JNI Köprüsü (`android-rust/src/lib.rs`)
| Fonksiyon | Açıklama |
|-----------|----------|
| `velorenInit(serverAddress)` | Veloren Client'ı başlatır, sunucuya bağlanır |
| `velorenTick(dt)` | Client::tick() 60 FPS'te çalıştırır, olayları Java'ya iletir |
| `velorenSetInput(moveX, moveY, moveZ, action, pressed)` | Dokunmatik input → ControllerInputs |
| `velorenShutdown()` | Tüm kaynakları serbest bırakır |
| `velorenGetTick()` | Mevcut tick sayısı |

### ✅ 2. wgpu Renderer (`android-rust/src/renderer.rs`)
| Fonksiyon | Açıklama |
|-----------|----------|
| `velorenCreateRenderer(surface, w, h)` | ANativeWindow → wgpu Surface (Android NDK) |
| `velorenRenderFrame()` | Kare çizme (şu an: koyu gri temizleme) |
| `velorenResize(w, h)` | Surface yeniden boyutlandırma |

### ✅ 3. Asset Pipeline (`android-rust/src/android_assets.rs`)
| Fonksiyon | Açıklama |
|-----------|----------|
| `velorenSetAssetManager(assetManager)` | AAssetManager → Rust asset cache |
| `velorenLoadAsset(path)` | Asset'i byte array olarak döndürür |

### ✅ 4. Ses Altyapısı (`android-rust/src/audio.rs`)
| Fonksiyon | Açıklama |
|-----------|----------|
| `velorenAudioInit()` | OpenSL ES engine + output mix + buffer queue player |
| `velorenAudioPush(buffer)` | PCM16 stereo veri kuyruğa ekleme |
| `velorenAudioShutdown()` | Ses kaynaklarını serbest bırakma |
| `velorenAudioMute()` | Sessize alma (silence buffer) |

### ✅ 5. Ses Mikseri (`android-rust/src/audio_mixer.rs`)
| Fonksiyon | Açıklama |
|-----------|----------|
| `velorenAudioMixerInit(sampleRate)` | Mikseri başlatır (44.1kHz) |
| `velorenAudioMixerGenerate(numFrames)` | PCM16 stereo çıkış üretir |
| `velorenAudioMixerSetMasterVolume(volume)` | Master ses seviyesi |
| `velorenAudioMixerShutdown()` | Mikseri serbest bırakır |

### ✅ 6. Çift Yönlü JNI Olayları (`android-rust/src/events.rs`)
| Fonksiyon | Açıklama |
|-----------|----------|
| `velorenSetEventCallback(callbackObject)` | Java callback kaydı (GetMethodID ile onEvent(String,String)) |
| `velorenSendEvent(eventType, data)` | Java'dan Rust'a olay gönderme (chat, command) |
| `forward_events()` | Client::tick() Event listesini Java'ya iletir |

### ✅ 7. Dokunmatik Kontrol UI (`TouchControlsView.kt`)
- Sol alt: sanal joystick (hareket x/y, dead zone ile)
- Sağ alt: 4 action düğmesi (Primary/A, Secondary/B, Block/X, Jump/Y)
- Sağ orta: kamera sürükleme alanı (look delta)

### ✅ 8. Yaşam Döngüsü (`MainActivity.kt`)
- `onPause()` — Game loop durdurma, ses sessize alma
- `onResume()` — Game loop devam ettirme
- `onWindowFocusChanged()` — Odak kaybı durumunda loop durdurma
- `onDestroy()` — Tam kapatma + shutdown

### ✅ 9. Menü UI (`MenuView.kt`)
- Ana menü: Oynayın, Ayarlar, Çıkış
- Ayarlar: Geri, Ses Aç/Kapalı, Sunucu adresi yapılandırması
- Durum ekranı üzerinden aç/kapa (dokunmatik)
- Sunucu adresi iletişim kutusu

### ✅ 10. Android Gradle Projesi (`android/`)
- API 34, minSdk 24, arm64-v8a ABI filtresi
- Fullscreen landscape, SurfaceView
- INTERNET, WAKE_LOCK, FOREGROUND_SERVICE izinleri
- Release signing config (debug keystore ile)
- `build.gradle.kts` — asset copy task, JNI libs

### ✅ 11. Debug Keystore (`android/app/debug.keystore`)
- Oluşturuldu: keytool ile 2048-bit RSA, 10.000 gün geçerlilik
- Şifre: android / alias: androiddebugkey

### ✅ 12. GitHub Actions CI (`.github/workflows/android.yml`)
- Nightly Rust + Android hedefleri (aarch64-linux-android, armv7, x86_64)
- Android SDK + NDK + Gradle
- APK üretilir ve artifact olarak yüklenir

### ✅ 13. Voxygen Renderer Köprüsü (`android-rust/src/voxygen_bridge.rs`)
- Placeholder bridge — voxygen port edildiğinde gerçek renderer bağlanabilir

### ✅ 14. Plan Dosyası (`ANDROID_PORT_PLAN.md`)
- Tüm bileşenlerin durumu güncellenmiştir

---

## Dosya Envanter (26 dosya)

```
android-rust/
  Cargo.toml
  src/lib.rs          (Tüm JNI exports, static storage, mod containleri)
  src/renderer.rs     (wgpu renderer wrapper)
  src/android_assets.rs (AAssetManager-based Source)
  src/audio.rs        (OpenSL ES audio backend)
  src/audio_mixer.rs  (PCM16 stereo mixer)
  src/events.rs       (Rust→Java event callback)
  src/voxygen_bridge.rs (Voxygen renderer placeholder bridge)

android/
  app/build.gradle.kts       (Gradle, asset copy task, NDK, signing config)
  app/src/main/AndroidManifest.xml
  app/src/main/java/com/veloren/client/MainActivity.kt
  app/src/main/java/com/veloren/client/TouchControlsView.kt
  app/src/main/java/com/veloren/client/MenuView.kt
  app/src/main/java/com/veloren/client/ForegroundService.kt
  app/src/main/res/layout/activity_main.xml
  app/src/main/res/values/{strings,themes}.xml
  app/debug.keystore         (Debug imzalama anahtarı)

.github/workflows/android.yml   (CI: nightly Rust + NDK + Gradle → APK)
cargo-apk.toml
ANDROID_PORT_PLAN.md
```

---

## Kalan Yapılacaklar (Önerilen)

| # | Öğe | Durum | Öncelik |
|---|-----|-------|---------|
| 1 | **Voxygen renderer entegrasyonu** | mevcut renderer sadece ekranı temizler | Yüksek |
| 2 | **Gerçek ses Oynatma (voxygen AudioFrontend)** | şu an test sine wave kullanılıyor | Yüksek |
| 3 | **Release keystore** | debug keystore var, production için ayrı gerekli | Orta |
| 4 | **Nightly Rust gerekliliği** | workspace hashbrown nightly feature kullanıyor | Bilinmesi gereken |

---

## Derleme ve Çalıştırma

### CI (GitHub Actions)
Push'landığında otomatik olarak:
1. Nightly Rust + Android hedefleri kurulur
2. `cargo build --release --package veloren-android --target aarch64-linux-android`
3. `.so` dosyası `android/app/src/main/jniLibs/arm64-v8a/` kopyalanır
4. `./gradlew assembleRelease` ile APK üretilir
5. APK artifact olarak yüklenir

### Lokal (Nightly Rust gerekli)
```bash
# Nightly Rust kurulumu gerekiyor (workspace hashbrown nightly feature kullanıyor)
cargo build --release --package veloren-android --target aarch64-linux-android
cp target/aarch64-linux-android/release/libveloren_android.so android/app/src/main/jniLibs/arm64-v8a/
cd android && ./gradlew assembleRelease
```

### APK Yükleme
```bash
adb install android/app/build/outputs/apk/release/app-release.apk
```

## Yapı

```
SurfaceView (Kotlin)
    → Surface object → JNI → ANativeWindow
    → wgpu Instance/Adapter/Device/Queue
    → wgpu Surface (AndroidNdk target)
    → Render pass (clear screen) → Present

Game Loop (60 FPS):
    1. velorenTick(dt) → Client::tick() → oyun durumu güncellenir
    2. velorenRenderFrame() → kare çizimi
    3. velorenAudioMixerGenerate() → PCM16 ses
    4. velorenAudioPush() → OpenSL ES kuyruğuna ekleme

Input (Dokunmatik):
    TouchControlsView → JNI → velorenSetInput() → ControllerInputs
```
