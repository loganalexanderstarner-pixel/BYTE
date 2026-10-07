package com.loganstarner.byteapp

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.ContactsContract
import android.telephony.SmsManager
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class ThreadsArgs {
  var limit: Int = 60
}

@InvokeArg
class ThreadArgs {
  lateinit var id: String
  var limit: Int = 40
}

@InvokeArg
class SendArgs {
  lateinit var to: String
  lateinit var text: String
}

@InvokeArg
class SinceArgs {
  var after: Long = 0
}

/**
 * Text messages for BYTE's Messages inbox (messages.rs). Reads the system SMS store (READ_SMS), looks names up in
 * Contacts (READ_CONTACTS) and sends with SmsManager (SEND_SMS). Everything stays on the phone; Rust decides when
 * any of it is called (the inbox is off by default, kids mode and the lock close it).
 */
@TauriPlugin(
  permissions = [
    Permission(strings = [Manifest.permission.READ_SMS], alias = "read"),
    Permission(strings = [Manifest.permission.SEND_SMS], alias = "send"),
    Permission(strings = [Manifest.permission.READ_CONTACTS], alias = "contacts"),
  ]
)
class SmsPlugin(private val activity: Activity) : Plugin(activity) {
  private val sms: Uri = Uri.parse("content://sms")
  private val names = HashMap<String, String>()

  private fun has(permission: String) =
    ContextCompat.checkSelfPermission(activity, permission) == PackageManager.PERMISSION_GRANTED

  /** What the user has allowed, so the settings screen can say what is missing. */
  @Command
  fun status(invoke: Invoke) {
    val r = JSObject()
    r.put("read", has(Manifest.permission.READ_SMS))
    r.put("send", has(Manifest.permission.SEND_SMS))
    r.put("contacts", has(Manifest.permission.READ_CONTACTS))
    invoke.resolve(r)
  }

  private fun nameFor(address: String): String {
    names[address]?.let { return it }
    var name = address
    if (address.isNotBlank() && has(Manifest.permission.READ_CONTACTS)) {
      try {
        val uri = Uri.withAppendedPath(ContactsContract.PhoneLookup.CONTENT_FILTER_URI, Uri.encode(address))
        activity.contentResolver.query(uri, arrayOf(ContactsContract.PhoneLookup.DISPLAY_NAME), null, null, null)?.use {
          if (it.moveToFirst()) name = it.getString(0) ?: address
        }
      } catch (_: Exception) {
        // A number Contacts can't look up stays a number.
      }
    }
    names[address] = name
    return name
  }

  /** Conversations, newest first (one row per thread: its newest text). */
  @Command
  fun threads(invoke: Invoke) {
    if (!has(Manifest.permission.READ_SMS)) {
      invoke.reject("BYTE isn't allowed to read text messages yet.")
      return
    }
    val args = invoke.parseArgs(ThreadsArgs::class.java)
    val seen = LinkedHashSet<Long>()
    val unread = HashSet<Long>()
    val out = JSArray()
    try {
      activity.contentResolver.query(
        sms, arrayOf("_id", "thread_id", "address", "body", "date", "type", "read"), null, null, "date DESC LIMIT 4000",
      )?.use { c ->
        // A first pass over the newest rows finds which threads have unread texts.
        val rows = ArrayList<Array<Any?>>()
        while (c.moveToNext()) {
          rows.add(arrayOf<Any?>(c.getLong(1), c.getString(2), c.getString(3), c.getLong(4), c.getInt(5), c.getInt(6)))
          if (c.getInt(6) == 0 && c.getInt(5) == 1) unread.add(c.getLong(1))
        }
        for (r in rows) {
          val thread = r[0] as Long
          if (!seen.add(thread)) continue
          if (seen.size > args.limit) break
          val address = (r[1] as String?) ?: ""
          val o = JSObject()
          o.put("chat", thread.toString())
          o.put("name", nameFor(address))
          o.put("handle", address)
          o.put("group", false)
          o.put("lastText", (r[2] as String?) ?: "")
          o.put("lastAt", r[3] as Long)
          o.put("lastFromMe", (r[4] as Int) != 1)
          o.put("unread", unread.contains(thread))
          out.put(o)
        }
      }
    } catch (e: Exception) {
      invoke.reject("Couldn't read the text messages: ${e.message}")
      return
    }
    val r = JSObject()
    r.put("threads", out)
    invoke.resolve(r)
  }

