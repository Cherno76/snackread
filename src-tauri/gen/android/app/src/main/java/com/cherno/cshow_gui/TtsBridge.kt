package com.cherno.cshow_gui

import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.webkit.JavascriptInterface
import android.webkit.WebView
import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale

/**
 * 朗读（TTS）桥：把 Android 系统 TextToSpeech（Google/讯飞/三星等引擎）暴露给 WebView JS。
 *
 * 设计要点（参考 KOReader 生态在 Android 上的做法）：
 * - TextToSpeech 的初始化与所有引擎调用都放在主线程（JS 接口回调跑在 WebView 后台线程，
 *   直接调 TTS 会有线程/Looper 问题）；JS 侧只做“发请求 + 收事件”的异步轮询/回调。
 * - 事件（start/done/error/range）经 evaluateJavascript 回传给页面，JS 据此逐句推进。
 * - 暂停/继续由 JS 层实现：stop() 当前句 + 从已读到的字符偏移重新朗读，不依赖引擎的暂停能力。
 *
 * JS 用法：
 *   window.AndroidTts.speak(text, lang, rate, pitch, utteranceId) -> bool
 *   window.AndroidTts.stop()
 *   window.AndroidTts.getInfo() -> JSON 字符串
 *   window.__ttsEvent(type, payload) 事件：init / start / done / error / range
 */
class TtsBridge(private val context: Context, private val webView: WebView) : TextToSpeech.OnInitListener {

    private val main = Handler(Looper.getMainLooper())
    private var tts: TextToSpeech? = null

    /** -2=初始化中 0=成功 其他=错误码。注意不能用 -1 当“初始化中”：
     *  TextToSpeech.ERROR 恰好也是 -1，会与“初始化中”混淆导致永不重试 */
    @Volatile private var initState = -2
    @Volatile private var engine: String? = null
    @Volatile private var voicesJson = "[]"
    @Volatile private var zhVoice = false
    private var lastInitAttempt = 0L

    init {
        // 必须在主线程创建（内部使用 Handler/回调）
        ensureTts()
    }

    private fun ensureTts() {
        // JS 接口回调跑在 WebView 后台线程（无 Looper），TextToSpeech 只能在主线程创建，
        // 否则初始化永远收不到回调（表现为 getInfo() 一直 pending → JS 提示引擎不可用）
        if (Looper.myLooper() != Looper.getMainLooper()) {
            main.post { ensureTts() }
            return
        }
        // 未初始化或上次初始化失败（如引擎后装）时重建；限 1.5s 一次，避免轮询刷爆绑定
        val now = System.currentTimeMillis()
        if ((tts == null || initState != TextToSpeech.SUCCESS) && now - lastInitAttempt > 1500) {
            lastInitAttempt = now
            runCatching { tts?.shutdown() }
            initState = -2
            tts = TextToSpeech(context.applicationContext, this)
        }
    }

    override fun onInit(status: Int) {
        initState = status
        val t = tts
        if (status == TextToSpeech.SUCCESS && t != null) {
            engine = try { t.defaultEngine } catch (_: Exception) { null }
            try {
                val arr = JSONArray()
                var zh = false
                for (v in t.voices) {
                    arr.put(JSONObject().put("name", v.name).put("lang", v.locale.toLanguageTag()))
                    if (v.locale.language.equals("zh", ignoreCase = true)) zh = true
                }
                zhVoice = zh
                voicesJson = arr.toString()
            } catch (_: Exception) { /* voices 可空/未就绪时保持空列表 */ }
            t.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
                override fun onStart(utteranceId: String?) {
                    emit("start", utteranceId)
                }

                override fun onDone(utteranceId: String?) {
                    emit("done", utteranceId)
                }

                @Deprecated("Deprecated in Java")
                override fun onError(utteranceId: String?) {
                    emit("error", utteranceId)
                }

                override fun onRangeStart(utteranceId: String?, start: Int, end: Int, frame: Int) {
                    if (Build.VERSION.SDK_INT >= 26) {
                        emit("range", utteranceId, JSONArray().put(start).put(end).toString())
                    }
                }
            })
        }
        emit("init", status.toString())
    }

    private fun emit(type: String, payload: String?) {
        emit(type, payload, null)
    }

    private fun emit(type: String, payload: String?, extra: String?) {
        val safeType = JSONObject.quote(type)
        val safePayload = payload?.let { JSONObject.quote(it) } ?: "null"
        val extraJson = extra ?: "null"
        val js = "window.__ttsEvent && window.__ttsEvent($safeType,$safePayload,$extraJson)"
        main.post {
            runCatching { webView.evaluateJavascript(js, null) }
        }
    }

    /** 朗读一段文本；text 为空或引擎未就绪返回 false。QUEUE_FLUSH：逐句调用，天然覆盖上一句。 */
    @JavascriptInterface
    fun speak(text: String?, lang: String?, rate: Float, pitch: Float, utteranceId: String?): Boolean {
        val trimmed = text?.trim().orEmpty()
        if (trimmed.isEmpty()) return false
        if (tts == null || initState != TextToSpeech.SUCCESS) {
            ensureTts() // 引擎后装/换引擎：下次调用时自动重试初始化
            return false
        }
        val rateOk = if (rate > 0f) rate else 1f
        val pitchOk = if (pitch > 0f) pitch else 1f
        main.post {
            val t = tts ?: return@post
            try {
                t.setSpeechRate(rateOk)
                t.setPitch(pitchOk)
                if (!lang.isNullOrBlank()) {
                    t.setLanguage(Locale.forLanguageTag(lang))
                }
                t.speak(trimmed, TextToSpeech.QUEUE_FLUSH, null, utteranceId)
            } catch (_: Exception) { /* 引擎异常时静默失败，JS 侧按 error 超时处理 */ }
        }
        return true
    }

    @JavascriptInterface
    fun stop() {
        main.post {
            runCatching { tts?.stop() }
        }
    }

    /** 打开系统「文字转语音」设置页（引擎缺失时引导用户安装/切换引擎） */
    @JavascriptInterface
    fun openTtsSettings() {
        main.post {
            try {
                context.startActivity(
                    Intent("android.settings.TTS_SETTINGS").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                )
            } catch (_: Exception) {
                try {
                    context.startActivity(
                        Intent(TextToSpeech.Engine.ACTION_INSTALL_TTS_DATA)
                            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                    )
                } catch (_: Exception) { /* 打不开设置页时静默 */ }
            }
        }
    }

    /** 释放引擎（应用退出/不再使用时调用；之后可重新 speak 自动重建） */
    @JavascriptInterface
    fun shutdown() {
        main.post {
            runCatching { tts?.shutdown() }
            tts = null
            initState = -2
        }
    }

    /** 查询引擎状态与可用声音（JSON 字符串，供前端提示/后续声音选择） */
    @JavascriptInterface
    fun getInfo(): String {
        if (initState != TextToSpeech.SUCCESS && initState != -2) ensureTts() // 失败后自动重建
        val o = JSONObject()
        o.put("ready", initState == TextToSpeech.SUCCESS && tts != null)
        o.put("pending", initState == -2)
        o.put("error", if (initState == TextToSpeech.SUCCESS || initState == -2) 0 else initState)
        o.put("engine", engine ?: "")
        o.put("zh", zhVoice)
        try {
            o.put("voices", JSONArray(voicesJson))
        } catch (_: Exception) {
            o.put("voices", JSONArray())
        }
        return o.toString()
    }

    fun destroy() {
        runCatching { tts?.shutdown() }
        tts = null
        initState = -2
    }
}
