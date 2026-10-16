package com.veloren.client

import android.content.Context
import android.graphics.*
import android.util.AttributeSet
import android.view.MotionEvent
import android.view.View
import kotlin.math.sqrt

/**
 * Dokunmatik oyun kontrolleri overlay'i.
 *
 * - Sol alt: sanal joystick (hareket x/y)
 * - Sağ alt: 4 action düğmesi (Primary, Secondary, Block, Jump)
 * - Sağ orta: kamera sürükleme alanı (look)
 *
 * Dokunma olayları [MainActivity] üzerinden JNI'ye [velorenSetInput] olarak iletilir.
 */
class TouchControlsView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : View(context, attrs, defStyleAttr) {

    // ---- Joystick durumu ----
    private val joystickCenter = PointF()
    private var joystickActive = false
    private var joystickTouchPos = PointF()
    private val joystickRadius = 80f
    private val joystickKnobRadius = 40f
    private var joystickId = -1

    // ---- Action butonları ----
    private lateinit var buttonPrimary: RectF
    private lateinit var buttonSecondary: RectF
    private lateinit var buttonBlock: RectF
    private lateinit var buttonJump: RectF
    private val buttonSize = 90f
    private val buttonGap = 20f

    // ---- Kamera (look) alanı ----
    private var lookActive = false
    private var lookTouchPos = PointF()
    private var lookId = -1

    // ---- Çağrı geri çağrısı ----
    var onInputListener: OnInputListener? = null

    interface OnInputListener {
        fun onJoystick(moveX: Float, moveY: Float)
        fun onAction(action: Int, pressed: Boolean)
        fun onLook(deltaX: Float, deltaY: Float)
    }

    init {
        joystickCenter.set(120f, 0f) // will be set in onSizeChanged
        buttonPrimary = RectF()
        buttonSecondary = RectF()
        buttonBlock = RectF()
        buttonJump = RectF()
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        joystickCenter.set(w * 0.15f, h * 0.85f)

        val bw = buttonSize
        val bh = buttonSize
        val rightX = w - bw - 40f
        val bottomY = h - bh - 60f

        // Jump (en üst)
        buttonJump.set(rightX, bottomY - bh - buttonGap, rightX + bw, bottomY)
        // Block (Jump'ın solu)
        buttonBlock.set(rightX - bw - buttonGap, bottomY - bh - buttonGap, rightX - buttonGap, bottomY)
        // Secondary (Block'ın altı)
        buttonSecondary.set(rightX, bottomY + bh + buttonGap, rightX + bw, bottomY + 2 * bh + buttonGap)
        // Primary (Secondary'nin sağında)
        buttonPrimary.set(rightX + bw + buttonGap, bottomY + bh + buttonGap, rightX + 2 * bw + buttonGap, bottomY + 2 * bh + buttonGap)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)

        // ---- Joystick çiz ----
        drawJoystick(canvas)

        // ---- Action butonları çiz ----
        drawButton(canvas, buttonPrimary, "A", Color.RED)
        drawButton(canvas, buttonSecondary, "B", Color.BLUE)
        drawButton(canvas, buttonBlock, "X", Color.GREEN)
        drawButton(canvas, buttonJump, "Y", Color.YELLOW)

