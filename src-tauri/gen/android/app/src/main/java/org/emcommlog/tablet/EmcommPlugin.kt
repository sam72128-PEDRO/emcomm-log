package org.emcommlog.tablet

import android.Manifest
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Color
import android.location.Location
import android.location.LocationManager
import android.net.Uri
import android.os.CancellationSignal
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.OpenableColumns
import android.view.View
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.ActivityResult
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.FileProvider
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import app.tauri.PermissionState
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.util.UUID

@InvokeArg
class PickArgs {
  var mimeTypes: Array<String>? = null
}

@InvokeArg
class CreateArgs {
  lateinit var name: String
  var mime: String? = null
}

@InvokeArg
class CopyArgs {
  lateinit var path: String
  lateinit var uri: String
}

@InvokeArg
class FileArgs {
  lateinit var path: String
  var mime: String? = null
  var title: String? = null
}

@InvokeArg
class LocationArgs {
  var timeoutS: Long = 30
  var maxAgeS: Long = 10
}

@InvokeArg
class BarArgs {
  var dark: Boolean = true
  var color: String? = null
}

/**
 * Android-only services for EMCOMM LOG: system file picker / "Save to…", share sheet, opening
 * files in other apps, removable-storage (microSD) discovery, window insets (system bars and the
 * on-screen keyboard), the Back button, and lifecycle notifications to the web UI.
 *
 * Every file handed to Rust is a real path in app storage: picked documents are copied into the
 * cache first, and exports are written to the cache and then copied to the chosen document.
 */
@TauriPlugin(
  permissions = [
    Permission(strings = [Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION], alias = "location"),
  ],
)
class EmcommPlugin(private val activity: Activity) : Plugin(activity) {
  private var webView: WebView? = null

