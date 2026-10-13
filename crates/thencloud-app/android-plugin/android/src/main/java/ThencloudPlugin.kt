package org.thencloud.plugin

import android.app.Activity
import android.content.ContentValues
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.provider.Settings
import android.util.Base64
import android.view.View
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import androidx.core.content.IntentCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.ScriptHandler
import androidx.webkit.ServiceWorkerClientCompat
import androidx.webkit.ServiceWorkerControllerCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.InputStream
import java.io.OutputStream
import org.json.JSONObject

@InvokeArg
class BeginArgs {
  lateinit var name: String
  var mime: String? = null
}

@InvokeArg
class WriteArgs {
  var id: Int = 0
  lateinit var data: String
}

@InvokeArg
class EndArgs {
  var id: Int = 0
  var ok: Boolean = false
}

@InvokeArg
class IdArgs {
  var id: Int = 0
}

@InvokeArg
class MediaArgs {
  var title: String = ""
  var artist: String = ""
  var album: String = ""
  var duration: Double = 0.0
  var position: Double = 0.0
  var rate: Double = 1.0
  var playing: Boolean = false
  var buffering: Boolean = false
}

@InvokeArg
class ArtworkArgs {
  var data: String? = null
}

/**
 * What the WebView doesn't do on Android by itself.
 *
 * Saving: Android's WebView drops downloads of blob: URLs, which is how the
 * page hands over a decrypted file. Instead the page calls saveBegin, then
 * saveWrite with each piece (base64) and saveEnd. On Android 10 and newer the
 * file goes to Downloads through MediaStore (no permission needed, like a
 * browser download); older versions ask where to save it.
 *
 * Back: the page is asked first (thencloudBack in web/src/lib/native.js) to
 * close what's on top of it.
 *
 * Music: what's playing is shown in a notification and on the lock screen,
 * and keeps playing in the background (NowPlaying, PlaybackService).
 *
 * Insets: the app draws edge to edge (Android 15 insists), so the page needs
 * to know where the status bar, the navigation bar and the keyboard are.
 * They are set as CSS variables in CSS pixels (--android-inset-top, -bottom,
 * -left and -right), with a keyboard-open class on <html> while the keyboard
 * is up, on every change and on every page load. The keyboard itself shrinks
 * the WebView.
 *
 * Network: networkStatus tells the page whether it's connected, on a
 * metered network, and whether Data Saver is on, so the page's on-device
 * file cache knows whether it may refresh a changed file.
 *
 * Sharing: "Share to thencloud" from another app queues its content: Uris
 * (queueIntent, onNewIntent, and the activity's own intent at load() for a
 * cold start); the page lists, reads and clears them (sharePending,
 * shareRead, shareDone). The reverse, sending a file out, writes it into
 * the app's own cache and hands it to the system share sheet
 * (shareOutBegin, shareOutWrite, shareOutEnd).
 */
@TauriPlugin
class ThencloudPlugin(private val activity: Activity) : Plugin(activity) {
  private class Save(val uri: Uri, val out: OutputStream, val pending: Boolean)

  private val saves = HashMap<Int, Save>()
  private var nextId = 1

