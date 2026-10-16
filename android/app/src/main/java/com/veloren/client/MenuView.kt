package com.veloren.client

import android.app.AlertDialog
import android.content.Context
import android.graphics.*
import android.text.InputType
import android.util.AttributeSet
import android.view.MotionEvent
import android.view.View
import android.widget.EditText

/**
 * Dokunmatik destekli oyun menüsü overlay'i.
 *
 * Durumlar:
 * - MAIN_MENU: Ana menü (Oynayın, Ayarlar, Çıkış)
 * - SETTINGS: Ayarlar ekranı (sunucu adresi, ses, kontroller)
 * - HIDDEN: Menü kapalı (oyun devam ediyor)
 */
class MenuView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : View(context, attrs, defStyleAttr) {

    enum class MenuState {
        MAIN_MENU,
        SETTINGS,
        HIDDEN
    }

    private var state = MenuState.MAIN_MENU

    private val titlePaint = Paint().apply {
        color = Color.WHITE
        textSize = 48f
        isAntiAlias = true
        textAlign = Paint.Align.CENTER
    }

    private val buttonPaint = Paint().apply {
        color = Color.argb(150, 60, 60, 80)
        style = Paint.Style.FILL
        isAntiAlias = true
    }

    private val buttonBorderPaint = Paint().apply {
        color = Color.WHITE
        style = Paint.Style.STROKE
        strokeWidth = 3f
        isAntiAlias = true
    }

    private val buttonTextPaint = Paint().apply {
        color = Color.WHITE
        textSize = 32f
        isAntiAlias = true
        textAlign = Paint.Align.CENTER
    }

    private val settingPaint = Paint().apply {
        color = Color.WHITE
        textSize = 24f
        isAntiAlias = true
        textAlign = Paint.Align.LEFT
    }

    // Button rectangles
    private lateinit var btnPlay: RectF
    private lateinit var btnSettings: RectF
    private lateinit var btnQuit: RectF
    private lateinit var btnBack: RectF

    // Toggle rectangles (for settings)
    private lateinit var toggleAudio: RectF
    private lateinit var toggleWeather: RectF
    private lateinit var toggleClouds: RectF
    private lateinit var toggleParticles: RectF
    private lateinit var btnServerAddress: RectF
    private var audioEnabled = true
    private var weatherEnabled = false
    private var cloudsEnabled = false
    private var particlesEnabled = false

    // Current server address (displayed in settings)
    var serverAddress: String = "veloren.club:3300"

    var onMenuAction: OnMenuAction? = null

    interface OnMenuAction {
        fun onPlayClicked()
        fun onSettingsClicked()
        fun onQuitClicked()
        fun onBackClicked()
        fun onAudioToggled(enabled: Boolean)
        fun onServerAddressConfigured()
    }

    init {
        // Buttons will be laid out in onSizeChanged
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        val bw = 300f
        val bh = 70f
        val centerX = w / 2f
        val centerY = h / 2f

        btnPlay = RectF(centerX - bw / 2, centerY - bh - 20, centerX + bw / 2, centerY - 20)
        btnSettings = RectF(centerX - bw / 2, centerY + 20, centerX + bw / 2, centerY + 20 + bh)
        btnQuit = RectF(centerX - bw / 2, centerY + 20 + bh + 20, centerX + bw / 2, centerY + 20 + 2 * bh + 20)

        btnBack = RectF(40f, 40f, 200f, 100f)

        toggleAudio = RectF(centerX - bw / 2, centerY - 110, centerX + bw / 2, centerY - 50)
        toggleWeather = RectF(centerX - bw / 2, centerY - 40, centerX + bw / 2, centerY + 20)
        toggleClouds = RectF(centerX - bw / 2, centerY + 30, centerX + bw / 2, centerY + 80)
        toggleParticles = RectF(centerX - bw / 2, centerY + 100, centerX + bw / 2, centerY + 150)

        btnServerAddress = RectF(centerX - bw / 2, centerY + 170, centerX + bw / 2, centerY + 170 + bh)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)

        // Dim background when menu is open
        if (state != MenuState.HIDDEN) {
            val dimPaint = Paint().apply {
                color = Color.argb(180, 0, 0, 0)
                style = Paint.Style.FILL
            }
            canvas.drawRect(0f, 0f, width.toFloat(), height.toFloat(), dimPaint)
        }

