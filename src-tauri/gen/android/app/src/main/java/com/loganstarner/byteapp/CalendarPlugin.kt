package com.loganstarner.byteapp

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.provider.CalendarContract
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
class EventsArgs {
  var from: Long = 0
  var to: Long = 0
}

/** Reads the phone's calendar (READ_CALENDAR) for the daily briefing. Nothing is written or kept. */
@TauriPlugin(permissions = [Permission(strings = [Manifest.permission.READ_CALENDAR], alias = "read")])
class CalendarPlugin(private val activity: Activity) : Plugin(activity) {
  private fun allowed() =
    ContextCompat.checkSelfPermission(activity, Manifest.permission.READ_CALENDAR) == PackageManager.PERMISSION_GRANTED

  @Command
  fun status(invoke: Invoke) {
    val r = JSObject()
    r.put("read", allowed())
    invoke.resolve(r)
  }

  /** Events that overlap [from, to) (ms since 1970), earliest first. */
  @Command
  fun events(invoke: Invoke) {
    if (!allowed()) {
      invoke.reject("BYTE isn't allowed to read the calendar yet.")
      return
    }
    val args = invoke.parseArgs(EventsArgs::class.java)
    val out = JSArray()
    try {
      val uri = CalendarContract.Instances.CONTENT_URI.buildUpon()
        .appendPath(args.from.toString()).appendPath(args.to.toString()).build()
      activity.contentResolver.query(
        uri,
        arrayOf(
          CalendarContract.Instances.TITLE, CalendarContract.Instances.BEGIN, CalendarContract.Instances.END,
          CalendarContract.Instances.ALL_DAY, CalendarContract.Instances.CALENDAR_DISPLAY_NAME,
        ),
        null, null, "${CalendarContract.Instances.BEGIN} ASC",
      )?.use { c ->
        while (c.moveToNext() && out.length() < 60) {
          val o = JSObject()
          o.put("title", c.getString(0) ?: "")
          o.put("start", c.getLong(1))
          o.put("end", c.getLong(2))
          o.put("allDay", c.getInt(3) == 1)
          o.put("calendar", c.getString(4) ?: "")
          out.put(o)
        }
      }
    } catch (e: Exception) {
      invoke.reject("Couldn't read the calendar: ${e.message}")
      return
    }
    val r = JSObject()
    r.put("events", out)
    invoke.resolve(r)
  }
}