  /** One conversation, oldest first. */
  @Command
  fun thread(invoke: Invoke) {
    if (!has(Manifest.permission.READ_SMS)) {
      invoke.reject("BYTE isn't allowed to read text messages yet.")
      return
    }
    val args = invoke.parseArgs(ThreadArgs::class.java)
    val rows = ArrayList<JSObject>()
    try {
      activity.contentResolver.query(
        sms, arrayOf("address", "body", "date", "type"), "thread_id = ?", arrayOf(args.id), "date DESC LIMIT ${args.limit}",
      )?.use { c ->
        while (c.moveToNext()) {
          val address = c.getString(0) ?: ""
          val fromMe = c.getInt(3) != 1
          val o = JSObject()
          o.put("text", c.getString(1) ?: "")
          o.put("at", c.getLong(2))
          o.put("fromMe", fromMe)
          o.put("sender", if (fromMe) "Me" else nameFor(address))
          rows.add(o)
        }
      }
    } catch (e: Exception) {
      invoke.reject("Couldn't read that conversation: ${e.message}")
      return
    }
    val out = JSArray()
    for (o in rows.reversed()) out.put(o)
    val r = JSObject()
    r.put("messages", out)
    invoke.resolve(r)
  }

  /** Sends a text (the user pressed Send: Rust only calls this after that). */
  @Command
  fun send(invoke: Invoke) {
    if (!has(Manifest.permission.SEND_SMS)) {
      invoke.reject("BYTE isn't allowed to send text messages yet.")
      return
    }
    val args = invoke.parseArgs(SendArgs::class.java)
    try {
      val manager: SmsManager =
        if (Build.VERSION.SDK_INT >= 31) activity.getSystemService(SmsManager::class.java)
        else @Suppress("DEPRECATION") SmsManager.getDefault()
      val parts = manager.divideMessage(args.text)
      if (parts.size > 1) manager.sendMultipartTextMessage(args.to, null, parts, null, null)
      else manager.sendTextMessage(args.to, null, args.text, null, null)
    } catch (e: Exception) {
      invoke.reject("Couldn't send the text: ${e.message}")
      return
    }
    invoke.resolve(JSObject())
  }

  /** Texts received after id `after` (oldest first), and the newest id there is (for the first look). */
  @Command
  fun newSince(invoke: Invoke) {
    if (!has(Manifest.permission.READ_SMS)) {
      invoke.reject("BYTE isn't allowed to read text messages yet.")
      return
    }
    val args = invoke.parseArgs(SinceArgs::class.java)
    val out = JSArray()
    var newest = 0L
    try {
      activity.contentResolver.query(sms, arrayOf("_id"), null, null, "_id DESC LIMIT 1")?.use {
        if (it.moveToFirst()) newest = it.getLong(0)
      }
      activity.contentResolver.query(
        sms, arrayOf("_id", "thread_id", "address", "body", "date"), "_id > ? AND type = 1", arrayOf(args.after.toString()),
        "_id ASC LIMIT 50",
      )?.use { c ->
        while (c.moveToNext()) {
          val address = c.getString(2) ?: ""
          val o = JSObject()
          o.put("chat", c.getLong(1).toString())
          o.put("name", nameFor(address))
          o.put("text", c.getString(3) ?: "")
          o.put("at", c.getLong(4))
          out.put(o)
        }
      }
    } catch (e: Exception) {
      invoke.reject("Couldn't check for new texts: ${e.message}")
      return
    }
    val r = JSObject()
    r.put("texts", out)
    r.put("newest", newest)
    invoke.resolve(r)
  }
}
