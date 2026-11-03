package org.emcommlog.tablet

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Color
import android.net.Uri
import android.os.Environment
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
import app.tauri.annotation.ActivityCallback
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
@TauriPlugin
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