  override fun load(webView: WebView) {
    this.webView = webView
    val act = activity as AppCompatActivity

    // Keep the web content clear of the status bar, gesture bar, display cutout and the on-screen
    // keyboard. Resizing the content view (instead of overlaying) lets the page scroll the focused
    // field into view and keeps dialog buttons above the keyboard.
    val content = act.findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { v, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
      v.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, ime.bottom))
      WindowInsetsCompat.CONSUMED
    }
    ViewCompat.requestApplyInsets(content)

    // Back: close the top dialog, go back a screen, or run the app's own exit (flush drafts,
    // close backup). Added after WryActivity's callback, so it takes precedence.
    act.onBackPressedDispatcher.addCallback(act, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        val wv = this@EmcommPlugin.webView
        if (wv == null) {
          act.moveTaskToBack(true)
          return
        }
        wv.evaluateJavascript("(function(){return window.__emcommBack ? window.__emcommBack() : 'none';})()") { r ->
          if (r == null || r.contains("none")) act.moveTaskToBack(true)
        }
      }
    })
  }

  private fun notifyWeb(event: String) {
    webView?.post {
      webView?.evaluateJavascript("window.__emcommLifecycle && window.__emcommLifecycle('$event')", null)
    }
  }

  override fun onStop() {
    notifyWeb("stop")
  }

  override fun onResume() {
    notifyWeb("resume")
  }

  // ---------- open a document ----------

  @Command
  fun pickDocument(invoke: Invoke) {
    val args = invoke.parseArgs(PickArgs::class.java)
    val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
      addCategory(Intent.CATEGORY_OPENABLE)
      type = "*/*"
      val types = args.mimeTypes?.filter { it.isNotBlank() && it != "*/*" }
      if (!types.isNullOrEmpty()) putExtra(Intent.EXTRA_MIME_TYPES, types.toTypedArray())
    }
    startActivityForResult(invoke, intent, "pickDocumentResult")
  }

  @ActivityCallback
  fun pickDocumentResult(invoke: Invoke, result: ActivityResult) {
    val ret = JSObject()
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) {
      ret.put("path", null)
      invoke.resolve(ret)
      return
    }
    Thread {
      try {
        val name = safeName(displayName(uri) ?: "picked-file")
        val dir = File(activity.cacheDir, "incoming/" + UUID.randomUUID().toString())
        dir.mkdirs()
        val out = File(dir, name)
        val input = activity.contentResolver.openInputStream(uri)
          ?: throw IllegalStateException("The selected file could not be read")
        input.use { i -> FileOutputStream(out).use { o -> i.copyTo(o); o.fd.sync() } }
        ret.put("path", out.absolutePath)
        ret.put("name", name)
        invoke.resolve(ret)
      } catch (e: Exception) {
        invoke.reject("Could not read the selected file: ${e.message}")
      }
    }.start()
  }

  // ---------- save to a document ----------

  @Command
  fun createDocument(invoke: Invoke) {
    val args = invoke.parseArgs(CreateArgs::class.java)
    val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
      addCategory(Intent.CATEGORY_OPENABLE)
      type = args.mime ?: "application/octet-stream"
      putExtra(Intent.EXTRA_TITLE, args.name)
    }
    startActivityForResult(invoke, intent, "createDocumentResult")
  }

  @ActivityCallback
  fun createDocumentResult(invoke: Invoke, result: ActivityResult) {
    val ret = JSObject()
    val uri = result.data?.data
    if (result.resultCode == Activity.RESULT_OK && uri != null) {
      ret.put("uri", uri.toString())
      ret.put("name", displayName(uri))
    } else {
      ret.put("uri", null)
    }
    invoke.resolve(ret)
  }

  @Command
  fun copyToUri(invoke: Invoke) {
    val args = invoke.parseArgs(CopyArgs::class.java)
    try {
      val uri = Uri.parse(args.uri)
      val pfd = activity.contentResolver.openFileDescriptor(uri, "wt")
        ?: throw IllegalStateException("The destination could not be opened")
      pfd.use { fd ->
        FileInputStream(File(args.path)).use { i ->
          FileOutputStream(fd.fileDescriptor).use { o ->
            i.copyTo(o)
            o.flush()
            o.fd.sync()
          }
        }
      }
      invoke.resolve(JSObject())
    } catch (e: Exception) {
      invoke.reject("Could not save the file to the chosen location: ${e.message}")
    }
  }

  // ---------- open / share ----------

  private fun contentUri(path: String): Uri =
    FileProvider.getUriForFile(activity, activity.packageName + ".fileprovider", File(path))

  @Command
  fun viewFile(invoke: Invoke) {
    val args = invoke.parseArgs(FileArgs::class.java)
    try {
      val intent = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(contentUri(args.path), args.mime ?: "*/*")
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      }
      activity.startActivity(Intent.createChooser(intent, args.title ?: "Open with"))
      invoke.resolve(JSObject())
    } catch (e: ActivityNotFoundException) {
      invoke.reject("No app on this tablet can open this kind of file")
    } catch (e: Exception) {
      invoke.reject("Could not open the file: ${e.message}")
    }
  }

  @Command
  fun shareFile(invoke: Invoke) {
    val args = invoke.parseArgs(FileArgs::class.java)
    try {
      val intent = Intent(Intent.ACTION_SEND).apply {
        type = args.mime ?: "application/octet-stream"
        putExtra(Intent.EXTRA_STREAM, contentUri(args.path))
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      }
      activity.startActivity(Intent.createChooser(intent, args.title ?: "Share"))
      invoke.resolve(JSObject())
    } catch (e: Exception) {
      invoke.reject("Could not share the file: ${e.message}")
    }
  }

  // ---------- storage ----------

  /** App-specific folders on removable storage (microSD). Real paths; no permission needed. */
  @Command
  fun storageInfo(invoke: Invoke) {
    val list = JSArray()
    for (dir in activity.getExternalFilesDirs(null)) {
      if (dir == null) continue
      val removable = try { Environment.isExternalStorageRemovable(dir) } catch (e: Exception) { false }
      if (!removable) continue
      val o = JSObject()
      o.put("path", dir.absolutePath)
      o.put("state", Environment.getExternalStorageState(dir))
      list.put(o)
    }
    val ret = JSObject()
    ret.put("removable", list)
    ret.put("cache", activity.cacheDir.absolutePath)
    invoke.resolve(ret)
  }

  // ---------- tablet location (one fix on request; never tracking) ----------

  private var locationCancel: CancellationSignal? = null

  /**
   * One position from the tablet's own location services. Asks for permission only when called
   * (Capture / Test). Uses the satellite (GPS) provider when the tablet has one — it works with no
   * network — and otherwise Android's network provider, which is labelled as an OS position.
   */
  @Command
  fun getLocation(invoke: Invoke) {
    if (getPermissionState("location") != PermissionState.GRANTED) {
      requestPermissionForAlias("location", invoke, "locationPermissionResult")
      return
    }
    acquireLocation(invoke)
  }

  @PermissionCallback
  fun locationPermissionResult(invoke: Invoke) {
    if (getPermissionState("location") == PermissionState.GRANTED) {
      acquireLocation(invoke)
    } else {
      invoke.resolve(status("permission_denied", "Location permission was not granted (Settings → Apps → EMCOMM LOG → Permissions → Location)"))
    }
  }

  private fun status(s: String, message: String): JSObject {
    val o = JSObject()
    o.put("status", s)
    o.put("message", message)
    return o
  }

  @Command
  fun cancelLocation(invoke: Invoke) {
    locationCancel?.cancel()
    invoke.resolve(JSObject())
  }

  private fun acquireLocation(invoke: Invoke) {
    val args = invoke.parseArgs(LocationArgs::class.java)
    val lm = activity.getSystemService(LocationManager::class.java)
    if (lm == null || !lm.isLocationEnabled) {
      invoke.resolve(status("unavailable", "Location is turned off on this tablet (Settings → Location)"))
      return
    }
    val providers = lm.getProviders(true)
    val provider = when {
      providers.contains(LocationManager.GPS_PROVIDER) -> LocationManager.GPS_PROVIDER
      providers.contains(LocationManager.NETWORK_PROVIDER) -> LocationManager.NETWORK_PROVIDER
      else -> null
    }
    if (provider == null) {
      invoke.resolve(status("no_device", "This tablet has no usable location provider (no GPS hardware, or location is restricted)"))
      return
    }
    val signal = CancellationSignal()
    locationCancel = signal
    val handler = Handler(Looper.getMainLooper())
    var done = false
    val finish = { o: JSObject ->
      if (!done) {
        done = true
        locationCancel = null
        invoke.resolve(o)
      }
    }
    handler.postDelayed({
      if (!done) {
        signal.cancel()
        finish(status("timed_out", if (provider == LocationManager.GPS_PROVIDER)
          "No satellite fix before the timeout. Go outdoors or near a window with a view of the sky, then try again."
        else "Android did not provide a position before the timeout (no GPS hardware; network location needs a connection)."))
      }
    }, args.timeoutS.coerceIn(5, 600) * 1000)
    signal.setOnCancelListener { handler.post { finish(status("cancelled", "Cancelled")) } }
    try {
      lm.getCurrentLocation(provider, signal, activity.mainExecutor) { loc: Location? ->
        if (loc == null) {
          finish(status("no_fix", "No position is available yet"))
        } else {
          val o = status("fix", "Position received")
          o.put("lat", loc.latitude)
          o.put("lon", loc.longitude)
          o.put("timeMs", loc.time)
          if (loc.hasAccuracy()) o.put("accuracyM", loc.accuracy.toDouble())
          if (loc.hasAltitude()) o.put("altitudeM", loc.altitude)
          val sats = loc.extras?.getInt("satellites", -1) ?: -1
          if (sats > 0) o.put("satellites", sats)
          o.put("provider", loc.provider ?: provider)
          o.put("satellite", provider == LocationManager.GPS_PROVIDER)
          finish(o)
        }
      }
    } catch (e: SecurityException) {
      finish(status("permission_denied", "Location permission was not granted"))
    } catch (e: Exception) {
      finish(status("unavailable", "Tablet location is not available: ${e.message}"))
    }
  }

  // ---------- window ----------

  @Command
  fun setSystemBars(invoke: Invoke) {
    val args = invoke.parseArgs(BarArgs::class.java)
    activity.runOnUiThread {
      val color = try { Color.parseColor(args.color ?: "#08131F") } catch (e: Exception) { Color.BLACK }
      activity.window.decorView.setBackgroundColor(color)
      val c = WindowCompat.getInsetsController(activity.window, activity.window.decorView)
      c.isAppearanceLightStatusBars = !args.dark
      c.isAppearanceLightNavigationBars = !args.dark
      invoke.resolve(JSObject())
    }
  }

  /** Finish after the web UI has closed the logbook; MainActivity ends the process. */
  @Command
  fun exitApp(invoke: Invoke) {
    invoke.resolve(JSObject())
    activity.runOnUiThread { activity.finishAndRemoveTask() }
  }

  // ---------- helpers ----------

  private fun displayName(uri: Uri): String? {
    return try {
      activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
        if (c.moveToFirst()) c.getString(0) else null
      }
    } catch (e: Exception) {
      null
    }
  }

  private fun safeName(name: String): String {
    val cleaned = name.replace(Regex("[\\\\/:*?\"<>|\\u0000-\\u001f]"), "_").trim().trimStart('.')
    return if (cleaned.isEmpty()) "picked-file" else cleaned.take(120)
  }
}
