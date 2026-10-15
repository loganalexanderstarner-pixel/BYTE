package com.loganstarner.byteapp

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.content.Intent
import android.widget.RemoteViews

/** The home-screen "Ask BYTE" bar: tapping it opens BYTE on a new chat with the box ready (SharePlugin, empty text). */
class AskWidget : AppWidgetProvider() {
  override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
    val open = Intent(context, MainActivity::class.java)
      .setAction(Intent.ACTION_SEND)
      .setType("text/plain")
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
    val tap = PendingIntent.getActivity(context, 0, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    for (id in ids) {
      val views = RemoteViews(context.packageName, R.layout.ask_widget)
      views.setOnClickPendingIntent(R.id.ask_widget_root, tap)
      manager.updateAppWidget(id, views)
    }
  }
}
