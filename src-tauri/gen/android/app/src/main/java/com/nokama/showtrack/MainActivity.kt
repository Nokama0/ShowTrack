package com.nokama.showtrack

import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Android 15 forces edge-to-edge for targetSdk 35+: without this the WebView
    // renders under the status bar. Top/side insets are padded natively; the
    // bottom stays 0 so the tab bar reaches the screen edge — the web layer
    // compensates with env(safe-area-inset-bottom) (viewport-fit=cover).
    ViewCompat.setOnApplyWindowInsetsListener(findViewById(android.R.id.content)) { v, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
      v.setPadding(bars.left, bars.top, bars.right, 0)
      insets
    }
  }
}
