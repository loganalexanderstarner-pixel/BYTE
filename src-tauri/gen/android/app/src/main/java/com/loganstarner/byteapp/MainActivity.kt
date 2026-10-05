package com.loganstarner.byteapp

import android.content.res.Configuration
import android.os.Bundle
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

    // Android 15 forces edge-to-edge: the app draws under the status bar, the
    // navigation bar and the keyboard. CSS env(safe-area-inset-*) is not reliable
    // in a WebView, so the top bar sat under the status bar and taps on it went to
    // the system, and the keyboard covered the composer. Pad the whole content view
    // by the real insets instead; the page then lays out in the space that is left.
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or
          WindowInsetsCompat.Type.displayCutout() or
          WindowInsetsCompat.Type.ime()
      )
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
  }

  // Folding changes the size and the density together. The activity is kept (see
  // configChanges in the manifest), so tell the view to lay out again.
  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    findViewById<View>(android.R.id.content)?.let {
      it.requestLayout()
      ViewCompat.requestApplyInsets(it)
    }
  }
}
