package org.emcommlog.tablet

import android.graphics.Color
import android.os.Bundle
import android.os.Process
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import app.tauri.plugin.PluginManager

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // Draw edge to edge (required from Android 15) with light icons on the app's dark chrome;
    // EmcommPlugin pads the content by the system-bar and keyboard insets.
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)
    window.decorView.setBackgroundColor(Color.parseColor("#08131F"))
  }

  // Tauri 2.11 generates a lifecycle observer for plugins but never registers it, so forward the
  // events EmcommPlugin needs (background backup on stop, storage re-check on resume) directly.
  override fun onResume() {
    super.onResume()
    PluginManager.onResume()
  }

  override fun onPause() {
    super.onPause()
    PluginManager.onPause()
  }

  override fun onStop() {
    super.onStop()
    PluginManager.onStop()
  }

  override fun onDestroy() {
    val finishing = isFinishing && !isChangingConfigurations
    super.onDestroy()
    // The Rust core and its single webview live for the whole process, and a new activity in a
    // cached process would get no webview (a blank screen). When the activity really finishes
    // (Back/exit, swipe from Recents), end the process so the next launch starts cleanly. Saved
    // entries are already durable and drafts are flushed when the app leaves the foreground;
    // the next open runs the stale-lock integrity check.
    if (finishing) Process.killProcess(Process.myPid())
  }
}
