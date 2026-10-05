package org.thencloud.plugin

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.Handler
import android.os.Looper

/**
 * What the Music view is playing, for Android to show: a notification, the
 * lock screen, and Bluetooth and headset buttons, all through one
 * MediaSession. PlaybackService keeps the app running in the background while
 * it plays. The WebView has navigator.mediaSession, but shows nothing for it.
 *
 * The page sends its state (mediaUpdate in ThencloudPlugin); buttons go back
 * to it as window.thencloudMedia({ action, seekTime })
 * (web/src/lib/nowplaying.svelte.js), and if nothing there takes them, it all
 * goes away. Everything runs on the main thread.
 */
internal object NowPlaying {
  var title = ""
  var artist = ""
  var album = ""
  var playing = false
  var artwork: Bitmap? = null

  /** Hands an action to the page (set by ThencloudPlugin, which stops it all if nothing takes it). */
  var send: ((action: String, seekTime: Double?) -> Unit)? = null

  var session: MediaSession? = null
    private set

  private val main = Handler(Looper.getMainLooper())

  fun onMain(block: () -> Unit) {
    if (Looper.myLooper() == Looper.getMainLooper()) block() else main.post(block)
  }

  fun action(name: String, seekTime: Double? = null) {
    val send = send ?: return stop(null)
    send(name, seekTime)
  }

  private fun session(context: Context): MediaSession =
    session ?: MediaSession(context.applicationContext, "thencloud").also { s ->
      s.setCallback(object : MediaSession.Callback() {
        override fun onPlay() = action("play")
        override fun onPause() = action("pause")
        override fun onStop() = action("pause")
        override fun onSkipToNext() = action("nexttrack")
        override fun onSkipToPrevious() = action("previoustrack")
        override fun onSeekTo(pos: Long) = action("seekto", pos / 1000.0)
      })
      if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
        @Suppress("DEPRECATION")
        s.setFlags(MediaSession.FLAG_HANDLES_MEDIA_BUTTONS or MediaSession.FLAG_HANDLES_TRANSPORT_CONTROLS)
      }
      openApp(context)?.let { s.setSessionActivity(it) }
      session = s
    }

  fun update(context: Context, duration: Double, position: Double, rate: Float, buffering: Boolean) {
    val s = session(context)
    s.setMetadata(
      MediaMetadata.Builder()
        .putString(MediaMetadata.METADATA_KEY_TITLE, title)
        .putString(MediaMetadata.METADATA_KEY_ARTIST, artist)
        .putString(MediaMetadata.METADATA_KEY_ALBUM, album)
        .putLong(MediaMetadata.METADATA_KEY_DURATION, if (duration > 0) (duration * 1000).toLong() else -1)
        .apply { artwork?.let { putBitmap(MediaMetadata.METADATA_KEY_ART, it) } }
        .build(),
    )
    val state = when {
      playing && buffering -> PlaybackState.STATE_BUFFERING
      playing -> PlaybackState.STATE_PLAYING
      else -> PlaybackState.STATE_PAUSED
    }
    s.setPlaybackState(
      PlaybackState.Builder()
        .setActions(
          PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_PLAY_PAUSE or
            PlaybackState.ACTION_SKIP_TO_NEXT or PlaybackState.ACTION_SKIP_TO_PREVIOUS or
            PlaybackState.ACTION_SEEK_TO or PlaybackState.ACTION_STOP,
        )
        .setState(state, (position * 1000).toLong(), if (state == PlaybackState.STATE_PLAYING) rate else 0f)
        .build(),
    )
    s.isActive = true
    PlaybackService.show(context)
  }

  /** Nothing is playing any more (or the page that played it is gone). */
  fun stop(context: Context?) {
    playing = false
    artwork = null
    session?.let {
      it.isActive = false
      it.release()
    }
    session = null
    PlaybackService.hide(context)
  }

  fun openApp(context: Context) =
    context.packageManager.getLaunchIntentForPackage(context.packageName)?.let {
      it.flags = Intent.FLAG_ACTIVITY_SINGLE_TOP
      android.app.PendingIntent.getActivity(context, 0, it, android.app.PendingIntent.FLAG_IMMUTABLE)
    }
}
