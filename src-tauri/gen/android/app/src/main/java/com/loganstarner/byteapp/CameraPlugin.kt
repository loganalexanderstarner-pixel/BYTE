package com.loganstarner.byteapp

import android.app.Activity
import android.content.Intent
import android.provider.MediaStore
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

/**
 * "Take a photo" for a chat: the phone's own camera app takes it (so BYTE needs no camera permission) and hands the picture
 * back through BYTE's file provider into the app's cache. The page then attaches it like any photo.
 */
@TauriPlugin
class CameraPlugin(private val activity: Activity) : Plugin(activity) {
  private var file: File? = null

  @Command
  fun capture(invoke: Invoke) {
    val dir = File(activity.cacheDir, "camera").apply { mkdirs() }
    // Old photos are not kept.
    dir.listFiles()?.forEach { it.delete() }
    val photo = File(dir, "photo-${System.currentTimeMillis()}.jpg")
    file = photo
    val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", photo)
    val intent = Intent(MediaStore.ACTION_IMAGE_CAPTURE)
      .putExtra(MediaStore.EXTRA_OUTPUT, uri)
      .addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION)
    try {
      startActivityForResult(invoke, intent, "captured")
    } catch (e: Exception) {
      invoke.reject("No camera app was found on this phone.")
    }
  }

  @ActivityCallback
  fun captured(invoke: Invoke, result: ActivityResult) {
    val photo = file
    val r = JSObject()
    if (result.resultCode == Activity.RESULT_OK && photo != null && photo.length() > 0) {
      r.put("path", photo.absolutePath)
    } else {
      photo?.delete()
    }
    invoke.resolve(r)
  }
}
