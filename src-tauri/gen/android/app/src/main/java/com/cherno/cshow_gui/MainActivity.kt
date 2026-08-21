package com.cherno.cshow_gui

import android.content.Context
import android.content.BroadcastReceiver
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.BatteryManager
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.net.Uri
import android.webkit.JavascriptInterface
import android.webkit.WebView

class MainActivity : TauriActivity() {
  private var batteryReceiver: BroadcastReceiver? = null
  private var ttsBridge: TtsBridge? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    // 读取手机存储里的书库需要「所有文件访问」权限；未授权时引导去设置页开启
    if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R &&
        !Environment.isExternalStorageManager()
    ) {
      try {
        startActivity(
          Intent(
            Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
            Uri.parse("package:$packageName")
          )
        )
      } catch (_: Exception) {
        startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
      }
    }
  }

  // 标题栏状态桥：JS 读取电池电量与网络类型（Wi-Fi / 蜂窝），只读、无副作用
  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webView.post {
      webView.addJavascriptInterface(StatusBridge(this), "AndroidStatus")
      // 朗读（TTS）桥：在 WebView 主线程创建 TextToSpeech（JS 接口回调线程没有 Looper）
      val bridge = TtsBridge(this, webView)
      ttsBridge = bridge
      webView.addJavascriptInterface(bridge, "AndroidTts")
      // 电池插拔/电量变化时立即通知前端刷新标题栏状态
      batteryReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
          webView.evaluateJavascript("window.dispatchEvent(new Event('statuschange'))", null)
        }
      }
      runCatching {
        registerReceiver(batteryReceiver, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
      }
    }
  }

  override fun onDestroy() {
    super.onDestroy()
    batteryReceiver?.let { runCatching { unregisterReceiver(it) } }
    batteryReceiver = null
    ttsBridge?.destroy()
    ttsBridge = null
  }

  class StatusBridge(private val activity: MainActivity) {
    @JavascriptInterface
    fun getStatus(): String {
      var battery = -1
      var charging = false
      try {
        val bm = activity.getSystemService(Context.BATTERY_SERVICE) as BatteryManager
        battery = bm.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)
        val status = bm.getIntProperty(BatteryManager.BATTERY_PROPERTY_STATUS)
        charging = status == BatteryManager.BATTERY_STATUS_CHARGING ||
          status == BatteryManager.BATTERY_STATUS_FULL
      } catch (_: Exception) {
      }
      var wifi = false
      try {
        val cm = activity.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val caps = cm.getNetworkCapabilities(cm.activeNetwork)
        wifi = caps?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) == true
      } catch (_: Exception) {
      }
      return "{\"battery\":$battery,\"wifi\":$wifi,\"charging\":$charging}"
    }
  }
}
