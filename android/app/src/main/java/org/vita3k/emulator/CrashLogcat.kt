package org.vita3k.emulator

import android.content.Context
import org.vita3k.emulator.data.AppStorage
import java.io.File

// Nova3K: after a native crash, Android writes the details (the allocator's abort message, the backtrace of
// every crashed thread) to the system log, not to our files. On the next start, if nova3k_crash.txt is newer
// than the last saved copy, save the recent warnings and errors of this app to nova3k_logcat.txt next to
// vita3k.log, so they can be sent together. An app may read its own log lines without any permission.
internal object CrashLogcat {
    fun saveAfterCrash(context: Context) {
        Thread {
            try {
                val dir = File(AppStorage.storageRootPath(context))
                val crash = File(dir, "nova3k_crash.txt")
                val out = File(dir, "nova3k_logcat.txt")
                if (!crash.isFile || (out.isFile && out.lastModified() >= crash.lastModified()))
                    return@Thread
                val process = ProcessBuilder("logcat", "-d", "-v", "threadtime", "-t", "4000", "*:W")
                    .redirectErrorStream(true)
                    .start()
                val text = process.inputStream.bufferedReader().readText()
                process.waitFor()
                out.writeText("Nova3K: system log (warnings and errors) saved after the crash in nova3k_crash.txt\n\n$text")
            } catch (e: Throwable) {
                // best effort only
            }
        }.apply { name = "nova3k-logcat"; isDaemon = true }.start()
    }
}
