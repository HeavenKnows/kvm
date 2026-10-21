package dev.heavenknows.kvmreader

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Path
import android.os.Bundle
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo

/** User-enabled input adapter; not a system-wide mouse cursor or arbitrary keyboard injector. */
class ReaderAccessibilityService : AccessibilityService() {
    override fun onServiceConnected() { super.onServiceConnected(); current = this }
    override fun onDestroy() { if (current === this) current = null; super.onDestroy() }
    override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit
    override fun onInterrupt() = Unit

    fun dispatchTap(x: Float, y: Float) {
        val path = Path().apply { moveTo(x, y) }
        val gesture = GestureDescription.Builder()
            .addStroke(GestureDescription.StrokeDescription(path, 0, 60)).build()
        dispatchGesture(gesture, null, null)
    }

    fun dispatchSwipe(x1: Float, y1: Float, x2: Float, y2: Float, durationMs: Long = 350) {
        val path = Path().apply { moveTo(x1, y1); lineTo(x2, y2) }
        val gesture = GestureDescription.Builder()
            .addStroke(GestureDescription.StrokeDescription(path, 0, durationMs.coerceAtLeast(1))).build()
        dispatchGesture(gesture, null, null)
    }

    /** Writes to a focused editable node; otherwise places text on the local clipboard. */
    fun deliverText(text: String): Boolean {
        val node = rootInActiveWindow?.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)
        if (node != null && node.isEditable) {
            val args = Bundle().apply {
                putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text)
            }
            val delivered = node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)
            node.recycle()
            if (delivered) return true
        } else {
            node?.recycle()
        }
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(ClipData.newPlainText("kvm", text))
        return false
    }

    companion object {
        @Volatile private var current: ReaderAccessibilityService? = null
        fun instance(): ReaderAccessibilityService? = current
    }
}
