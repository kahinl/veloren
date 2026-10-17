package com.veloren.client

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.os.Bundle
import android.text.InputType
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import java.io.File

class MainActivity : Activity(), SurfaceHolder.Callback {

    private lateinit var gameSurface: SurfaceView
    private lateinit var touchControls: TouchControlsView
    private lateinit var menuView: MenuView
    private lateinit var statusText: TextView
    private var gameLoopThread: GameLoopThread? = null
    private var isInitialized = false
    private var isRendererCreated = false
    private var audioInitialized = false

    // Veloren server address - configurable from menu
    private var serverAddress: String = "server.veloren.net"

    // Cached surface passed to the native renderer
    private var cachedSurface: Surface? = null
    private var surfaceWidth: Int = 0
    private var surfaceHeight: Int = 0

    // Account credentials for the server login (empty -> auth-less "android" fallback)
    private var loginUsername: String = ""
    private var loginPassword: String = ""

    // Audio mixer state
    private val AUDIO_SAMPLE_RATE = 44100
    private val AUDIO_FRAMES_PER_BUFFER = 1024 // ~23ms at 44.1kHz

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        loginUsername = getSharedPreferences("veloren", Context.MODE_PRIVATE)
            .getString("username", "") ?: ""

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
                if (loginUsername.isEmpty()) {
                    showLoginDialog()
                } else {
                    startConnect()
                }
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

        // Ship a CA trust store for the account auth server: Android has no
        // system store rustls can read, so copy the bundled PEM and point
        // SSL_CERT_FILE at it via native code.
        try {
            val caFile = File(filesDir, "cacert.pem")
            assets.open("cacert.pem").use { input ->
                caFile.outputStream().use { output -> input.copyTo(output) }
            }
            velorenSetCaCertPath(caFile.absolutePath)
        } catch (e: Exception) {
            statusText.text = "Veloren: CA bundle failed (${e.message})"
        }
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
            if (result != 0L) {
                isRendererCreated = true
                statusText.text = "Veloren: renderer created ($w x $h)"
                startGameLoopIfReady()
            } else {
                statusText.text = "Veloren: renderer create failed"
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
        startGameLoopIfReady()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            startGameLoopIfReady()
        } else {
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

    /** Ask for the account credentials required by the official server. */
    private fun showLoginDialog() {
        val layout = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(48, 16, 48, 8)
        }
        val userEdit = EditText(this).apply {
            hint = "Kullanıcı adı"
            setText(loginUsername)
        }
        val passEdit = EditText(this).apply {
            hint = "Şifre"
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
        }
        layout.addView(userEdit)
        layout.addView(passEdit)

        AlertDialog.Builder(this)
            .setTitle("Veloren hesabı")
            .setMessage(
                "Resmi sunucu hesap girişi ister. veloren.net'te ücretsiz hesap açabilirsin. " +
                    "Boş bırakırsan 'android' adıyla hesapsız sunuculara bağlanılır."
            )
            .setView(layout)
            .setPositiveButton("Bağlan") { _, _ ->
                loginUsername = userEdit.text.toString().trim()
                loginPassword = passEdit.text.toString()
                getSharedPreferences("veloren", Context.MODE_PRIVATE).edit()
                    .putString("username", loginUsername).apply()
                velorenSetCredentials(loginUsername, loginPassword)
                startConnect()
            }
            .setNegativeButton("İptal", null)
            .show()
    }

    /** Connect to the server and create the renderer. */
    private fun startConnect() {
        menuView.hide()
        statusText.text = "Veloren: connecting to $serverAddress..."
        // The server handshake blocks until it succeeds or times out,
        // so it must not run on the UI thread.
        Thread {
            val clientPtr = velorenInit(serverAddress)
            runOnUiThread {
                if (clientPtr != 0L) {
                    isInitialized = true
                    statusText.text = "Veloren: connected"
                    val surface = cachedSurface
                    if (surface != null && !isRendererCreated) {
                        val result =
                            velorenCreateRenderer(surface, surfaceWidth, surfaceHeight)
                        if (result != 0L) {
                            isRendererCreated = true
                            statusText.text =
                                "Veloren: renderer created ($surfaceWidth x $surfaceHeight)"
                        } else {
                            statusText.text = "Veloren: renderer create failed"
                        }
                    }
                    startGameLoopIfReady()
                } else {
                    val err = runCatching { velorenLastInitError() }
                        .getOrNull().orEmpty().take(160)
                    statusText.text = if (err.isNotEmpty())
                        "Veloren: connection failed ($err)"
                    else
                        "Veloren: connection failed"
                }
            }
        }.start()
    }

    /** Start the render loop once both the client and the renderer are ready. */
    private fun startGameLoopIfReady() {
        if (isInitialized && isRendererCreated && gameLoopThread == null) {
            gameLoopThread = GameLoopThread()
            gameLoopThread?.start()
        }
    }

    /** JNI declarations */
    external fun velorenInit(serverAddress: String): Long
    external fun velorenLastInitError(): String?
    external fun velorenSetCredentials(username: String, password: String)
    external fun velorenSetCaCertPath(path: String)
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
    external fun velorenResize(width: Int, height: Int)
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
        @Volatile
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
