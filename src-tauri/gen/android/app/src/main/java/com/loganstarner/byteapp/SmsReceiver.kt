package com.loganstarner.byteapp

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.provider.ContactsContract
import android.net.Uri
import android.provider.Telephony
import androidx.core.app.NotificationCompat

/**
 * Shows a notification for a text that arrives while BYTE is closed (the inbox's "Notify me" switch). The phone starts
 * this receiver for every incoming text; it does nothing unless BYTE's inbox has switched it on (`SmsPlugin.setBackground`),
 * reads nothing but the text that arrived, and keeps nothing. The words are hidden on the lock screen.
 */
class SmsReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    if (intent.action != Telephony.Sms.Intents.SMS_RECEIVED_ACTION) return
    val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    if (!prefs.getBoolean(KEY_ON, false)) return
    val parts = Telephony.Sms.Intents.getMessagesFromIntent(intent) ?: return
    if (parts.isEmpty()) return
    val address = parts[0].originatingAddress ?: ""
    val body = parts.joinToString("") { it.messageBody ?: "" }
    val who = nameFor(context, address)

    val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    if (Build.VERSION.SDK_INT >= 26) {
      manager.createNotificationChannel(NotificationChannel(CHANNEL, "Text messages", NotificationManager.IMPORTANCE_HIGH))
    }
    val open = PendingIntent.getActivity(
      context, 0, Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
      PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )
    val hidden = NotificationCompat.Builder(context, CHANNEL)
      .setSmallIcon(android.R.drawable.ic_dialog_email)
      .setContentTitle(who)
      .setContentText("New text message")
      .build()
    val note = NotificationCompat.Builder(context, CHANNEL)
      .setSmallIcon(android.R.drawable.ic_dialog_email)
      .setContentTitle(who)
      .setContentText(body)
      .setStyle(NotificationCompat.BigTextStyle().bigText(body))
      .setAutoCancel(true)
      .setContentIntent(open)
      .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
      .setPublicVersion(hidden)
      .build()
    try {
      manager.notify(address.hashCode(), note)
    } catch (_: SecurityException) {
      // Notifications aren't allowed (Android 13+): nothing to show.
    }
  }

  private fun nameFor(context: Context, address: String): String {
    if (address.isBlank()) return "New text message"
    try {
      val uri = Uri.withAppendedPath(ContactsContract.PhoneLookup.CONTENT_FILTER_URI, Uri.encode(address))
      context.contentResolver.query(uri, arrayOf(ContactsContract.PhoneLookup.DISPLAY_NAME), null, null, null)?.use {
        if (it.moveToFirst()) return it.getString(0) ?: address
      }
    } catch (_: Exception) {
      // No contacts permission or no match: the number is the name.
    }
    return address
  }

  companion object {
    const val PREFS = "byte_texts"
    const val KEY_ON = "notify"
    const val CHANNEL = "texts"
  }
}
