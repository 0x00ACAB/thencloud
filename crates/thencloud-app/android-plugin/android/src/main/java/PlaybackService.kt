package org.thencloud.plugin

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.graphics.drawable.Icon
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper

/**
 * Keeps the app running while music plays with the app in the background or
 * the screen off, which Android allows only to a foreground service with a
 * notification: the media notification from NowPlaying's session. Paused for
 * half a minute, it stops being a foreground service but keeps the
 * notification (which can then be swiped away), so Android may close the app
 * as it would any other.
 */
class PlaybackService : Service() {
  companion object {
    private const val CHANNEL = "playing"
    private const val ID = 1
    private const val PLAY_PAUSE = "org.thencloud.plugin.PLAY_PAUSE"
    private const val NEXT = "org.thencloud.plugin.NEXT"
    private const val PREVIOUS = "org.thencloud.plugin.PREVIOUS"
    private const val DISMISS = "org.thencloud.plugin.DISMISS"
    private const val PAUSED_GRACE_MS = 30_000L

    private var running: PlaybackService? = null

    /** Show (or refresh) the notification for NowPlaying's state. */
    fun show(context: Context) {
      running?.let { return it.refresh() }
      if (!NowPlaying.playing) return
      // Started from the page while it's in front, which is allowed. From the
      // background (a track ending when the service was stopped) it isn't,
      // and then there's nothing to do but leave it.
      try {
        val intent = Intent(context, PlaybackService::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) context.startForegroundService(intent)
        else context.startService(intent)
      } catch (e: IllegalStateException) {
        android.util.Log.w("thencloud", "Couldn't keep playing in the background", e)
      }
    }

    fun hide(context: Context?) {
      running?.let {
        it.stopForeground(Service.STOP_FOREGROUND_REMOVE)
        it.stopSelf()
      }
        ?: context?.getSystemService(NotificationManager::class.java)?.cancel(ID)
    }
  }

  private var foreground = false
  private var noisy = false

  // Headphones unplugged or Bluetooth gone: pause rather than play out loud.
  private val becomingNoisy = object : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
      if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY && NowPlaying.playing) NowPlaying.action("pause")
    }
  }

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onCreate() {
    super.onCreate()
    running = this
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      getSystemService(NotificationManager::class.java).createNotificationChannel(
        NotificationChannel(CHANNEL, getString(R.string.thencloud_playing_channel), NotificationManager.IMPORTANCE_LOW).apply {
          setShowBadge(false)
        },
      )
    }
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == DISMISS) {
      hide(this)
      return START_NOT_STICKY
    }
    // Started with startForegroundService (or from the notification), it has
    // to become one at once, whatever happens next.
    promote(notification())
    if (NowPlaying.session == null) {
      // The page that was playing is gone (the app was closed).
      hide(this)
      return START_NOT_STICKY
    }
    when (intent?.action) {
      PLAY_PAUSE -> NowPlaying.action(if (NowPlaying.playing) "pause" else "play")
      NEXT -> NowPlaying.action("nexttrack")
      PREVIOUS -> NowPlaying.action("previoustrack")
    }
    refresh()
    return START_NOT_STICKY
  }

  override fun onDestroy() {
    handler.removeCallbacks(demote)
    if (noisy) unregisterReceiver(becomingNoisy)
    noisy = false
    running = null
    super.onDestroy()
  }

  /** The app was swiped away from recents: its page, and the music, are gone. */
  override fun onTaskRemoved(rootIntent: Intent?) {
    NowPlaying.stop(this)
    super.onTaskRemoved(rootIntent)
  }

  private fun promote(n: Notification) {
    try {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) startForeground(ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
      else startForeground(ID, n)
      foreground = true
    } catch (e: IllegalStateException) {
      // Not allowed from the background right now: show it anyway.
      getSystemService(NotificationManager::class.java).notify(ID, n)
    }
  }

  // Going from one track to the next pauses for a moment, and a service
  // that has stopped being a foreground one can't become one again from the
  // background. So it only stops once it has been paused for a while.
  private val handler = Handler(Looper.getMainLooper())
  private val demote = Runnable {
    if (!NowPlaying.playing && foreground) {
      stopForeground(STOP_FOREGROUND_DETACH)
      foreground = false
      getSystemService(NotificationManager::class.java).notify(ID, notification())
    }
  }

  fun refresh() {
    if (NowPlaying.session == null) return
    val n = notification()
    handler.removeCallbacks(demote)
    if (NowPlaying.playing) {
      promote(n)
      if (!noisy) registerReceiver(becomingNoisy, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY))
      noisy = true
    } else {
      if (noisy) unregisterReceiver(becomingNoisy)
      noisy = false
      getSystemService(NotificationManager::class.java).notify(ID, n)
      if (foreground) handler.postDelayed(demote, PAUSED_GRACE_MS)
    }
  }

  private fun pending(action: String): PendingIntent {
    val intent = Intent(this, PlaybackService::class.java).setAction(action)
    val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && action != DISMISS) PendingIntent.getForegroundService(this, 0, intent, flags)
    else PendingIntent.getService(this, 0, intent, flags)
  }

  private fun button(icon: Int, label: Int, action: String) =
    Notification.Action.Builder(Icon.createWithResource(this, icon), getString(label), pending(action)).build()

  private fun notification(): Notification {
    val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) Notification.Builder(this, CHANNEL) else @Suppress("DEPRECATION") Notification.Builder(this)
    val playing = NowPlaying.playing
    builder
      .setSmallIcon(R.drawable.thencloud_playing)
      .setContentTitle(NowPlaying.title)
      .setContentText(NowPlaying.artist)
      .setLargeIcon(NowPlaying.artwork)
      .setContentIntent(NowPlaying.openApp(this))
      .setDeleteIntent(pending(DISMISS))
      .setVisibility(Notification.VISIBILITY_PUBLIC)
      .setOngoing(playing)
      .setShowWhen(false)
      .addAction(button(android.R.drawable.ic_media_previous, R.string.thencloud_previous, PREVIOUS))
      .addAction(
        if (playing) button(android.R.drawable.ic_media_pause, R.string.thencloud_pause, PLAY_PAUSE)
        else button(android.R.drawable.ic_media_play, R.string.thencloud_play, PLAY_PAUSE),
      )
      .addAction(button(android.R.drawable.ic_media_next, R.string.thencloud_next, NEXT))
    NowPlaying.session?.let {
      builder.setStyle(Notification.MediaStyle().setMediaSession(it.sessionToken).setShowActionsInCompactView(0, 1, 2))
    }
    return builder.build()
  }
}
