package com.loganstarner.byteapp

import android.app.Activity
import android.content.Intent
import android.provider.Settings
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/**
 * Text sent to BYTE from the share sheet ("Share → BYTE") and the home-screen widget ("Ask BYTE"). The text waits here
 * until the page asks for it (`take`); it goes into a new chat's box and is never sent by itself. An empty text (the
 * widget) just means "start a new chat".
 */
@TauriPlugin
class SharePlugin(private val activity: Activity) : Plugin(activity) {
  @Volatile private var pending: String? = null

  override fun load(webView: WebView) {
    grab(activity.intent)
  }

  override fun onNewIntent(intent: Intent) {
    grab(intent)
  }

  private fun grab(intent: Intent?) {
    when (intent?.action) {
      Intent.ACTION_SEND -> if (intent.type?.startsWith("text/") == true) pending = (intent.getStringExtra(Intent.EXTRA_TEXT) ?: "").take(8000)
      // "Select text → BYTE": the selection goes into the box for the user to add a question to.
      Intent.ACTION_PROCESS_TEXT -> pending = (intent.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT)?.toString() ?: "").take(8000)
      // The assist gesture without a session: just a new chat.
      Intent.ACTION_ASSIST -> pending = ""
      else -> return
    }
    // Handled: a later resume of the same intent must not ask again.
    intent.action = Intent.ACTION_MAIN
  }

  /** Whether BYTE is the phone's default digital assistant. */
  @Command
  fun assistantStatus(invoke: Invoke) {
    val current = try {
      Settings.Secure.getString(activity.contentResolver, "assistant")
    } catch (_: Exception) {
      null
    }
    val r = JSObject()
    r.put("isDefault", current?.contains(activity.packageName) == true)
    invoke.resolve(r)
  }

  /** Opens Android's default-assistant settings (the user picks BYTE there). */
  @Command
  fun openAssistantSettings(invoke: Invoke) {
    try {
      activity.startActivity(Intent(Settings.ACTION_VOICE_INPUT_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    } catch (_: Exception) {
      activity.startActivity(Intent(Settings.ACTION_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }
    invoke.resolve(JSObject())
  }

  /** The waiting text (null: nothing was shared), once. */
  @Command
  fun take(invoke: Invoke) {
    val r = JSObject()
    pending?.let { r.put("text", it) }
    pending = null
    invoke.resolve(r)
  }
}