        when (state) {
            MenuState.MAIN_MENU -> drawMainMenu(canvas)
            MenuState.SETTINGS -> drawSettings(canvas)
            MenuState.HIDDEN -> { /* nothing */ }
        }
    }

    private fun drawMainMenu(canvas: Canvas) {
        val cx = width / 2f
        val cy = height / 2f - 120f

        // Title
        canvas.drawText("Veloren", cx, cy, titlePaint)

        // Buttons
        drawButton(canvas, btnPlay, "Oynayın")
        drawButton(canvas, btnSettings, "Ayarlar")
        drawButton(canvas, btnQuit, "Çıkış")
    }

    private fun drawSettings(canvas: Canvas) {
        val cx = width / 2f
        val cy = height / 2f - 100f

        canvas.drawText("Ayarlar", cx, cy - 60, titlePaint)

        // Back button
        drawButton(canvas, btnBack, "Geri")

        // Audio toggle
        val audioLabel = if (audioEnabled) "Ses: AÇIK" else "Ses: KAPALI"
        val audioColor = if (audioEnabled) Color.GREEN else Color.RED
        val audioPaint = Paint(buttonPaint).apply { color = audioColor }
        canvas.drawRoundRect(toggleAudio, 16f, 16f, audioPaint)
        canvas.drawRoundRect(toggleAudio, 16f, 16f, buttonBorderPaint)
        canvas.drawText(audioLabel, toggleAudio.centerX(), toggleAudio.centerY() + 10f, buttonTextPaint)

        // Weather toggle
        val weatherLabel = if (weatherEnabled) "Yağmur: AÇIK" else "Yağmur: KAPALI"
        val weatherColor = if (weatherEnabled) Color.GREEN else Color.GRAY
        val weatherPaint = Paint(buttonPaint).apply { color = weatherColor }
        canvas.drawRoundRect(toggleWeather, 16f, 16f, weatherPaint)
        canvas.drawRoundRect(toggleWeather, 16f, 16f, buttonBorderPaint)
        canvas.drawText(weatherLabel, toggleWeather.centerX(), toggleWeather.centerY() + 10f, buttonTextPaint)

        // Clouds toggle
        val cloudsLabel = if (cloudsEnabled) "Bulutlar: AÇIK" else "Bulutlar: KAPALI"
        val cloudsColor = if (cloudsEnabled) Color.GREEN else Color.GRAY
        val cloudsPaint = Paint(buttonPaint).apply { color = cloudsColor }
        canvas.drawRoundRect(toggleClouds, 16f, 16f, cloudsPaint)
        canvas.drawRoundRect(toggleClouds, 16f, 16f, buttonBorderPaint)
        canvas.drawText(cloudsLabel, toggleClouds.centerX(), toggleClouds.centerY() + 10f, buttonTextPaint)

        // Particles toggle
        val particlesLabel = if (particlesEnabled) "Parçacıklar: AÇIK" else "Parçacıklar: KAPALI"
        val particlesColor = if (particlesEnabled) Color.GREEN else Color.GRAY
        val particlesPaint = Paint(buttonPaint).apply { color = particlesColor }
        canvas.drawRoundRect(toggleParticles, 16f, 16f, particlesPaint)
        canvas.drawRoundRect(toggleParticles, 16f, 16f, buttonBorderPaint)
        canvas.drawText(particlesLabel, toggleParticles.centerX(), toggleParticles.centerY() + 10f, buttonTextPaint)

        // Server address button
        drawButton(canvas, btnServerAddress, "Sunucu: $serverAddress")

        // Server address placeholder
        canvas.drawText("Sunucu: veloren.club:3300", 40f, cy + 80f, settingPaint)
        canvas.drawText("Kontroller: Dokunmatik joystick + butonlar", 40f, cy + 110f, settingPaint)
    }

    private fun drawButton(canvas: Canvas, rect: RectF, label: String) {
        canvas.drawRoundRect(rect, 12f, 12f, buttonPaint)
        canvas.drawRoundRect(rect, 12f, 12f, buttonBorderPaint)

        val textBounds = Rect()
        buttonTextPaint.getTextBounds(label, 0, label.length, textBounds)
        val textX = rect.centerX()
        val textY = rect.centerY() + textBounds.height() / 2f
        canvas.drawText(label, textX, textY, buttonTextPaint)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (state == MenuState.HIDDEN) return false

        val x = event.x
        val y = event.y

        if (event.actionMasked == android.view.MotionEvent.ACTION_DOWN) {
            when (state) {
                MenuState.MAIN_MENU -> {
                    if (btnPlay.contains(x, y)) {
                        onMenuAction?.onPlayClicked()
                        return true
                    }
                    if (btnSettings.contains(x, y)) {
                        state = MenuState.SETTINGS
                        invalidate()
                        return true
                    }
                    if (btnQuit.contains(x, y)) {
                        onMenuAction?.onQuitClicked()
                        return true
                    }
                }
                MenuState.SETTINGS -> {
                    if (btnBack.contains(x, y)) {
                        state = MenuState.MAIN_MENU
                        invalidate()
                        return true
                    }
                    if (toggleAudio.contains(x, y)) {
                        audioEnabled = !audioEnabled
                        onMenuAction?.onAudioToggled(audioEnabled)
                        invalidate()
                        return true
                    }
                    if (toggleWeather.contains(x, y)) {
                        weatherEnabled = !weatherEnabled
                        invalidate()
                        return true
                    }
                    if (toggleClouds.contains(x, y)) {
                        cloudsEnabled = !cloudsEnabled
                        invalidate()
                        return true
                    }
                    if (toggleParticles.contains(x, y)) {
                        particlesEnabled = !particlesEnabled
                        invalidate()
                        return true
                    }
                    if (btnServerAddress.contains(x, y)) {
                        showServerAddressDialog()
                        return true
                    }
                }
                MenuState.HIDDEN -> {}
            }
        }

        return true // Consume touch events when menu is open
    }

    fun show() {
        state = MenuState.MAIN_MENU
        invalidate()
    }

    fun hide() {
        state = MenuState.HIDDEN
        invalidate()
    }

    private fun showServerAddressDialog() {
        val input = EditText(context)
        input.setText(serverAddress)
        input.inputType = InputType.TYPE_CLASS_TEXT
        input.setSelection(input.text.length)

        AlertDialog.Builder(context)
            .setTitle("Sunucu Adresi")
            .setMessage("Veloren sunucu adresini giriniz (host:port):")
            .setView(input)
            .setPositiveButton("Tamam") { _, _ ->
                serverAddress = input.text.toString()
                onMenuAction?.onServerAddressConfigured()
                invalidate()
            }
            .setNegativeButton("İptal") { _, _ -> }
            .show()
    }

    fun isVisible(): Boolean {
        return state != MenuState.HIDDEN
    }
}