        // ---- Kamera alanı (görünmez ama etkin) ----
        // Görsel olarak işaretlenmez, sadece dokunma alanı olarak kullanılır
    }

    private fun drawJoystick(canvas: Canvas) {
        // Joystick tabanı
        val basePaint = Paint().apply {
            color = Color.argb(100, 255, 255, 255)
            style = Paint.Style.FILL
            isAntiAlias = true
        }
        canvas.drawCircle(joystickCenter.x, joystickCenter.y, joystickRadius, basePaint)

        // Joystick knob (hareketli kısmı)
        val knobPos = if (joystickActive) joystickTouchPos else joystickCenter
        val knobPaint = Paint().apply {
            color = Color.argb(180, 200, 200, 200)
            style = Paint.Style.FILL
            isAntiAlias = true
        }
        canvas.drawCircle(knobPos.x, knobPos.y, joystickKnobRadius, knobPaint)

        // Joystick merkez noktası
        val centerPaint = Paint().apply {
            color = Color.argb(80, 255, 255, 255)
            style = Paint.Style.STROKE
            strokeWidth = 4f
            isAntiAlias = true
        }
        canvas.drawCircle(joystickCenter.x, joystickCenter.y, joystickRadius * 0.6f, centerPaint)
    }

    private fun drawButton(canvas: Canvas, rect: RectF, label: String, buttonColor: Int) {
        val semiTransparent = Color.argb(120, Color.red(buttonColor), Color.green(buttonColor), Color.blue(buttonColor))
        val buttonPaint = Paint().apply {
            this.color = semiTransparent
            style = Paint.Style.FILL
            isAntiAlias = true
        }
        canvas.drawRoundRect(rect, 16f, 16f, buttonPaint)

        val borderPaint = Paint().apply {
            this.color = Color.WHITE
            style = Paint.Style.STROKE
            strokeWidth = 3f
            isAntiAlias = true
        }
        canvas.drawRoundRect(rect, 16f, 16f, borderPaint)

        val textPaint = Paint().apply {
            this.color = Color.WHITE
            textSize = 40f
            isAntiAlias = true
            textAlign = Paint.Align.CENTER
        }
        val textBounds = Rect()
        textPaint.getTextBounds(label, 0, label.length, textBounds)
        val textX = rect.centerX()
        val textY = rect.centerY() + textBounds.height() / 2f
        canvas.drawText(label, textX, textY, textPaint)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val actionMasked = event.actionMasked
        val pointerIndex = event.actionIndex
        val pointerId = event.getPointerId(pointerIndex)

        when (actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                val x = event.getX(pointerIndex)
                val y = event.getY(pointerIndex)

                // Joystick alanı mı?
                if (isInJoystickArea(x, y)) {
                    joystickId = pointerId
                    joystickActive = true
                    joystickTouchPos.set(x, y)
                    updateJoystick()
                    return true
                }

                // Action butonları mı?
                if (buttonPrimary.contains(x, y)) {
                    onInputListener?.onAction(0, true) // Primary
                    return true
                }
                if (buttonSecondary.contains(x, y)) {
                    onInputListener?.onAction(1, true) // Secondary
                    return true
                }
                if (buttonBlock.contains(x, y)) {
                    onInputListener?.onAction(2, true) // Block
                    return true
                }
                if (buttonJump.contains(x, y)) {
                    onInputListener?.onAction(3, true) // Jump
                    return true
                }

                // Kamera alanı mı? (ekranın sağ yarısı, butonların dışında)
                if (x > width * 0.55f && y < height * 0.7f) {
                    lookId = pointerId
                    lookActive = true
                    lookTouchPos.set(x, y)
                    return true
                }
            }

            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> {
                if (pointerId == joystickId) {
                    joystickActive = false
                    joystickTouchPos.set(joystickCenter.x, joystickCenter.y)
                    updateJoystick()
                    joystickId = -1
                    return true
                }
                if (pointerId == lookId) {
                    lookActive = false
                    lookId = -1
                    return true
                }
                // Action butonları bırakıldı
                onInputListener?.onAction(0, false)
                onInputListener?.onAction(1, false)
                onInputListener?.onAction(2, false)
                onInputListener?.onAction(3, false)
                return true
            }

            MotionEvent.ACTION_MOVE -> {
                // Joystick hareketi
                if (joystickId != -1) {
                    val idx = event.findPointerIndex(joystickId)
                    if (idx >= 0) {
                        joystickTouchPos.set(event.getX(idx), event.getY(idx))
                        updateJoystick()
                        return true
                    }
                }

                // Kamera hareketi
                if (lookId != -1) {
                    val idx = event.findPointerIndex(lookId)
                    if (idx >= 0) {
                        val newX = event.getX(idx)
                        val newY = event.getY(idx)
                        val deltaX = newX - lookTouchPos.x
                        val deltaY = newY - lookTouchPos.y
                        lookTouchPos.set(newX, newY)
                        onInputListener?.onLook(deltaX * 0.5f, deltaY * 0.5f)
                        return true
                    }
                }
            }
        }

        return super.onTouchEvent(event)
    }

    private fun isInJoystickArea(x: Float, y: Float): Boolean {
        val dx = x - joystickCenter.x
        val dy = y - joystickCenter.y
        return dx * dx + dy * dy <= joystickRadius * joystickRadius
    }

    private fun updateJoystick() {
        if (!joystickActive) {
            onInputListener?.onJoystick(0f, 0f)
            return
        }

        val dx = joystickTouchPos.x - joystickCenter.x
        val dy = joystickTouchPos.y - joystickCenter.y
        val dist = (dx * dx + dy * dy).sqrt()

        var normX = dx / joystickRadius
        var normY = dy / joystickRadius

        // Sınırla
        if (dist > joystickRadius) {
            normX = dx / dist
            normY = dy / dist
        }

        // Dead zone
        if (dist < joystickRadius * 0.15f) {
            normX = 0f
            normY = 0f
        }

        // Y ekseni ters (ekranda aşağı pozitif, oyunda yukarı pozitif)
        onInputListener?.onJoystick(normX, -normY)
    }
}
