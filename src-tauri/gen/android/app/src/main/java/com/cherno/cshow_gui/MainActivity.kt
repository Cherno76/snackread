package com.cherno.cshow_gui

import android.content.Intent
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.net.Uri

class MainActivity : TauriActivity() {
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
}
