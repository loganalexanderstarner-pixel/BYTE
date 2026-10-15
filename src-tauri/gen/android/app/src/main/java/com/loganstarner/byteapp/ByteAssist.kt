package com.loganstarner.byteapp

import android.app.assist.AssistContent
import android.app.assist.AssistStructure
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.service.voice.VoiceInteractionService
import android.service.voice.VoiceInteractionSession
import android.service.voice.VoiceInteractionSessionService
import android.speech.RecognitionService
import android.speech.SpeechRecognizer
import android.text.InputType
import android.view.View

/**
 * BYTE as the phone's assistant (Settings → Apps → Default apps → Digital assistant): the assist gesture (hold the power
 * button, swipe from a corner) opens BYTE on a new chat whose box holds "What's on my screen?" and the words visible in
 * the app underneath. It is only put in the box: nothing is sent until the user presses Send, and the model that answers
 * is the one on this phone (or BYTE Cloud if that is what the user chose).
 */
class ByteAssistService : VoiceInteractionService()

class ByteSessionService : VoiceInteractionSessionService() {
  override fun onNewSession(args: Bundle?): VoiceInteractionSession = ByteSession(this)
}

/** Android lists an assistant only if it names a recognition service; BYTE listens inside its own screens, so this does nothing. */
class ByteRecognitionService : RecognitionService() {
  override fun onStartListening(recognizerIntent: Intent?, listener: Callback?) {
    listener?.error(SpeechRecognizer.ERROR_CLIENT)
  }

  override fun onCancel(listener: Callback?) {}

  override fun onStopListening(listener: Callback?) {}
}

class ByteSession(private val context: Context) : VoiceInteractionSession(context) {
  private var opened = false

  override fun onShow(args: Bundle?, showFlags: Int) {
    super.onShow(args, showFlags)
    opened = false
    // If the phone gives no screen contents (the user turned "use text from screen" off), still open BYTE.
    Handler(Looper.getMainLooper()).postDelayed({ if (!opened) open("") }, 800)
  }

  @Suppress("OVERRIDE_DEPRECATION")
  override fun onHandleAssist(data: Bundle?, structure: AssistStructure?, content: AssistContent?) {
    super.onHandleAssist(data, structure, content)
    open(screenText(structure))
  }

  private fun open(screen: String) {
    if (opened) return
    opened = true
    val text = if (screen.isBlank()) "" else "What's on my screen?\n\n$screen"
    val intent = Intent(context, MainActivity::class.java)
      .setAction(Intent.ACTION_SEND)
      .setType("text/plain")
      .putExtra(Intent.EXTRA_TEXT, text)
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
    startAssistantActivity(intent)
    hide()
  }

  /** The words on screen, top to bottom: no passwords, nothing the app marked private, not BYTE itself, at most 6000 characters. */
  private fun screenText(structure: AssistStructure?): String {
    if (structure == null) return ""
    val seen = LinkedHashSet<String>()
    var size = 0
    fun walk(node: AssistStructure.ViewNode) {
      if (size > MAX || node.visibility != View.VISIBLE || node.isAssistBlocked) return
      val password = (node.inputType and InputType.TYPE_TEXT_VARIATION_PASSWORD) != 0
      if (!password) {
        for (s in listOf(node.text?.toString(), node.contentDescription?.toString())) {
          val t = s?.trim()
          if (!t.isNullOrEmpty() && seen.add(t)) size += t.length + 1
        }
      }
      for (i in 0 until node.childCount) walk(node.getChildAt(i))
    }
    for (i in 0 until structure.windowNodeCount) {
      val window = structure.getWindowNodeAt(i)
      if (window.title?.toString()?.contains(context.packageName) == true) continue
      walk(window.rootViewNode)
    }
    return seen.joinToString("\n").take(MAX)
  }

  companion object {
    const val MAX = 6000
  }
}
