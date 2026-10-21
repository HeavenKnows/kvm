package dev.heavenknows.kvmreader

import android.accessibilityservice.AccessibilityServiceInfo
import android.app.Activity
import android.content.ComponentName
import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import android.view.ViewGroup
import android.view.accessibility.AccessibilityManager
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.text.InputType
import android.graphics.Typeface
import android.util.TypedValue

class MainActivity : Activity() {
    private lateinit var accessibilityStatus: TextView
    private lateinit var transportStatus: TextView
    private lateinit var pairingKey: EditText
    private lateinit var testText: EditText

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(24), dp(20), dp(24))
        }
        val scroll = ScrollView(this).apply { addView(content) }
        content.addView(label("kvm · 掌阅 Ocean5 Pro", 25f, true))
        content.addView(label("Android 14 / API 34 开发版。第一阶段仅面向 Ocean5 Pro。", 15f))
        content.addView(label("无障碍权限", 19f, true).marginTop(20))
        accessibilityStatus = label("正在检查…", 15f)
        content.addView(accessibilityStatus)
        content.addView(button("打开系统无障碍设置") {
            startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
        }.marginTop(8))

        content.addView(label("配对码格式检查", 19f, true).marginTop(24))
        pairingKey = EditText(this).apply {
            hint = "32 位十六进制密钥，或 6–16 位字母/数字码"
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
            setSingleLine(true)
        }
        content.addView(pairingKey, matchWrap())
        content.addView(button("检查配对码格式") {
            transportStatus.text = if (PairingKeyValidator.isValid(pairingKey.text.toString())) {
                "配对码格式有效；加密握手和网络互通尚未接入。"
            } else {
                "格式无效：请输入 32 位十六进制密钥，或 6–16 位 ASCII 字母/数字码。"
            }
        }.marginTop(8))

        content.addView(label("本地文本输入测试", 19f, true).marginTop(24))
        testText = EditText(this).apply { hint = "测试文本"; setText("kvm 测试文本"); minLines = 2 }
        content.addView(testText, matchWrap())
        content.addView(button("发送到当前焦点文本框") {
            val service = ReaderAccessibilityService.instance()
            if (service == null) {
                transportStatus.text = "请先在系统无障碍设置中启用 kvm 服务。"
            } else {
                transportStatus.text = if (service.deliverText(testText.text.toString())) {
                    "已将文本写入当前焦点文本框。"
                } else {
                    "没有可编辑焦点；文本已复制到阅读器本机剪贴板，可手动粘贴。"
                }
            }
        }.marginTop(8))

        content.addView(label("连接状态", 19f, true).marginTop(24))
        transportStatus = label(
            "尚未连接：Noise 加密 TCP 与 MessagePack 兼容层仍在实现中。当前版本不提供远程键鼠或电脑到阅读器的网络剪贴板同步。",
            15f
        )
        content.addView(transportStatus)
        content.addView(label(
            "Android 必须由用户手动启用无障碍服务。此服务可提交触摸手势、向焦点文本框写入文本；它不等于系统级鼠标指针，也不支持任意物理键盘注入。",
            14f
        ).marginTop(18))
        setContentView(scroll)
        refreshAccessibilityStatus()
    }

    override fun onResume() {
        super.onResume()
        if (::accessibilityStatus.isInitialized) refreshAccessibilityStatus()
    }

    private fun refreshAccessibilityStatus() {
        val manager = getSystemService(ACCESSIBILITY_SERVICE) as AccessibilityManager
        val component = ComponentName(this, ReaderAccessibilityService::class.java)
        val enabled = manager.getEnabledAccessibilityServiceList(
            AccessibilityServiceInfo.FEEDBACK_ALL_MASK
        ).any {
            it.resolveInfo.serviceInfo.packageName == packageName &&
                it.resolveInfo.serviceInfo.name == component.className
        }
        accessibilityStatus.text = if (enabled) "已启用 kvm 无障碍服务。" else
            "尚未启用。点击按钮后在系统设置中手动开启；返回本应用会自动刷新状态。"
    }

    private fun label(text: String, size: Float, bold: Boolean = false) = TextView(this).apply {
        this.text = text
        setTextSize(TypedValue.COMPLEX_UNIT_SP, size)
        if (bold) setTypeface(typeface, Typeface.BOLD)
    }
    private fun button(text: String, action: () -> Unit) = Button(this).apply {
        this.text = text
        setOnClickListener { action() }
        layoutParams = matchWrap()
    }
    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
    private fun matchWrap() = LinearLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
    )
    private fun TextView.marginTop(value: Int): TextView = apply {
        layoutParams = matchWrap().apply { topMargin = dp(value) }
    }
}
