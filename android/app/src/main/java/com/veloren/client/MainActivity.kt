package com.veloren.client

import android.app.AlertDialog
import android.content.Context
import android.os.Bundle
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity(), SurfaceHolder.Callback {

    private lateinit var gameSurface: SurfaceView
    private lateinit var touchControls: TouchControlsView
    private lateinit var menuView: MenuView
    private lateinit var statusText: TextView
    private var gameLoopThread: GameLoopThread? = null
    private var isInitialized = false
    private var isRendererCreated = false
    private var audioInitialized = false

    // Veloren server address - configurable from menu
    private var serverAddress: String = "veloren.club:3300"

    // Cached surface passed to the native renderer
    private var cachedSurface: Surface? = null
    private var surfaceWidth: Int = 0
    private var surfaceHeight: Int = 0

    // Audio mixer state
    private val AUDIO_SAMPLE_RATE = 44100
    private val AUDIO_FRAMES_PER_BUFFER = 1024 // ~23ms at 44.1kHz

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Build layout programmatically: SurfaceView + TouchControls overlay + Menu overlay
        val root = FrameLayout(this)

        gameSurface = SurfaceView(this)
        gameSurface.holder.addCallback(this)
        root.addView(
            gameSurface,
            FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT
            )
        )

        touchControls = TouchControlsView(this)
        touchControls.onInputListener = object : TouchControlsView.OnInputListener {
            override fun onJoystick(moveX: Float, moveY: Float) {
                velorenSetInput(moveX, moveY, 0.0f, 0, 1)
            }

            override fun onAction(action: Int, pressed: Boolean) {
                velorenSetInput(0.0f, 0.0f, 0.0f, action, if (pressed) 1 else 0)
            }

            override fun onLook(deltaX: Float, deltaY: Float) {
                velorenSetInput(deltaX, deltaY, 0.0f, 4, 1)
            }
        }
        root.addView(
            touchControls,
            FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT
            )
        )

        menuView = MenuView(this)
        menuView.onMenuAction = object : MenuView.OnMenuAction {
            override fun onPlayClicked() {
                menuView.hide()
                statusText.text = "Veloren: connecting to $serverAddress..."
                velorenInit(serverAddress)
            }

            override fun onSettingsClicked() {
                // Settings already shown by menu
            }

            override fun onQuitClicked() {
                finish()
            }

            override fun onBackClicked() {
                // Back to main menu handled in MenuView
            }

            override fun onAudioToggled(enabled: Boolean) {
                if (enabled) {
                    // Unmute would be handled by setting volume back
                } else {
                    velorenAudioMute()
                }
            }

            override fun onServerAddressConfigured() {
                serverAddress = menuView.serverAddress
                statusText.text = "Sunucu güncellendi: $serverAddress"
            }
        }
        root.addView(
            menuView,
            FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT
            )
        )

        statusText = TextView(this)
        statusText.text = "Veloren: initializing..."
        statusText.setTextColor(0xFFFFFFFF.toInt())
        statusText.textSize = 16f
        val lp = FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.WRAP_CONTENT,
            FrameLayout.LayoutParams.WRAP_CONTENT
        )
        lp.gravity = android.view.Gravity.BOTTOM or android.view.Gravity.START
        lp.setMargins(16, 16, 16, 16)
        val bgPaint = android.graphics.drawable.GradientDrawable()
        bgPaint.setColor(0x88000000.toInt())
        bgPaint.setCornerRadius(8f)
        statusText.background = bgPaint
        statusText.setPadding(12, 8, 12, 8)
        // Tap status text to toggle menu
        statusText.isClickable = true
        statusText.setOnClickListener {
            if (menuView.isVisible()) {
                menuView.hide()
            } else {
                menuView.show()
            }
        }
        root.addView(statusText, lp)

        setContentView(root)

        // Load the native library
        System.loadLibrary("veloren_android")

        // Pass the Android AssetManager to native code for asset loading
        val assetManager = applicationContext.assets
        val assetResult = velorenSetAssetManager(assetManager)
        if (assetResult != 0) {
            statusText.text = "Veloren: asset manager failed (code $assetResult)"
        }

        // Initialize audio mixer
        val mixerResult = velorenAudioMixerInit(AUDIO_SAMPLE_RATE)
        if (mixerResult != 0) {
            statusText.text = "Veloren: audio mixer init failed (code $mixerResult)"
        } else {
            audioInitialized = true
        }

        // Initialize audio (OpenSL ES)
        val audioResult = velorenAudioInit()
        if (audioResult != 0) {
            statusText.text = "Veloren: audio init failed (code $audioResult)"
        }

        // Detect device performance tier and apply Android optimizations
        val perfTier = detectMobilePerfTier()
        velorenSetMobilePerfTier(perfTier)

        // Initialize shader cache on first launch to avoid stuttering
        val shaderResult = velorenInitShaderCache()
        if (shaderResult != 0) {
            statusText.text = "Veloren: shader cache init failed (code $shaderResult)"
        }

        // Configure thread count for big.LITTLE ARM architecture
        val cpuCores = Runtime.getRuntime().availableProcessors()
        val threadCount = when {
            cpuCores <= 2 -> 2
            cpuCores <= 4 -> 4
            else -> 6
        }
        velorenSetThreadCount(threadCount)

        // Set texture compression to ASTC (preferred for mobile)
        velorenSetTextureCompression(1) // ASTC

        // Set max FPS to 30 for battery/heat saving
        velorenSetMaxFps(30)

        // Enable on-demand asset loading
        velorenSetOnDemandAssets(true)

        statusText.text = "Veloren: perf tier $perfTier - tap screen to play"
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        // Surface is ready; renderer will be created in surfaceChanged when we know the size
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, w: Int, h: Int) {
        surfaceWidth = w
        surfaceHeight = h
        cachedSurface = holder.surface

        if (!isRendererCreated && isInitialized && cachedSurface != null) {
            val result = velorenCreateRenderer(cachedSurface!!, w, h)
            if (result == 0L) {
                isRendererCreated = true
                statusText.text = "Veloren: renderer created ($w x $h)"
            } else {
                statusText.text = "Veloren: renderer create failed (code $result)"
            }
        } else if (isRendererCreated) {
            velorenResize(w, h)
        }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        cachedSurface = null
        surfaceWidth = 0
        surfaceHeight = 0
    }

    override fun onPause() {
        super.onPause()
        gameLoopThread?.running = false
        gameLoopThread = null
        velorenAudioMute()
    }

    override fun onResume() {
        super.onResume()
        if (isInitialized && isRendererCreated) {
            gameLoopThread = GameLoopThread()
            gameLoopThread?.start()
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && isInitialized && isRendererCreated) {
            gameLoopThread = GameLoopThread()
            gameLoopThread?.start()
        } else if (!hasFocus) {
            gameLoopThread?.running = false
            gameLoopThread = null
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        gameLoopThread?.running = false
        velorenAudioMixerShutdown()
        velorenAudioShutdown()
        velorenShutdown()
    }

    /** JNI declarations */
    external fun velorenInit(serverAddress: String): Long
    external fun velorenTick(dt: Float): Int
    external fun velorenSetAssetManager(assetManager: android.content.res.AssetManager): Int
    external fun velorenSetEventCallback(callbackObject: Any): Int
    external fun velorenSetMobilePerfTier(tier: Int)
    external fun velorenSetWeatherEnabled(enabled: Boolean)
    external fun velorenGetWeatherEnabled(): Int
    external fun velorenSetCloudsEnabled(enabled: Boolean)
    external fun velorenGetCloudsEnabled(): Int
    external fun velorenSetParticlesEnabled(enabled: Boolean)
    external fun velorenGetParticlesEnabled(): Int
    external fun velorenInitShaderCache(): Int
    external fun velorenSetThreadCount(count: Int)
    external fun velorenGetThreadCount(): Int
    external fun velorenSetOnDemandAssets(enabled: Boolean)
    external fun velorenGetOnDemandAssets(): Int
    external fun velorenSetTextureCompression(format: Int)
    external fun velorenGetTextureCompression(): Int
    external fun velorenSetMaxFps(fps: Int)
    external fun velorenGetMaxFps(): Int
    external fun velorenCreateRenderer(surface: Surface, width: Int, height: Int): Long
    external fun velorenRenderFrame(): Int
    external fun velorenLoadAsset(path: String): ByteArray
    external fun velorenResize(width: Int, height: Int): Int
    external fun velorenShutdown(): Unit
    external fun velorenSetInput(moveX: Float, moveY: Float, moveZ: Float, action: Int, pressed: Int): Unit
    external fun velorenGetTick(): Int
    external fun velorenAudioInit(): Int
    external fun velorenAudioPush(buffer: ByteArray)
    external fun velorenAudioShutdown(): Unit
    external fun velorenAudioMute(): Int
    external fun velorenAudioMixerInit(sampleRate: Int): Int
    external fun velorenAudioMixerGenerate(numFrames: Int): ByteArray
    external fun velorenAudioMixerSetMasterVolume(volume: Float)
    external fun velorenAudioMixerShutdown(): Unit

    /** Game loop thread - runs at 60 FPS */
    private inner class GameLoopThread : Thread() {
        var running = true
        private val targetFrameTime = 1_000_000_000L / 60 // 60 FPS in nanoseconds

        override fun run() {
            val startTime = System.nanoTime()
            var lastTime = startTime

            while (running) {
                val now = System.nanoTime()
                val elapsedNanos = now - lastTime
                lastTime = now

                if (elapsedNanos >= targetFrameTime) {
                    val dt = elapsedNanos / 1_000_000_000.0f

                    // 1) Update game state
                    velorenTick(dt)

                    // 2) Render frame
                    if (isRendererCreated) {
                        velorenRenderFrame()
                    }

                    // 3) Generate and push audio
                    if (audioInitialized) {
                        val audioBuffer = velorenAudioMixerGenerate(AUDIO_FRAMES_PER_BUFFER)
                        if (audioBuffer.size > 0) {
                            velorenAudioPush(audioBuffer)
                        }
                    }

                    // Update status text occasionally
                    val tick = velorenGetTick()
                    if (tick % 60 == 0) {
                        runOnUiThread {
                            statusText.text = "Veloren: tick $tick"
                        }
                    }
                }

                // Sleep to avoid busy-waiting
                val sleepTime = (targetFrameTime - (System.nanoTime() - now)).coerceAtLeast(0)
                if (sleepTime > 0) {
                    Thread.sleep(sleepTime / 1_000_000, (sleepTime % 1_000_000).toInt())
                }
            }
        }
    }

    /** Detect Android device performance tier based on RAM and CPU cores */
    private fun detectMobilePerfTier(): Int {
        val activityManager = getSystemService(Context.ACTIVITY_SERVICE) as android.app.ActivityManager
        val memoryInfo = android.app.ActivityManager.MemoryInfo()
        activityManager.getMemoryInfo(memoryInfo)
        val totalRamGB = memoryInfo.totalMem / (1024L * 1024 * 1024)
        val cpuCores = Runtime.getRuntime().availableProcessors()

        return when {
            totalRamGB <= 2 || cpuCores <= 2 -> 1 // Low tier: <=2GB RAM or <=2 cores
            totalRamGB <= 4 || cpuCores <= 4 -> 2 // Medium tier: <=4GB RAM or <=4 cores
            else -> 0 // None: high-end device
        }
    }

    /** Show a dialog to configure the server address */
    private fun showServerAddressDialog() {
        val input = EditText(this)
        input.setText(serverAddress)
        input.inputType = EditorInfo.TYPE_CLASS_TEXT
        input.setSelection(input.text.length)

        AlertDialog.Builder(this)
            .setTitle("Sunucu Adresi")
            .setMessage("Veloren sunucu adresini giriniz (host:port):")
            .setView(input)
            .setPositiveButton("Tamam") { _, _ ->
                serverAddress = input.text.toString()
                statusText.text = "Sunucu: $serverAddress"
            }
            .setNegativeButton("İptal") { _, _ -> }
            .show()
    }
}
