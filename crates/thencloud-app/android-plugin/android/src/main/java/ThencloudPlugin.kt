package org.thencloud.plugin

import android.app.Activity
import android.content.ContentValues
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.util.Base64
import android.view.View
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.ActivityResult
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
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
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
 * Insets: the app draws edge to edge (Android 15 insists), so the page needs
 * to know where the status bar, the navigation bar and the keyboard are.
 * They are set as CSS variables in CSS pixels (--android-inset-top, -bottom,
 * -left and -right), with a keyboard-open class on <html> while the keyboard
 * is up, on every change and on every page load. The keyboard itself shrinks
 * the WebView.
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

  private var insetsScript: ScriptHandler? = null

  override fun load(webView: WebView) {
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
