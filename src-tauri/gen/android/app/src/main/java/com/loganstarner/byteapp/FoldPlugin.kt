package com.loganstarner.byteapp

import android.app.Activity
import android.webkit.WebView
import androidx.core.content.ContextCompat
import androidx.core.util.Consumer
import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowInfoTracker
import androidx.window.layout.WindowLayoutInfo
import androidx.window.java.layout.WindowInfoTrackerCallbackAdapter
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Plugin

/**
 * Tells the page how a foldable is folded: `data-fold` on <html> is "tabletop" (half-open with a horizontal hinge: the
 * chat can sit in the top half and the keyboard and box in the bottom one), "book" (half-open, vertical hinge) or
 * "flat", with the hinge's top and height in CSS pixels as `--hinge-top` and `--hinge-h`. The page decides what to do (app.css).
 */
@TauriPlugin
class FoldPlugin(private val activity: Activity) : Plugin(activity) {
  private var web: WebView? = null
  private var last = ""

  override fun load(webView: WebView) {
    web = webView
    val tracker = WindowInfoTrackerCallbackAdapter(WindowInfoTracker.getOrCreate(activity))
    tracker.addWindowLayoutInfoListener(activity, ContextCompat.getMainExecutor(activity), Consumer<WindowLayoutInfo> { push(it) })
  }

  private fun push(info: WindowLayoutInfo) {
    val view = web ?: return
    val fold = info.displayFeatures.filterIsInstance<FoldingFeature>().firstOrNull()
    var posture = "flat"
    var top = 0
    var height = 0
    if (fold != null && fold.state == FoldingFeature.State.HALF_OPENED) {
      posture = if (fold.orientation == FoldingFeature.Orientation.HORIZONTAL) "tabletop" else "book"
      val density = view.resources.displayMetrics.density
      val where = IntArray(2)
      view.getLocationInWindow(where)
      top = Math.round((fold.bounds.top - where[1]) / density)
      height = Math.round(fold.bounds.height() / density)
    }
    val js = "(function(){var d=document.documentElement;d.dataset.fold='$posture';" +
      "d.style.setProperty('--hinge-top','${top}px');d.style.setProperty('--hinge-h','${height}px');})()"
    if (js == last) return
    last = js
    view.post { view.evaluateJavascript(js, null) }
  }
}