  @Command
  fun saveBegin(invoke: Invoke) {
    val args = invoke.parseArgs(BeginArgs::class.java)
    val mime = args.mime?.takeIf { it.isNotBlank() } ?: "application/octet-stream"
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      try {
        val values = ContentValues().apply {
          put(MediaStore.Downloads.DISPLAY_NAME, args.name)
          put(MediaStore.Downloads.MIME_TYPE, mime)
          put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
          put(MediaStore.Downloads.IS_PENDING, 1)
        }
        val resolver = activity.contentResolver
        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
          ?: return invoke.reject("Couldn't create the file in Downloads.")
        val out = resolver.openOutputStream(uri, "w")
          ?: return invoke.reject("Couldn't open the file in Downloads.")
        invoke.resolve(started(Save(uri, out, true)))
      } catch (e: Exception) {
        invoke.reject(e.message)
      }
    } else {
      val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        type = mime
        putExtra(Intent.EXTRA_TITLE, args.name)
      }
      startActivityForResult(invoke, intent, "savePicked")
    }
  }

  @ActivityCallback
  fun savePicked(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    if (result.resultCode != Activity.RESULT_OK || uri == null) {
      // Cancelled: not an error, the page just stops.
      invoke.resolve(JSObject().put("id", JSONObject.NULL))
      return
    }
    try {
      val out = activity.contentResolver.openOutputStream(uri, "w")
        ?: return invoke.reject("Couldn't open the file.")
      invoke.resolve(started(Save(uri, out, false)))
    } catch (e: Exception) {
      invoke.reject(e.message)
    }
  }

  private fun started(save: Save): JSObject {
    val id = nextId++
    synchronized(saves) { saves[id] = save }
    return JSObject().put("id", id)
  }

  // Sharing in: Android's share sheet ("Share to thencloud" from another
  // app) delivers a SEND or SEND_MULTIPLE intent carrying one or more
  // content:// Uris, which only native code can read. Queued here by uri,
  // name, mime and size (no bytes yet); the page lists them (sharePending),
  // picks a folder, then pulls each one's plaintext a piece at a time
  // (shareRead, base64) to encrypt and upload like any local file, and
  // tells us it's done (shareDone) so the stream closes. Nothing is copied
  // to disk first.
  private class Shared(val uri: Uri, val name: String, val mime: String, val size: Long, var stream: InputStream? = null)

  private val shares = HashMap<Int, Shared>()
  private val SHARE_CHUNK = 512 * 1024

  /** A SEND/SEND_MULTIPLE intent's Uris, queued for the page to list and import. */
  private fun queueIntent(intent: Intent?) {
    val uris = when (intent?.action) {
      Intent.ACTION_SEND -> IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java)?.let { listOf(it) }
      Intent.ACTION_SEND_MULTIPLE -> IntentCompat.getParcelableArrayListExtra(intent, Intent.EXTRA_STREAM, Uri::class.java)
      else -> null
    } ?: return
    val resolver = activity.contentResolver
    for (uri in uris) {
      var name = uri.lastPathSegment ?: "shared"
      var size = -1L
      resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
        if (c.moveToFirst()) {
          c.getColumnIndex(OpenableColumns.DISPLAY_NAME).takeIf { it >= 0 }?.let { name = c.getString(it) ?: name }
          c.getColumnIndex(OpenableColumns.SIZE).takeIf { it >= 0 }?.let { size = c.getLong(it) }
        }
      }
      val mime = resolver.getType(uri) ?: "application/octet-stream"
      synchronized(shares) { shares[nextId++] = Shared(uri, name, mime, size) }
    }
  }

  /** Pending shared files not yet imported: `[{ id, name, mime, size }]`. */
  @Command
  fun sharePending(invoke: Invoke) {
    val items = JSArray()
    synchronized(shares) { shares.entries.sortedBy { it.key } }.forEach { (id, s) ->
      items.put(JSObject().put("id", id).put("name", s.name).put("mime", s.mime).put("size", s.size))
    }
    invoke.resolve(JSObject().put("items", items))
  }

  /** The next piece of a pending share's plaintext (base64), opening it on the first call. `eof` when there's no more. */
  @Command
  fun shareRead(invoke: Invoke) {
    val args = invoke.parseArgs(IdArgs::class.java)
    val shared = synchronized(shares) { shares[args.id] } ?: return invoke.reject("No such shared file.")
    try {
      val stream = shared.stream ?: activity.contentResolver.openInputStream(shared.uri)?.also { shared.stream = it }
        ?: return invoke.reject("Couldn't open the shared file.")
      val buf = ByteArray(SHARE_CHUNK)
      var read = 0
      // A content:// stream may return a short read before more is ready.
      while (read < buf.size) {
        val n = stream.read(buf, read, buf.size - read)
        if (n < 0) break
        read += n
      }
      invoke.resolve(JSObject().put("data", Base64.encodeToString(buf, 0, read, Base64.NO_WRAP)).put("eof", read < buf.size))
    } catch (e: Exception) {
      invoke.reject(e.message)
    }
  }

  /** Done with a pending share (imported, failed or dismissed): closes its stream. */
  @Command
  fun shareDone(invoke: Invoke) {
    val args = invoke.parseArgs(IdArgs::class.java)
    val shared = synchronized(shares) { shares.remove(args.id) }
    try {
      shared?.stream?.close()
    } catch (_: Exception) {
    }
    invoke.resolve()
  }

  // Sharing out: a decrypted file, written into our own cache (shared_out/,
  // pruned of anything older than an hour before each new one starts, since
  // we can't tell when the other app is done reading it) and handed to
  // another app as a content:// Uri through the FileProvider Tauri's
  // Android template already declares, via the system share sheet.
  private class ShareOut(val file: File, val out: OutputStream, val mime: String)

  private val shareOuts = HashMap<Int, ShareOut>()

  @Command
  fun shareOutBegin(invoke: Invoke) {
    val args = invoke.parseArgs(BeginArgs::class.java)
    val mime = args.mime?.takeIf { it.isNotBlank() } ?: "application/octet-stream"
    try {
      val dir = File(activity.cacheDir, "shared_out").apply { mkdirs() }
      val cutoff = System.currentTimeMillis() - 60 * 60 * 1000
      dir.listFiles()?.forEach { if (it.lastModified() < cutoff) it.delete() }
      val name = args.name.replace('/', '_').replace('\\', '_').trim().ifEmpty { "shared" }
      val file = File(dir, "${nextId}_$name")
      val out = file.outputStream()
      val id = nextId++
      synchronized(shareOuts) { shareOuts[id] = ShareOut(file, out, mime) }
      invoke.resolve(JSObject().put("id", id))
    } catch (e: Exception) {
      invoke.reject(e.message)
    }
  }

  @Command
  fun shareOutWrite(invoke: Invoke) {
    val args = invoke.parseArgs(WriteArgs::class.java)
    val out = synchronized(shareOuts) { shareOuts[args.id] }?.out ?: return invoke.reject("No such share.")
    try {
      out.write(Base64.decode(args.data, Base64.DEFAULT))
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject(e.message)
    }
  }

  /** Finish a share-out; with ok, hands the file to the system share sheet. */
  @Command
  fun shareOutEnd(invoke: Invoke) {
    val args = invoke.parseArgs(EndArgs::class.java)
    val share = synchronized(shareOuts) { shareOuts.remove(args.id) } ?: return invoke.resolve()
    try {
      share.out.close()
      if (!args.ok) {
        share.file.delete()
        return invoke.resolve()
      }
      val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", share.file)
      val send = Intent(Intent.ACTION_SEND).apply {
        type = share.mime
        putExtra(Intent.EXTRA_STREAM, uri)
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      }
      activity.startActivity(Intent.createChooser(send, null))
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject(e.message)
    }
  }

  @Command
  fun saveWrite(invoke: Invoke) {
    val args = invoke.parseArgs(WriteArgs::class.java)
    val save = synchronized(saves) { saves[args.id] } ?: return invoke.reject("No such download.")
    try {
      save.out.write(Base64.decode(args.data, Base64.DEFAULT))
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject(e.message)
    }
  }

  /** Finish a save; with ok = false (failed or cancelled) the partial file is deleted. */
  @Command
  fun saveEnd(invoke: Invoke) {
    val args = invoke.parseArgs(EndArgs::class.java)
    val save = synchronized(saves) { saves.remove(args.id) } ?: return invoke.resolve()
    val resolver = activity.contentResolver
    try {
      save.out.close()
      if (!args.ok) {
        if (save.pending) resolver.delete(save.uri, null, null)
        else DocumentsContract.deleteDocument(resolver, save.uri)
      } else if (save.pending) {
        resolver.update(save.uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
      }
      invoke.resolve()
    } catch (e: Exception) {
      invoke.reject(e.message)
    }
  }

  /** What the Music view is playing (web/src/lib/nowplaying.svelte.js). */
  @Command
  fun mediaUpdate(invoke: Invoke) {
    val args = invoke.parseArgs(MediaArgs::class.java)
    NowPlaying.onMain {
      // Shown as text by the system; tags can be anything, so keep them short.
      NowPlaying.title = args.title.take(300)
      NowPlaying.artist = args.artist.take(300)
      NowPlaying.album = args.album.take(300)
      NowPlaying.playing = args.playing
      NowPlaying.update(activity, args.duration, args.position, args.rate.toFloat(), args.buffering)
      invoke.resolve()
    }
  }

  /** The cover, a JPEG of at most 512 pixels a side that the page made itself (base64), or none. */
  @Command
  fun mediaArtwork(invoke: Invoke) {
    val data = invoke.parseArgs(ArtworkArgs::class.java).data
    val bitmap = data?.let {
      val bytes = Base64.decode(it, Base64.DEFAULT)
      BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
    }
    NowPlaying.onMain {
      NowPlaying.artwork = bitmap
      invoke.resolve()
    }
  }

  @Command
  fun mediaStop(invoke: Invoke) {
    NowPlaying.onMain {
      NowPlaying.stop(activity)
      invoke.resolve()
    }
  }

  /** Whether refreshing cached ciphertext is allowed on the current network. */
  @Command
  fun networkStatus(invoke: Invoke) {
    val manager = activity.getSystemService(ConnectivityManager::class.java)
    val capabilities = manager.activeNetwork?.let { manager.getNetworkCapabilities(it) }
    val connected = capabilities?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) == true
    val metered = capabilities == null ||
      !capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
    val dataSaver = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
      manager.restrictBackgroundStatus != ConnectivityManager.RESTRICT_BACKGROUND_STATUS_DISABLED
    } else {
      Settings.Global.getInt(activity.contentResolver, "restrict_background_data", 0) == 1
    }
    invoke.resolve(
      JSObject()
        .put("connected", connected)
        .put("metered", metered)
        .put("dataSaver", dataSaver),
    )
  }

  // The one with the activity takes AppCompatActivity, which the plugin
  // doesn't otherwise need.
  @Suppress("OVERRIDE_DEPRECATION")
  override fun onDestroy() {
    // The page goes with the activity, and its music with it.
    NowPlaying.send = null
    NowPlaying.stop(activity)
  }

  private var sharedWebView: WebView? = null

  /** The app was already open: tell the page a new share arrived. */
  override fun onNewIntent(intent: Intent) {
    activity.setIntent(intent)
    queueIntent(intent)
    notifyShareArrived()
  }

  private fun notifyShareArrived() {
    if (synchronized(shares) { shares.isEmpty() }) return
    sharedWebView?.evaluateJavascript(
      "(function(){try{window.thencloudShareArrived&&window.thencloudShareArrived()}catch(e){}})()",
      null,
    )
  }

  private var insetsScript: ScriptHandler? = null

  override fun load(webView: WebView) {
    sharedWebView = webView
    // A share that opened the app fresh: onNewIntent isn't called for the
    // intent an activity was started with, only for ones delivered later.
    queueIntent(activity.intent)

    // Buttons in the notification, on the lock screen or on headphones. If
    // the page has no player to take them (it was reloaded, or signed out),
    // the notification goes.
    NowPlaying.send = { action, seekTime ->
      val details = JSONObject().put("action", action).apply { seekTime?.let { put("seekTime", it) } }
      webView.evaluateJavascript("(function(){try{return !!(window.thencloudMedia&&window.thencloudMedia($details))}catch(e){return false}})()") { taken ->
        if (taken != "true") NowPlaying.stop(activity)
      }
    }

    ViewCompat.setOnApplyWindowInsetsListener(webView) { view, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
      // Edge to edge, the window no longer shrinks for the keyboard and the
      // WebView doesn't notice it, so a field near the bottom would end up
      // under it. Shrink the WebView instead (it's the activity's content).
      (view.parent as? View)?.let { if (it.paddingBottom != keyboard) it.setPadding(0, 0, 0, keyboard) }
      val density = view.resources.displayMetrics.density
      fun css(px: Int) = "${Math.round(px / density)}px"
      // The keyboard covers the navigation bar.
      val bottom = if (keyboard > 0) 0 else bars.bottom
      setInsets(
        webView,
        "var s=html.style;" +
          "s.setProperty('--android-inset-top','${css(bars.top)}');" +
          "s.setProperty('--android-inset-bottom','${css(bottom)}');" +
          "s.setProperty('--android-inset-left','${css(bars.left)}');" +
          "s.setProperty('--android-inset-right','${css(bars.right)}');" +
          "html.classList.toggle('keyboard-open',${keyboard > 0});",
      )
      ViewCompat.onApplyWindowInsets(view, insets)
    }
    ViewCompat.requestApplyInsets(webView)

    // The app's pages come from wry's request handler on the WebView's client,
    // which a service worker's requests skip, so it couldn't even load
    // /sw.js. Send them the same way: then video, music and big previews
    // stream through it (web/src/lib/stream.js) instead of being decrypted
    // into memory whole.
    if (WebViewFeature.isFeatureSupported(WebViewFeature.SERVICE_WORKER_BASIC_USAGE) &&
      WebViewFeature.isFeatureSupported(WebViewFeature.SERVICE_WORKER_SHOULD_INTERCEPT_REQUEST) &&
      WebViewFeature.isFeatureSupported(WebViewFeature.GET_WEB_VIEW_CLIENT)
    ) {
      val client = WebViewCompat.getWebViewClient(webView)
      ServiceWorkerControllerCompat.getInstance().setServiceWorkerClient(object : ServiceWorkerClientCompat() {
        override fun shouldInterceptRequest(request: WebResourceRequest): WebResourceResponse? =
          client.shouldInterceptRequest(webView, request)
      })
    }

    // Back closes what's on top in the page (a menu, a dialog, a preview)
    // before it goes back in history or leaves the app, which is what the
    // callback registered before this one (WryActivity's) does.
    (activity as? ComponentActivity)?.let { owner ->
      owner.onBackPressedDispatcher.addCallback(owner, object : OnBackPressedCallback(true) {
        override fun handleOnBackPressed() {
          webView.evaluateJavascript("(function(){try{return !!(window.thencloudBack&&window.thencloudBack())}catch(e){return false}})()") { closed ->
            if (closed != "true") {
              isEnabled = false
              owner.onBackPressedDispatcher.onBackPressed()
              isEnabled = true
            }
          }
        }
      })
    }
  }

  /** Run `js` with `html` as the page's <html>, now and on every page from here on. */
  private fun setInsets(webView: WebView, js: String) {
    // At document start there may be no <html> yet; then it runs once there is.
    val wrapped = "(function(){function apply(html){$js}" +
      "if(document.documentElement)return apply(document.documentElement);" +
      "var o=new MutationObserver(function(){if(document.documentElement){o.disconnect();apply(document.documentElement)}});" +
      "o.observe(document,{childList:true})})();"
    webView.evaluateJavascript(wrapped, null)
    // Pages loaded later (signing in loads another one) get them from the start.
    if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
      insetsScript?.remove()
      insetsScript = WebViewCompat.addDocumentStartJavaScript(webView, wrapped, setOf("*"))
    }
  }
}
