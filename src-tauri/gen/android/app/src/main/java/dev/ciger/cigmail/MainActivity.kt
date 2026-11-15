package dev.ciger.cigmail

import android.content.Intent
import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // The process outlives this activity (MailService keeps it running), but Tauri only builds
    // its webview for the first activity of a process: a later one would stay blank. Start
    // over in a fresh process instead; it only costs a cold start.
    if (orphaned) {
      super.onCreate(null)
      startActivity(
        Intent(intent).setClass(this, MainActivity::class.java)
          .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
      )
      finish()
      Runtime.getRuntime().exit(0)
      return
    }
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    MailService.start(this)
    // Not on a restore: the link was already handled before the activity was recreated.
    if (savedInstanceState == null) handleMailto(intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    handleMailto(intent)
  }

  /** Opened from a mailto: link elsewhere: open the composer for it. */
  private fun handleMailto(intent: Intent?) {
    // Reopened from Recents: that's the old intent again, already handled.
    if (intent == null || intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0) return
    val uri = intent.data ?: return
    if (uri.scheme.equals("mailto", ignoreCase = true)) NativeBridge.mailto(uri.toString())
  }

  override fun onDestroy() {
    if (!isChangingConfigurations) orphaned = true
    super.onDestroy()
  }

  companion object {
    /** The activity that owned Tauri's webview is gone for good. */
    @Volatile private var orphaned = false
  }
}
